//! The look-ahead true-peak limiter at the end of the mastering chain: the
//! output never goes above the ceiling (-1.0 dBTP by default, the SPEC's
//! limit), peaks between samples included.
//!
//! How it works, per sample:
//! 1. **True peak.** Each channel is interpolated 4 times between samples
//!    with a windowed-sinc filter (our own, designed at start-up), and the
//!    peak of a sample is the highest value on either side of it.
//! 2. **Gain needed:** `ceiling / peak`, or 1 when the peak is under it.
//! 3. **Look-ahead.** The gain is the minimum of the needs over the next
//!    look-ahead window, then averaged over that same window: so the gain is
//!    already down when a peak arrives, and changes smoothly (no distortion
//!    from instant gain jumps). The audio is delayed to line up.
//! 4. **Release.** When less reduction is needed, the gain comes back with a
//!    release time constant.
//!
//! Processing never allocates (every buffer is sized in [`Limiter::new`]), so
//! the live preview (#26) can run it in the audio callback.

use std::collections::VecDeque;
use std::f64::consts::PI;

/// Points interpolated between two samples, the original included (4x).
const OVERSAMPLE: usize = 4;
/// Samples on each side used for each interpolated point.
const HALF_TAPS: usize = 12;
/// Kaiser window shape for the interpolation filter.
const KAISER_BETA: f64 = 8.0;
/// Headroom kept under the ceiling, in dB: the gain moves smoothly but still
/// moves between samples, and our interpolator and a meter's differ slightly.
const SAFETY_DB: f32 = 0.15;

/// A limiter's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimiterSettings {
    /// The highest true peak allowed out, in dBTP.
    pub ceiling_dbtp: f32,
    /// How far ahead the limiter looks, in ms (also its delay).
    pub lookahead_ms: f32,
    /// How fast the gain comes back after a peak, in ms.
    pub release_ms: f32,
}

impl Default for LimiterSettings {
    /// The SPEC's -1.0 dBTP, 5 ms ahead, 80 ms release.
    fn default() -> Self {
        Self {
            ceiling_dbtp: -1.0,
            lookahead_ms: 5.0,
            release_ms: 80.0,
        }
    }
}

/// The zeroth-order modified Bessel function, for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..50 {
        term *= (x / (2.0 * k as f64)).powi(2);
        sum += term;
        if term < sum * 1e-12 {
            break;
        }
    }
    sum
}

/// The interpolation filter: for each fractional position `phase / 4`
/// (phases 1 to 3), the weights of the `2 * HALF_TAPS` samples around it,
/// from `-(HALF_TAPS - 1)` to `HALF_TAPS`. Each set sums to 1.
fn interpolation_taps() -> [[f32; 2 * HALF_TAPS]; OVERSAMPLE - 1] {
    let mut taps = [[0.0_f32; 2 * HALF_TAPS]; OVERSAMPLE - 1];
    let half = HALF_TAPS as f64;
    let norm = bessel_i0(KAISER_BETA);
    for (row, phase) in taps.iter_mut().zip(1..OVERSAMPLE) {
        let frac = phase as f64 / OVERSAMPLE as f64;
        let mut weights = [0.0_f64; 2 * HALF_TAPS];
        for (weight, j) in weights.iter_mut().zip(0..2 * HALF_TAPS) {
            // The sample at offset `j - (HALF_TAPS - 1)` from the left one.
            let t = frac - (j as f64 - (half - 1.0));
            let sinc = if t.abs() < 1e-12 {
                1.0
            } else {
                (PI * t).sin() / (PI * t)
            };
            let x = t / (half + 0.5);
            let window = if x.abs() >= 1.0 {
                0.0
            } else {
                bessel_i0(KAISER_BETA * (1.0 - x * x).sqrt()) / norm
            };
            *weight = sinc * window;
        }
        let sum: f64 = weights.iter().sum();
        for (out, weight) in row.iter_mut().zip(weights) {
            *out = (weight / sum) as f32;
        }
    }
    taps
}

/// A fixed-size ring of the last `capacity` values of something.
#[derive(Debug, Clone)]
struct Ring<T> {
    values: Vec<T>,
    next: usize,
}

