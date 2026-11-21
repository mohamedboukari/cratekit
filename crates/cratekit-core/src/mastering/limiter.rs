//! The look-ahead true-peak limiter at the end of the mastering chain: the
//! output stays under the ceiling (-1.0 dBTP by default, the SPEC's limit)
//! as a BS.1770 true-peak meter measures it, peaks between samples included.
//!
//! How it works, per sample:
//! 1. **True peak.** Each channel is interpolated between samples twice:
//!    4 times with our own windowed-sinc filter (accurate up to about 0.42 of
//!    the sample rate), and the way a BS.1770 meter does it (48-tap
//!    Hann-windowed sinc, 4x below 96 kHz, 2x up to 192 kHz; the meter that
//!    checks our exports, `ebur128`, works so). A sample's peak is the highest
//!    value either finds on either side of it.
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
/// moves between samples.
const SAFETY_DB: f32 = 0.15;
/// Taps of a BS.1770 meter's interpolation filter.
const METER_TAPS: usize = 48;
/// Longest look-ahead accepted, in ms (a limiter, not a delay line).
const MAX_LOOKAHEAD_MS: f32 = 50.0;

/// A limiter's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimiterSettings {
    /// The highest true peak allowed out, in dBTP.
    pub ceiling_dbtp: f32,
    /// How far ahead the limiter looks, in ms, from 0.1 to 50. The output
    /// lags by this plus 11 samples ([`Limiter::latency`]).
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

/// The interpolation of a BS.1770 true-peak meter, set up to land between
/// the same two samples as ours.
#[derive(Debug, Clone)]
struct MeterTaps {
    /// For each point between the two samples: its weights, newest sample
    /// first.
    rows: Vec<Vec<f32>>,
    /// The age of the sample the first weight applies to.
    first_age: usize,
}

