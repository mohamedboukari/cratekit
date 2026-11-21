//! The soft clipper before the limiter (#144): it takes the tips off the
//! highest peaks (hat and snare transients) with a smooth curve, so the
//! limiter has less to pull down and a master gets louder at the same
//! -1.0 dBTP ceiling (Club's -8 LUFS on hat-heavy tracks, where the limiter
//! alone stops near -8.8).
//!
//! The knee sits 3 dB above the limiter's ceiling: only peaks the limiter
//! would otherwise pull down by more than that are clipped. A moderate
//! master never reaches it, so it comes out exactly as before, only delayed.
//!
//! How it works, per sample and channel:
//! 1. **Between samples.** Three points between two samples are
//!    interpolated (the limiter's 4x windowed-sinc filter), so a peak
//!    between samples is clipped too.
//! 2. **The cut.** Each of the four points over the knee follows a smooth
//!    curve (`tanh`) that never goes more than 3 dB over the knee, and never
//!    takes more than 6 dB off a peak (past that the limiter does the rest);
//!    the cut is what the curve takes off.
//! 3. **No aliasing.** Only the cut is filtered: a low pass at the base
//!    rate's Nyquist frequency (Kaiser-windowed sinc at 4x) turns the cut
//!    back into the base rate, so the harmonics clipping makes above it are
//!    removed, and the cut is added to the input, delayed to line up. Where
//!    nothing is clipped the filter is skipped, which keeps it cheap.
//!
//! Processing never allocates (every buffer is sized in [`Clipper::new`]), so
//! the live preview (#26) can run it in the audio callback.

use super::limiter::{HALF_TAPS, KAISER_BETA, OVERSAMPLE, Ring, bessel_i0, interpolation_taps};

/// The knee over the limiter's ceiling, in dB.
pub const KNEE_OVER_CEILING_DB: f32 = 3.0;
/// How far over the knee the curve can reach, in dB (its asymptote).
const SOFT_DB: f32 = 3.0;
/// The most taken off any peak, in dB. Past that the curve rises with the
/// input again (cutting exactly this much) and the limiter does the rest, so
/// a very loud target cannot be reached by clipping the music to pieces.
pub const MAX_CUT_DB: f32 = 6.0;
/// The decimation filter spans this many base samples on each side.
const DECIMATION_HALF: usize = 16;
/// Taps of the decimation filter, at 4x (odd, so it is centred on a point).
const DECIMATION_TAPS: usize = 2 * OVERSAMPLE * DECIMATION_HALF + 1;
/// The decimation filter's cutoff, as a fraction of the base rate: under the
/// Nyquist frequency (0.5), so the filter is already far down there and
/// clipping products just above it barely fold back.
const CUTOFF: f64 = 0.45;
/// The most channels [`clip`] handles (7.1 surround); more are left as they
/// are.
const MAX_FRAME_CHANNELS: usize = 8;

/// A clipper's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipperSettings {
    /// Where the curve starts, in dB of full scale (peaks between samples
    /// included).
    pub knee_db: f32,
}

impl ClipperSettings {
    /// The clipper in front of a limiter with this ceiling: the knee 3 dB
    /// above it.
    #[must_use]
    pub fn for_ceiling(ceiling_dbtp: f32) -> Self {
        Self {
            knee_db: ceiling_dbtp + KNEE_OVER_CEILING_DB,
        }
    }
}

impl Default for ClipperSettings {
    /// In front of the SPEC's -1.0 dBTP limiter: the knee at +2 dB.
    fn default() -> Self {
        Self::for_ceiling(-1.0)
    }
}

/// The decimation low pass at 4x: a Kaiser-windowed sinc with its cutoff at
/// 0.45 of the base rate, its taps summing to 1.
fn decimation_taps() -> [f32; DECIMATION_TAPS] {
    #[expect(clippy::cast_precision_loss, reason = "a small tap count")]
    let centre = (DECIMATION_TAPS - 1) as f64 / 2.0;
    #[expect(clippy::cast_precision_loss, reason = "the oversampling factor")]
    let cutoff = CUTOFF / OVERSAMPLE as f64;
    let norm = bessel_i0(KAISER_BETA);
    let mut weights = [0.0_f64; DECIMATION_TAPS];
    for (index, weight) in weights.iter_mut().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "a small tap index")]
        let x = index as f64 - centre;
        let sinc = if x == 0.0 {
            2.0 * cutoff
        } else {
            (std::f64::consts::TAU * cutoff * x).sin() / (std::f64::consts::PI * x)
        };
        let r = x / centre;
        let window = bessel_i0(KAISER_BETA * (1.0 - r * r).max(0.0).sqrt()) / norm;
        *weight = sinc * window;
    }
    let sum: f64 = weights.iter().sum();
    #[expect(clippy::cast_possible_truncation, reason = "filter weights")]
    let taps = weights.map(|weight| (weight / sum) as f32);
    taps
}