impl<T: Copy> Ring<T> {
    fn new(capacity: usize, fill: T) -> Self {
        Self {
            values: vec![fill; capacity.max(1)],
            next: 0,
        }
    }

    /// Stores `value` and returns the one it replaces (the oldest).
    fn push(&mut self, value: T) -> T {
        let old = std::mem::replace(&mut self.values[self.next], value);
        self.next = (self.next + 1) % self.values.len();
        old
    }

    /// The value `age` pushes ago (0 is the newest).
    fn get(&self, age: usize) -> T {
        let len = self.values.len();
        self.values[(self.next + len - 1 - age % len) % len]
    }
}

/// A look-ahead true-peak limiter for planar audio.
#[derive(Debug, Clone)]
pub struct Limiter {
    /// The level to stay under, linear, with the safety headroom.
    target: f32,
    lookahead: usize,
    release: f32,
    taps: [[f32; 2 * HALF_TAPS]; OVERSAMPLE - 1],
    /// Per channel: the last `2 * HALF_TAPS` input samples, for interpolation.
    history: Vec<Ring<f32>>,
    /// Per channel: the input waiting to come out (the limiter's delay).
    delay: Vec<Ring<f32>>,
    /// The peak between the previous pair of samples (all channels).
    previous_interval_peak: f32,
    /// The gain needs of the look-ahead window, as a monotonic queue
    /// (position, need) whose front is the window's minimum.
    minimum: VecDeque<(u64, f32)>,
    /// The minimums of the window, to average them.
    held: Ring<f32>,
    held_sum: f64,
    position: u64,
    gain: f32,
}

impl Limiter {
    /// A limiter for `channels` channels at `sample_rate`.
    #[must_use]
    pub fn new(sample_rate: u32, channels: usize, settings: LimiterSettings) -> Self {
        let rate = sample_rate.max(1) as f32;
        let lookahead = ((settings.lookahead_ms.max(0.1) / 1000.0 * rate).round() as usize).max(1);
        let release_samples = settings.release_ms.max(1.0) / 1000.0 * rate;
        let delay_len = lookahead - 1 + HALF_TAPS;
        Self {
            target: 10_f32.powf((settings.ceiling_dbtp - SAFETY_DB) / 20.0),
            lookahead,
            release: (-1.0 / release_samples).exp(),
            taps: interpolation_taps(),
            history: vec![Ring::new(2 * HALF_TAPS, 0.0); channels],
            delay: vec![Ring::new(delay_len + 1, 0.0); channels],
            previous_interval_peak: 0.0,
            minimum: VecDeque::with_capacity(lookahead + 1),
            held: Ring::new(lookahead, 1.0),
            held_sum: lookahead as f64,
            position: 0,
            gain: 1.0,
        }
    }

    /// How many samples the output lags the input.
    #[must_use]
    pub fn latency(&self) -> usize {
        self.lookahead - 1 + HALF_TAPS
    }

    /// The gain applied right now (1 when not limiting), for a meter.
    #[must_use]
    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// The highest absolute value between the two samples at ages
    /// `HALF_TAPS` and `HALF_TAPS - 1` of `history`, both included.
    fn interval_peak(&self, history: &Ring<f32>) -> f32 {
        let left = history.get(HALF_TAPS);
        let right = history.get(HALF_TAPS - 1);
        let mut peak = left.abs().max(right.abs());
        for taps in &self.taps {
            // taps[j] weighs the sample at offset j - (HALF_TAPS - 1) from
            // `left`, whose age is 2 * HALF_TAPS - 1 - j.
            let value: f32 = taps
                .iter()
                .enumerate()
                .map(|(j, weight)| weight * history.get(2 * HALF_TAPS - 1 - j))
                .sum();
            peak = peak.max(value.abs());
        }
        peak
    }

    /// Limits one frame (one sample per channel) in place: the input goes in
    /// and the frame `latency()` samples older comes out, limited.
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        let mut interval_peak = 0.0_f32;
        for (sample, history) in frame.iter().zip(&mut self.history) {
            history.push(*sample);
        }
        for history in &self.history {
            interval_peak = interval_peak.max(self.interval_peak(history));
        }
        // The sample at age HALF_TAPS: its peak is the higher of the
        // intervals on its two sides.
        let peak = interval_peak.max(self.previous_interval_peak);
        self.previous_interval_peak = interval_peak;
        let need = if peak > self.target {
            self.target / peak
        } else {
            1.0
        };