/// A BS.1770 meter's interpolation at `sample_rate`: 48 taps of a
/// Hann-windowed sinc, 4x below 96 kHz, 2x below 192 kHz, none above (no
/// rows: our own interpolator alone).
fn meter_taps(sample_rate: u32) -> MeterTaps {
    let factor = if sample_rate < 96_000 {
        4
    } else if sample_rate < 192_000 {
        2
    } else {
        return MeterTaps {
            rows: Vec::new(),
            first_age: 0,
        };
    };
    let per_point = METER_TAPS / factor;
    let taps = METER_TAPS as f64;
    let rows = (1..factor)
        .map(|point| {
            (0..per_point)
                .map(|k| {
                    let j = (k * factor + point) as f64;
                    let window = 0.5 * (1.0 - (2.0 * PI * j / taps).cos());
                    let x = (j - taps / 2.0) * PI / factor as f64;
                    let sinc = if x.abs() < 1e-9 { 1.0 } else { x.sin() / x };
                    (window * sinc) as f32
                })
                .collect()
        })
        .collect();
    // The meter's point `point` lies (METER_TAPS / 2 - point) / factor samples
    // before the newest sample it reads: so it reads from this age to land
    // between ages HALF_TAPS and HALF_TAPS - 1.
    MeterTaps {
        rows,
        first_age: HALF_TAPS - METER_TAPS / (2 * factor),
    }
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
    meter: MeterTaps,
    /// The age in the delay line of the sample that comes out.
    delay_age: usize,
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
    /// A limiter for `channels` channels at `sample_rate`. Settings outside
    /// their range are brought back into it (a ceiling that is not a number
    /// becomes -1.0 dBTP).
    #[must_use]
    pub fn new(sample_rate: u32, channels: usize, settings: LimiterSettings) -> Self {
        let rate = sample_rate.max(1) as f32;
        let lookahead_ms = if settings.lookahead_ms.is_finite() {
            settings.lookahead_ms.clamp(0.1, MAX_LOOKAHEAD_MS)
        } else {
            LimiterSettings::default().lookahead_ms
        };
        let lookahead = ((lookahead_ms / 1000.0 * rate).round() as usize).max(1);
        let release_ms = if settings.release_ms.is_finite() {
            settings.release_ms.max(1.0)
        } else {
            LimiterSettings::default().release_ms
        };
        let ceiling = if settings.ceiling_dbtp.is_finite() {
            settings.ceiling_dbtp
        } else {
            LimiterSettings::default().ceiling_dbtp
        };
        let delay_age = lookahead + HALF_TAPS - 2;
        Self {
            target: 10_f32.powf((ceiling - SAFETY_DB) / 20.0),
            lookahead,
            release: (-1000.0 / (release_ms * rate)).exp(),
            taps: interpolation_taps(),
            meter: meter_taps(sample_rate),
            delay_age,
            history: vec![Ring::new(2 * HALF_TAPS, 0.0); channels],
            delay: vec![Ring::new(delay_age + 1, 0.0); channels],
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
    /// `HALF_TAPS` and `HALF_TAPS - 1` of `history`, both included, as our
    /// interpolator or a BS.1770 meter's finds it.
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
        for row in &self.meter.rows {
            let value: f32 = row
                .iter()
                .enumerate()
                .map(|(k, weight)| weight * history.get(self.meter.first_age + k))
                .sum();
            peak = peak.max(value.abs());
        }
        peak
    }

    /// Limits one frame (one sample per channel, as many as given to
    /// [`Self::new`]) in place: the input goes in and the frame `latency()`
    /// samples older comes out, limited. A sample that is not a number is
    /// taken as silence. Channels beyond the count are left as they are.
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        for sample in frame.iter_mut() {
            if !sample.is_finite() {
                *sample = 0.0;
            }
        }
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
            let delayed = delay.get(self.delay_age);
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

/// Limits a whole signal in place, planar (channels of the same length; a
/// longer channel's extra samples are left as they are), with the delay
/// taken out: the output lines up with the input and has the same length.
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

    /// Deterministic noise in [-1, 1] (a linear congruential generator).
    fn noise(len: usize, seed: u32) -> Vec<f32> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / (1_u32 << 23) as f32 * 2.0 - 1.0
            })
            .collect()
    }

    fn a_minor() -> Key {
        Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        }
    }

    /// Limits `channels` with `settings` and checks the meter reads at or
    /// under the ceiling.
    fn assert_under_ceiling(
        mut channels: Vec<Vec<f32>>,
        rate: u32,
        settings: LimiterSettings,
        what: &str,
    ) {
        limit(&mut channels, rate, settings);
        let peak = true_peak(&channels, rate);
        assert!(
            peak <= settings.ceiling_dbtp,
            "{what} at {rate} Hz: true peak {peak} dBTP"
        );
    }

    /// A quarter-rate sine whose peak falls `at` of the way between samples.
    fn peak_between(at: f32, rate: u32) -> Vec<f32> {
        use std::f32::consts::FRAC_PI_2;
        (0..rate as usize / 10)
            .map(|n| (FRAC_PI_2 * (n as f32 - at) + FRAC_PI_2).sin())
            .collect()
    }

    /// The value of each in-between point of both interpolators, at most
    /// over a steady stretch of `samples` (once the window holds only it).
    fn point_peaks(limiter: &Limiter, samples: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut history = Ring::new(2 * HALF_TAPS, 0.0);
        let mut ours = vec![0.0_f32; limiter.taps.len()];
        let mut meter = vec![0.0_f32; limiter.meter.rows.len()];
        for (n, sample) in samples.iter().enumerate() {
            history.push(*sample);
            if n < 2 * HALF_TAPS {
                continue;
            }
            for (found, taps) in ours.iter_mut().zip(&limiter.taps) {
                let value: f32 = taps
                    .iter()
                    .enumerate()
                    .map(|(j, w)| w * history.get(2 * HALF_TAPS - 1 - j))
                    .sum();
                *found = found.max(value.abs());
            }
            for (found, row) in meter.iter_mut().zip(&limiter.meter.rows) {
                let value: f32 = row
                    .iter()
                    .enumerate()
                    .map(|(k, w)| w * history.get(limiter.meter.first_age + k))
                    .sum();
                *found = found.max(value.abs());
            }
        }
        (ours, meter)
    }

    #[test]
    fn both_interpolators_find_peaks_at_each_point_between_samples() {
        let limiter = Limiter::new(RATE, 1, LimiterSettings::default());
        assert_eq!(limiter.meter.rows.len(), 3, "4x below 96 kHz");
        for (point, at) in [(0, 0.25_f32), (1, 0.5), (2, 0.75)] {
            let (ours, meter) = point_peaks(&limiter, &peak_between(at, RATE));
            assert!(
                (ours[point] - 1.0).abs() < 0.005,
                "ours at {at}: {}",
                ours[point]
            );
            assert!(
                (meter[point] - 1.0).abs() < 0.02,
                "meter at {at}: {}",
                meter[point]
            );
        }
        let high = Limiter::new(96_000, 1, LimiterSettings::default());
        assert_eq!(high.meter.rows.len(), 1, "2x from 96 kHz");
        let (_, meter) = point_peaks(&high, &peak_between(0.5, 96_000));
        assert!(
            (meter[0] - 1.0).abs() < 0.02,
            "meter at 96 kHz: {}",
            meter[0]
        );
        assert!(
            Limiter::new(192_000, 1, LimiterSettings::default())
                .meter
                .rows
                .is_empty()
        );
    }

    #[test]
    fn transients_and_broadband_content_stay_under_the_ceiling() {
        let settings = LimiterSettings::default();
        for rate in [44_100, 48_000, 96_000] {
            let mix = synth::normalize(
                synth::mix(&[
                    &synth::test_loop(126.0, a_minor(), 4.0, rate),
                    &synth::hats(126.0, 4, 0.5, -3.0, 4.0, rate),
                ]),
                6.0,
            );
            assert_under_ceiling(vec![mix.clone(), mix], rate, settings, "loop with hats");
            let hats = synth::normalize(synth::hats(126.0, 2, 0.0, -3.0, 2.0, rate), 12.0);
            assert_under_ceiling(vec![hats], rate, settings, "hats");
            let binary = noise(rate as usize / 2, 7)
                .iter()
                .map(|v| v.signum())
                .collect();
            assert_under_ceiling(vec![binary], rate, settings, "binary noise");
        }
        // A high tone starting at full blast after silence.
        for (rate, freq) in [
            (44_100_u32, 18_000.0),
            (48_000, 20_000.0),
            (48_000, 19_000.0),
        ] {
            let mut onset = vec![0.0_f32; rate as usize / 2];
            onset.extend(
                (0..rate as usize)
                    .map(|n| (2.0 * (2.0 * PI * freq * n as f64 / f64::from(rate)).cos()) as f32),
            );
            assert_under_ceiling(vec![onset], rate, settings, "high onset");
        }
        // Sparse full-level spikes over quiet noise, also with a short look-ahead.
        let mut spiky: Vec<f32> = noise(2 * 44_100, 3).iter().map(|v| v * 0.05).collect();
        let last = spiky.len() - 1;
        for p in noise(40, 11) {
            let at = ((p + 1.0) / 2.0 * last as f32) as usize;
            spiky[at.min(last)] = p.signum();
        }
        assert_under_ceiling(vec![spiky.clone()], 44_100, settings, "spikes");
        let short = LimiterSettings {
            lookahead_ms: 2.0,
            ..settings
        };
        assert_under_ceiling(vec![spiky], 48_000, short, "spikes, 2 ms look-ahead");
    }

    #[test]
    fn every_sample_gets_at_most_the_gain_its_peak_needs() {
        // Our own detector reads the output at the target or under it: so the
        // gain lines up with the audio, covers both sides of each sample, and
        // is down before the peak (not merely under the meter's ceiling).
        let mut impulse = vec![0.0_f32; RATE as usize / 2];
        impulse[RATE as usize / 4] = 10.0;
        let mut burst = vec![0.0_f32; RATE as usize / 4];
        burst.extend(synth::sine(1_000.0, 12.0, 0.01, RATE));
        burst.extend(vec![0.0_f32; RATE as usize / 4]);
        for signal in [
            impulse,
            burst,
            synth::normalize(synth::hats(126.0, 4, 0.0, -3.0, 2.0, RATE), 12.0),
        ] {
            let mut channels = vec![signal];
            limit(&mut channels, RATE, LimiterSettings::default());
            let reader = Limiter::new(RATE, 1, LimiterSettings::default());
            let mut history = Ring::new(2 * HALF_TAPS, 0.0);
            let mut highest = 0.0_f32;
            for sample in &channels[0] {
                history.push(*sample);
                highest = highest.max(reader.interval_peak(&history));
            }
            let slack = 10_f32.powf(0.03 / 20.0);
            assert!(
                highest <= reader.target * slack,
                "{highest} over {}",
                reader.target
            );
        }
    }

    #[test]
    fn the_gain_moves_smoothly_down_and_releases_slowly() {
        let mut limiter = Limiter::new(RATE, 1, LimiterSettings::default());
        let lookahead = limiter.lookahead;
        let mut signal = vec![0.0_f32; RATE as usize / 10];
        signal.extend(synth::sine(1_000.0, 12.0, 0.05, RATE));
        let end = signal.len();
        signal.extend(vec![0.0_f32; RATE as usize]);
        let mut gains = Vec::with_capacity(signal.len());
        for sample in &signal {
            let mut frame = [*sample];
            limiter.process_frame(&mut frame);
            gains.push(limiter.gain());
        }
        // Down in steps of at most 1 / look-ahead: no click from a jump.
        let biggest_drop = gains
            .windows(2)
            .map(|w| w[0] - w[1])
            .fold(0.0_f32, f32::max);
        assert!(
            biggest_drop <= 1.0 / lookahead as f32 + 1e-6,
            "{biggest_drop}"
        );
        // Released over the release time, not at once: 20 ms after the burst
        // has passed the look-ahead, the gain is still well under 1.
        let lowest = gains.iter().copied().fold(1.0_f32, f32::min);
        let soon = gains[end + lookahead + RATE as usize / 50];
        assert!(
            soon < 1.0 - 0.5 * (1.0 - lowest),
            "{soon} vs lowest {lowest}"
        );
        assert!(gains.last().is_some_and(|g| *g > 0.999));
    }

    #[test]
    fn odd_settings_and_input_are_handled() {
        // A look-ahead that rounds to one sample.
        let mut tiny = Limiter::new(
            8_000,
            1,
            LimiterSettings {
                lookahead_ms: 0.1,
                ..LimiterSettings::default()
            },
        );
        let mut frame = [0.5_f32];
        tiny.process_frame(&mut frame);
        // Not-a-number settings fall back to the defaults.
        let odd = LimiterSettings {
            ceiling_dbtp: f32::NAN,
            lookahead_ms: f32::INFINITY,
            release_ms: f32::NAN,
        };
        let mut loud = vec![synth::sine(440.0, 12.0, 0.5, RATE)];
        limit(&mut loud, RATE, odd);
        assert!(true_peak(&loud, RATE) <= -1.0);
        // A sample that is not a number comes out as silence and spoils nothing.
        let mut signal = synth::sine(440.0, 12.0, 0.5, RATE);
        signal[1000] = f32::NAN;
        signal[2000] = f32::INFINITY;
        let mut channels = vec![signal];
        limit(&mut channels, RATE, LimiterSettings::default());
        assert!(channels[0].iter().all(|s| s.is_finite()));
        assert!(true_peak(&channels, RATE) <= -1.0);
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
        let mix = synth::normalize(synth::test_loop(126.0, a_minor(), 8.0, RATE), 12.0);
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