/// The curve: unchanged up to `knee`, then a smooth rise that never goes
/// more than `room` over it, and never below the input less `MAX_CUT_DB`.
fn soft_clip(knee: f32, room: f32, value: f32) -> f32 {
    let level = value.abs();
    if level <= knee {
        value
    } else {
        let curve = knee + room * ((level - knee) / room).tanh();
        curve.max(level * MAX_CUT_RATIO).copysign(value)
    }
}

/// `MAX_CUT_DB` as a gain (`10^(-6 / 20)`).
const MAX_CUT_RATIO: f32 = 0.501_187_2;

/// A 4x oversampled soft clipper for planar audio.
#[derive(Debug, Clone)]
pub struct Clipper {
    /// Where the curve starts, linear.
    knee: f32,
    /// How far over the knee the curve can reach, linear.
    room: f32,
    taps: [[f32; 2 * HALF_TAPS]; OVERSAMPLE - 1],
    /// The most an interpolated point can be over the window's largest
    /// sample (the largest sum of absolute weights): a window whose samples
    /// all stay under `knee / reach` cannot clip.
    reach: f32,
    decimation: [f32; DECIMATION_TAPS],
    /// Per channel: the last `2 * HALF_TAPS` input samples.
    history: Vec<Ring<f32>>,
    /// Per channel: the input waiting to come out.
    delay: Vec<Ring<f32>>,
    /// Per channel: the cut at 4x, the decimation filter's span.
    cut: Vec<Ring<f32>>,
    /// Per channel: cut points pushed since the last one that was not zero
    /// (at the filter's length or more, the correction is zero).
    quiet: Vec<usize>,
    /// The smallest ratio of a clipped point to the point (1 when nothing
    /// was clipped).
    least_ratio: f32,
    /// Points looked at and points clipped (four per sample and channel).
    points: u64,
    clipped_points: u64,
    /// Off, the clipper only delays (so what follows stays aligned).
    enabled: bool,
}

impl Clipper {
    /// A clipper for `channels` channels. A knee that is not a number
    /// becomes the default.
    #[must_use]
    pub fn new(channels: usize, settings: ClipperSettings) -> Self {
        let knee_db = if settings.knee_db.is_finite() {
            settings.knee_db
        } else {
            ClipperSettings::default().knee_db
        };
        let knee = 10_f32.powf(knee_db / 20.0);
        let taps = interpolation_taps();
        let reach = taps
            .iter()
            .map(|row| row.iter().map(|w| w.abs()).sum::<f32>())
            .fold(1.0_f32, f32::max);
        Self {
            knee,
            room: knee * (10_f32.powf(SOFT_DB / 20.0) - 1.0),
            taps,
            reach,
            decimation: decimation_taps(),
            history: vec![Ring::new(2 * HALF_TAPS, 0.0); channels],
            delay: vec![Ring::new(Self::LATENCY + 1, 0.0); channels],
            cut: vec![Ring::new(DECIMATION_TAPS, 0.0); channels],
            quiet: vec![DECIMATION_TAPS; channels],
            least_ratio: 1.0,
            points: 0,
            clipped_points: 0,
            enabled: true,
        }
    }

    /// How many samples the output lags the input: the interpolation's
    /// look-ahead plus the decimation filter's half length.
    pub const LATENCY: usize = HALF_TAPS - 1 + DECIMATION_HALF;

    /// How many samples the output lags the input ([`Self::LATENCY`]).
    #[must_use]
    pub fn latency(&self) -> usize {
        Self::LATENCY
    }

    /// The most the clipper has taken off a peak since it was made or reset,
    /// in dB (0 when it never clipped).
    #[must_use]
    pub fn largest_cut_db(&self) -> f32 {
        -20.0 * self.least_ratio.log10()
    }