        // Minimum over the look-ahead window (monotonic queue).
        while self.minimum.back().is_some_and(|(_, value)| *value >= need) {
            self.minimum.pop_back();
        }
        self.minimum.push_back((self.position, need));
        let window_start = (self.position + 1).saturating_sub(self.lookahead as u64);
        while self
            .minimum
            .front()
            .is_some_and(|(at, _)| *at < window_start)
        {
            self.minimum.pop_front();
        }
        let held = self.minimum.front().map_or(1.0, |(_, value)| *value);
        self.position += 1;

        // Average of the minimums over the same window.
        let dropped = self.held.push(held);
        self.held_sum += f64::from(held) - f64::from(dropped);
        let average = (self.held_sum / self.lookahead as f64) as f32;
        self.gain = if average < self.gain {
            average
        } else {
            average + (self.gain - average) * self.release
        };

        for (sample, delay) in frame.iter_mut().zip(&mut self.delay) {
            let delayed = delay.get(self.lookahead - 2 + HALF_TAPS);
            delay.push(*sample);
            *sample = delayed * self.gain;
        }
    }

    /// Limits `channels` in place, planar, as a stream: the output lags by
    /// [`Self::latency`] samples (see [`limit`] for a whole signal).
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
                *slot = channel.as_mut()[index];
            }
            self.process_frame(&mut frame[..count]);
            for (slot, channel) in frame.iter().zip(channels.iter_mut()) {
                channel.as_mut()[index] = *slot;
            }
        }
    }
}

/// The most channels [`Limiter::process`] handles (7.1 surround); more are
/// left as they are.
const MAX_FRAME_CHANNELS: usize = 8;