    /// Forgets the signal so far (a new track, a seek). Never allocates.
    pub fn reset(&mut self) {
        for ring in self
            .history
            .iter_mut()
            .chain(&mut self.delay)
            .chain(&mut self.cut)
        {
            ring.clear(0.0);
        }
        self.quiet.fill(DECIMATION_TAPS);
        self.least_ratio = 1.0;
        self.points = 0;
        self.clipped_points = 0;
    }

    /// The share of the points clipped since the clipper was made or reset,
    /// 0 to 1 (four points per sample and channel, between samples
    /// included).
    #[must_use]
    pub fn clipped_share(&self) -> f32 {
        if self.points == 0 {
            0.0
        } else {
            #[expect(clippy::cast_precision_loss, reason = "a share, 0 to 1")]
            let share = self.clipped_points as f32 / self.points as f32;
            share
        }
    }

    /// Turns clipping on or off; off, the clipper only delays by
    /// [`Self::LATENCY`], so what follows stays aligned. Never allocates.
    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled != self.enabled {
            self.enabled = enabled;
            for ring in &mut self.cut {
                ring.clear(0.0);
            }
            self.quiet.fill(DECIMATION_TAPS);
        }
    }

    /// Clips one frame (one sample per channel, as many as given to
    /// [`Self::new`]) in place: the input goes in and the frame
    /// [`Self::LATENCY`] samples older comes out, clipped. A sample that is
    /// not a number is taken as silence. Channels beyond the count are left
    /// as they are.
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        let channels = self
            .history
            .iter_mut()
            .zip(&mut self.delay)
            .zip(&mut self.cut)
            .zip(&mut self.quiet);
        for (sample, (((history, delay), cut), quiet)) in frame.iter_mut().zip(channels) {
            let input = if sample.is_finite() { *sample } else { 0.0 };
            history.push(input);
            let mut window = [0.0_f32; 2 * HALF_TAPS];
            history.ordered(&mut window);
            let loudest = window.iter().fold(0.0_f32, |peak, x| peak.max(x.abs()));
            self.points = self.points.saturating_add(OVERSAMPLE as u64);
            if !self.enabled || loudest * self.reach <= self.knee {
                // Nothing in reach of the knee: four zero cuts.
                for _ in 0..OVERSAMPLE {
                    cut.push(0.0);
                }
                *quiet = quiet.saturating_add(OVERSAMPLE);
            } else {
                // The three points after the sample at age HALF_TAPS, then
                // the sample at age HALF_TAPS - 1, oldest first.
                let newer = window.get(HALF_TAPS).copied().unwrap_or(0.0);
                for point in 0..OVERSAMPLE {
                    let value = self.taps.get(point).map_or(newer, |taps| {
                        taps.iter().zip(&window).map(|(w, x)| w * x).sum()
                    });
                    let clipped = soft_clip(self.knee, self.room, value);
                    let removed = clipped - value;
                    cut.push(removed);
                    if removed == 0.0 {
                        *quiet = quiet.saturating_add(1);
                    } else {
                        *quiet = 0;
                        self.clipped_points = self.clipped_points.saturating_add(1);
                        self.least_ratio = self.least_ratio.min(clipped.abs() / value.abs());
                    }
                }
            }
            let correction = if *quiet >= DECIMATION_TAPS {
                0.0
            } else {
                let mut span = [0.0_f32; DECIMATION_TAPS];
                cut.ordered(&mut span);
                span.iter().zip(&self.decimation).map(|(c, w)| c * w).sum()
            };
            delay.push(input);
            *sample = delay.get(Self::LATENCY) + correction;
        }
    }

    /// Clips `channels` in place, planar, as a stream: the output lags by
    /// [`Self::LATENCY`] samples (see [`clip`] for a whole signal).
    pub fn process<C: AsMut<[f32]>>(&mut self, channels: &mut [C]) {
        let frames = channels
            .iter_mut()
            .map(|c| c.as_mut().len())
            .min()
            .unwrap_or(0);
        let mut frame = [0.0_f32; MAX_FRAME_CHANNELS];
        let count = channels.len().min(MAX_FRAME_CHANNELS);
        for index in 0..frames {
            for (slot, channel) in frame.iter_mut().zip(channels.iter_mut()) {
                *slot = channel.as_mut().get(index).copied().unwrap_or(0.0);
            }
            self.process_frame(&mut frame[..count]);
            for (slot, channel) in frame.iter().zip(channels.iter_mut()) {
                if let Some(out) = channel.as_mut().get_mut(index) {
                    *out = *slot;
                }
            }
        }
    }
}