/// Limits a whole signal in place, planar, with the delay taken out: the
/// output lines up with the input and has the same length.
pub fn limit<C: AsMut<[f32]>>(channels: &mut [C], sample_rate: u32, settings: LimiterSettings) {
    let count = channels.len().min(MAX_FRAME_CHANNELS);
    let mut limiter = Limiter::new(sample_rate, count, settings);
    let latency = limiter.latency();
    let frames = channels
        .iter_mut()
        .map(|c| c.as_mut().len())
        .min()
        .unwrap_or(0);
    let mut frame = [0.0_f32; MAX_FRAME_CHANNELS];
    // Feed the signal, then `latency` frames of silence to flush it out; the
    // output of input frame n arrives at step n + latency, where it is
    // written back (always behind the frame being read).
    for step in 0..frames + latency {
        for (slot, channel) in frame.iter_mut().zip(channels.iter_mut()).take(count) {
            *slot = channel.as_mut().get(step).copied().unwrap_or(0.0);
        }
        limiter.process_frame(&mut frame[..count]);
        if step >= latency {
            for (slot, channel) in frame.iter().zip(channels.iter_mut()).take(count) {
                channel.as_mut()[step - latency] = *slot;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::key::{Key, Mode, PitchClass};
    use crate::analysis::loudness::measure_loudness;
    use crate::synth;

    const RATE: u32 = 48_000;

    fn true_peak(channels: &[Vec<f32>], rate: u32) -> f32 {
        let slices: Vec<&[f32]> = channels.iter().map(Vec::as_slice).collect();
        measure_loudness(&slices, rate).map_or(f32::NEG_INFINITY, |l| l.true_peak_dbtp)
    }

    /// A sine at a quarter of the sample rate with a 45 degree phase: its
    /// samples sit at 0.707 of its peak, so its true peak is 3 dB above the
    /// sample peak.
    fn quarter_rate_sine(peak: f32, seconds: f32, rate: u32) -> Vec<f32> {
        (0..(seconds * rate as f32) as usize)
            .map(|n| {
                peak * (std::f32::consts::FRAC_PI_2 * n as f32 + std::f32::consts::FRAC_PI_4).sin()
            })
            .collect()
    }

    #[test]
    fn the_interpolator_passes_dc_and_finds_peaks_between_samples() {
        let taps = interpolation_taps();
        for row in &taps {
            let sum: f32 = row.iter().sum();
            assert!((sum - 1.0).abs() < 1e-5);
        }
        // Sample peaks of 0.707 hide a peak of 1.0 between them.
        let samples = quarter_rate_sine(1.0, 0.1, RATE);
        let limiter = Limiter::new(RATE, 1, LimiterSettings::default());
        let mut history = Ring::new(2 * HALF_TAPS, 0.0);
        let mut found = 0.0_f32;
        for (n, sample) in samples[..1000].iter().enumerate() {
            history.push(*sample);
            // Once the window holds only the tone (its abrupt start rings).
            if n >= 2 * HALF_TAPS {
                found = found.max(limiter.interval_peak(&history));
            }
        }
        assert!((found - 1.0).abs() < 0.005, "{found}");
    }

    #[test]
    fn a_quiet_signal_comes_out_unchanged_and_in_line() {
        let input = vec![
            synth::sine(440.0, -6.0, 0.5, RATE),
            synth::sine(660.0, -9.0, 0.5, RATE),
        ];
        let mut output = input.clone();
        limit(&mut output, RATE, LimiterSettings::default());
        for (out, inp) in output.iter().zip(&input) {
            assert_eq!(out.len(), inp.len());
            for (o, i) in out.iter().zip(inp) {
                assert!((o - i).abs() < 1e-6, "{o} vs {i}");
            }
        }
    }

    #[test]
    fn a_loud_mix_never_goes_over_the_ceiling() {
        // The test loop pushed 12 dB over full scale.
        let a_minor = Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        };
        let mix = synth::normalize(synth::test_loop(126.0, a_minor, 8.0, RATE), 12.0);
        let mut stereo = vec![mix.clone(), mix];
        limit(&mut stereo, RATE, LimiterSettings::default());
        let peak = true_peak(&stereo, RATE);
        assert!(peak <= -1.0, "true peak {peak} dBTP");
        assert!(peak > -1.6, "{peak} dBTP: limited, not crushed");
    }

    #[test]
    fn it_catches_peaks_between_samples() {
        // Sample peaks at -1.5 dBFS, true peak at +1.5 dBTP.
        let tone = quarter_rate_sine(10_f32.powf(1.5 / 20.0), 1.0, RATE);
        assert!(synth::peak_db(&tone) < -1.0);
        let mut channels = vec![tone];
        limit(&mut channels, RATE, LimiterSettings::default());
        let peak = true_peak(&channels, RATE);
        assert!(peak <= -1.0, "true peak {peak} dBTP");
    }

    #[test]
    fn high_notes_near_nyquist_stay_under_the_ceiling_at_44_1_khz() {
        let rate = 44_100;
        let highs = synth::mix(&[
            &synth::sine(15_000.0, 3.0, 2.0, rate),
            &synth::sine(17_500.0, 0.0, 2.0, rate),
            &synth::sine(19_000.0, -3.0, 2.0, rate),
        ]);
        let mut channels = vec![highs];
        limit(&mut channels, rate, LimiterSettings::default());
        let peak = true_peak(&channels, rate);
        assert!(peak <= -1.0, "true peak {peak} dBTP");
    }

    #[test]
    fn the_gain_comes_back_after_a_burst() {
        let loud = synth::sine(440.0, 6.0, 0.5, RATE);
        let quiet = synth::sine(440.0, -20.0, 1.0, RATE);
        let mut limiter = Limiter::new(RATE, 1, LimiterSettings::default());
        let mut gains = Vec::new();
        for sample in loud.iter().chain(&quiet) {
            let mut frame = [*sample];
            limiter.process_frame(&mut frame);
            gains.push(limiter.gain());
        }
        let during = gains[RATE as usize / 4];
        assert!(during < 0.5, "limiting a +6 dB tone: {during}");
        let after = *gains.last().unwrap_or(&0.0);
        assert!(after > 0.999, "released: {after}");
    }

    #[test]
    fn the_stream_lags_by_its_latency() {
        let mut limiter = Limiter::new(RATE, 1, LimiterSettings::default());
        let latency = limiter.latency();
        let mut impulse = vec![vec![0.0_f32; latency + 10]];
        impulse[0][0] = 0.5;
        limiter.process(&mut impulse);
        let at = impulse[0].iter().position(|s| *s != 0.0);
        assert_eq!(at, Some(latency));
    }
}