/// Clips a whole signal in place, planar (channels of the same length; a
/// longer channel's extra samples are left as they are), with the delay
/// taken out: the output lines up with the input and has the same length.
/// Returns the share of the signal clipped, 0 to 1 (see
/// [`Clipper::clipped_share`]).
pub fn clip<C: AsMut<[f32]>>(channels: &mut [C], settings: ClipperSettings) -> f32 {
    let count = channels.len().min(MAX_FRAME_CHANNELS);
    let mut clipper = Clipper::new(count, settings);
    // Nothing anywhere in reach of the knee (between samples included): the
    // signal would come out exactly as it went in, so it is left alone.
    let loudest = channels
        .iter_mut()
        .take(count)
        .flat_map(|channel| channel.as_mut().iter())
        .fold(0.0_f32, |peak, &x| {
            if x.is_finite() {
                peak.max(x.abs())
            } else {
                f32::INFINITY
            }
        });
    if loudest * clipper.reach <= clipper.knee {
        return 0.0;
    }
    let latency = Clipper::LATENCY;
    let frames = channels
        .iter_mut()
        .map(|c| c.as_mut().len())
        .min()
        .unwrap_or(0);
    let mut frame = [0.0_f32; MAX_FRAME_CHANNELS];
    // Feed the signal, then `latency` frames of silence to flush it out; the
    // output of input frame n arrives at step n + latency.
    for step in 0..frames + latency {
        for (slot, channel) in frame.iter_mut().zip(channels.iter_mut()).take(count) {
            *slot = channel.as_mut().get(step).copied().unwrap_or(0.0);
        }
        clipper.process_frame(&mut frame[..count]);
        if let Some(index) = step.checked_sub(latency) {
            for (slot, channel) in frame.iter().zip(channels.iter_mut()).take(count) {
                if let Some(out) = channel.as_mut().get_mut(index) {
                    *out = *slot;
                }
            }
        }
    }
    clipper.clipped_share()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::key::{Key, Mode, PitchClass};
    use crate::synth;

    const RATE: u32 = 48_000;

    fn stereo(samples: Vec<f32>) -> Vec<Vec<f32>> {
        vec![samples.clone(), samples]
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0_f32, |p, x| p.max(x.abs()))
    }

    fn db(value: f32) -> f32 {
        20.0 * value.log10()
    }

    #[test]
    fn a_signal_under_the_knee_comes_out_unchanged() {
        let key = Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        };
        // Up to +1 dBFS: under the +2 dB knee, peaks between samples too.
        let input = stereo(synth::normalize(
            synth::test_loop(126.0, key, 3.0, RATE),
            1.0,
        ));
        let mut output = input.clone();
        assert_eq!(clip(&mut output, ClipperSettings::default()), 0.0);
        assert_eq!(output, input, "bit for bit");
    }

    #[test]
    fn a_signal_far_under_the_knee_is_not_even_processed() {
        // Same answer whether the whole-signal shortcut applies or not.
        let input = stereo(synth::sine(1000.0, -6.0, 0.5, RATE));
        let mut shortcut = input.clone();
        assert_eq!(clip(&mut shortcut, ClipperSettings::default()), 0.0);
        let mut streamed = input.clone();
        let mut clipper = Clipper::new(2, ClipperSettings::default());
        clipper.process(&mut streamed);
        let lag = Clipper::LATENCY;
        assert_eq!(shortcut[0][..input[0].len() - lag], streamed[0][lag..]);
        assert_eq!(shortcut, input);
    }

    #[test]
    fn peaks_over_the_knee_come_down_smoothly() {
        // A 1 kHz sine 8 dB over full scale: 6 dB over the knee.
        let input = stereo(synth::sine(1000.0, 8.0, 1.0, RATE));
        let mut output = input.clone();
        let mut clipper = Clipper::new(2, ClipperSettings::default());
        clipper.process(&mut output);
        let cut = clipper.largest_cut_db();
        assert!(cut > 2.0, "cut {cut:.2} dB");
        assert!(
            clipper.clipped_share() > 0.2,
            "most of each cycle is over the knee"
        );
        let output: Vec<Vec<f32>> = output
            .iter()
            .map(|channel| channel[Clipper::LATENCY..].to_vec())
            .collect();
        let settled = &output[0][4_800..43_000];
        // Never past the curve's top (+5 dB), give or take the filtering.
        assert!(db(peak(settled)) < 5.3, "{:.2} dBFS", db(peak(settled)));
        assert!(
            db(peak(settled)) > 3.0,
            "still loud: {:.2} dBFS",
            db(peak(settled))
        );
        // Smooth: no jump bigger than the sine's own steepest step.
        let steepest = input[0]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        let largest = settled
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(largest <= steepest * 1.05, "{largest} vs {steepest}");
    }

    #[test]
    fn clipping_makes_far_less_aliasing_than_clipping_each_sample() {
        // A 5 kHz sine driven 10 dB over full scale: its 7th harmonic
        // (35 kHz) folds back to 13 kHz when clipped sample by sample.
        let input = synth::sine(5_000.0, 10.0, 1.0, RATE);
        let mut oversampled = stereo(input.clone());
        clip(&mut oversampled, ClipperSettings::default());
        let knee = 10_f32.powf(2.0 / 20.0);
        let room = knee * (10_f32.powf(SOFT_DB / 20.0) - 1.0);
        let naive: Vec<f32> = input.iter().map(|&x| soft_clip(knee, room, x)).collect();
        let alias = |samples: &[f32]| {
            let settled = &samples[4_800..43_200];
            10.0 * (synth::tone_power(settled, 13_000.0, RATE)
                / synth::tone_power(settled, 5_000.0, RATE))
            .log10()
        };
        let (ours, theirs) = (alias(&oversampled[0]), alias(&naive));
        assert!(
            ours < theirs - 30.0,
            "13 kHz alias: {ours:.1} dB vs {theirs:.1} dB"
        );
    }

    #[test]
    fn switched_off_it_only_delays() {
        let input = stereo(synth::sine(1000.0, 8.0, 0.5, RATE));
        let mut output = input.clone();
        let mut clipper = Clipper::new(2, ClipperSettings::default());
        clipper.set_enabled(false);
        clipper.process(&mut output);
        assert_eq!(clipper.clipped_share(), 0.0);
        let lag = Clipper::LATENCY;
        assert_eq!(output[0][lag..], input[0][..input[0].len() - lag]);
        // On again, it clips.
        clipper.set_enabled(true);
        clipper.process(&mut output);
        assert!(clipper.clipped_share() > 0.0);
    }

    #[test]
    fn the_stream_lags_by_its_latency_and_reset_forgets() {
        let mut clipper = Clipper::new(1, ClipperSettings::default());
        let mut out = Vec::new();
        for n in 0..64 {
            let mut frame = [if n == 3 { 0.5 } else { 0.0 }];
            clipper.process_frame(&mut frame);
            out.push(frame[0]);
        }
        assert_eq!(
            out.iter().position(|&x| x != 0.0),
            Some(3 + Clipper::LATENCY)
        );
        assert_eq!(clipper.latency(), Clipper::LATENCY);
        // A loud burst, then reset: nothing of it comes out after.
        for _ in 0..32 {
            clipper.process_frame(&mut [4.0]);
        }
        assert!(clipper.largest_cut_db() > 0.0);
        clipper.reset();
        assert_eq!(clipper.largest_cut_db(), 0.0);
        for _ in 0..64 {
            let mut frame = [0.0];
            clipper.process_frame(&mut frame);
            assert_eq!(frame[0], 0.0);
        }
    }

    #[test]
    fn channels_are_clipped_on_their_own_and_bad_input_is_silence() {
        let loud = synth::sine(1000.0, 8.0, 0.5, RATE);
        let quiet = synth::sine(1000.0, -12.0, 0.5, RATE);
        let mut channels = vec![loud.clone(), quiet.clone()];
        clip(&mut channels, ClipperSettings::default());
        assert_eq!(channels[1], quiet, "the quiet channel is untouched");
        assert!(peak(&channels[0]) < peak(&loud));
        let mut broken = vec![vec![f32::NAN, f32::INFINITY, 0.25, 0.0]];
        clip(&mut broken, ClipperSettings::default());
        assert_eq!(broken[0], vec![0.0, 0.0, 0.25, 0.0]);
        // A knee that is not a number is the default one.
        assert_eq!(
            Clipper::new(1, ClipperSettings { knee_db: f32::NAN }).knee,
            Clipper::new(1, ClipperSettings::default()).knee
        );
    }
}
