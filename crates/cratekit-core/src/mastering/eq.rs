//! Biquad filters for the mastering EQ: low and high shelves, peaks, low and
//! high passes, with the formulas of Robert Bristow-Johnson's "Audio EQ
//! Cookbook". Coefficients and filter state are `f64`: the low shelves (70 to
//! 120 Hz at 44.1 or 48 kHz) sit close to the unit circle, where `f32` state
//! would add noise.
//!
//! Processing never allocates (the state is sized in [`Biquad::new`]), so the
//! live preview (#26) can run the same filters in the audio callback.

use std::f64::consts::PI;

/// Lowest corner frequency, in Hz: below it the formulas lose precision.
const MIN_FREQ_HZ: f64 = 10.0;
/// Highest corner frequency, as a fraction of the sample rate (near Nyquist
/// the formulas warp).
const MAX_FREQ_FRACTION: f64 = 0.49;
/// Smallest Q and shelf slope accepted (wider than any musical EQ).
const MIN_Q: f64 = 0.05;
/// The narrowest bandwidth-form peak, in octaves (as narrow as `MIN_Q` is wide).
const MIN_OCTAVES: f64 = 0.01;
/// Steepest shelf slope: 1 is the steepest without overshoot; above it the
/// shelf resonates and, far enough, stops being stable.
const MAX_SLOPE: f64 = 1.0;
/// Filter state below this is flushed to zero, so a silent tail never runs
/// on denormal numbers (very slow on some CPUs).
const DENORMAL: f64 = 1e-30;

/// The five coefficients of one biquad, normalized so `a0` is 1:
/// `y[n] = b0 x[n] + b1 x[n-1] + b2 x[n-2] - a1 y[n-1] - a2 y[n-2]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

/// The cookbook's shared terms for a corner frequency.
struct Corner {
    cos: f64,
    sin: f64,
}

impl Corner {
    fn new(sample_rate: u32, freq_hz: f64) -> Self {
        let rate = f64::from(sample_rate.max(1));
        let highest = (rate * MAX_FREQ_FRACTION).max(MIN_FREQ_HZ);
        let freq = freq_hz.max(MIN_FREQ_HZ).min(highest);
        let w0 = 2.0 * PI * freq / rate;
        Self {
            cos: w0.cos(),
            sin: w0.sin(),
        }
    }
}

impl Coefficients {
    /// A filter that leaves the signal unchanged.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    /// Normalizes raw cookbook coefficients by `a0`.
    fn normalized(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// A peak (bell) of `gain_db` at `freq_hz`, `q` wide (0.7 to 1 is broad
    /// and musical).
    #[must_use]
    pub fn peaking(sample_rate: u32, freq_hz: f64, gain_db: f64, q: f64) -> Self {
        let corner = Corner::new(sample_rate, freq_hz);
        let a = 10_f64.powf(gain_db / 40.0);
        let alpha = corner.sin / (2.0 * q.max(MIN_Q));
        Self::normalized(
            1.0 + alpha * a,
            -2.0 * corner.cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * corner.cos,
            1.0 - alpha / a,
        )
    }

    /// A peak (bell) of `gain_db` at `freq_hz`, `octaves` wide between its
    /// half-gain points: the cookbook's bandwidth form, prewarped, so the
    /// width holds at any sample rate, even close to the Nyquist frequency
    /// (the reference match's bands use it).
    #[must_use]
    pub fn peaking_octaves(sample_rate: u32, freq_hz: f64, gain_db: f64, octaves: f64) -> Self {
        let corner = Corner::new(sample_rate, freq_hz);
        let w0 = corner.sin.atan2(corner.cos);
        let a = 10_f64.powf(gain_db / 40.0);
        let width = octaves.max(MIN_OCTAVES);
        let alpha = corner.sin * (std::f64::consts::LN_2 / 2.0 * width * w0 / corner.sin).sinh();
        Self::normalized(
            1.0 + alpha * a,
            -2.0 * corner.cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * corner.cos,
            1.0 - alpha / a,
        )
    }

    /// A low shelf: `gain_db` below `freq_hz` (the shelf's midpoint), with
    /// `slope` from 0.05 (gentle) to 1.0 (the steepest that does not
    /// overshoot; steeper values are clamped to it).
    #[must_use]
    pub fn low_shelf(sample_rate: u32, freq_hz: f64, gain_db: f64, slope: f64) -> Self {
        let (corner, a, alpha) = Self::shelf_terms(sample_rate, freq_hz, gain_db, slope);
        let (cos, root) = (corner.cos, 2.0 * a.sqrt() * alpha);
        Self::normalized(
            a * ((a + 1.0) - (a - 1.0) * cos + root),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - root),
            (a + 1.0) + (a - 1.0) * cos + root,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - root,
        )
    }

    /// A high shelf: `gain_db` above `freq_hz` (the shelf's midpoint), with
    /// `slope` as for [`Self::low_shelf`].
    #[must_use]
    pub fn high_shelf(sample_rate: u32, freq_hz: f64, gain_db: f64, slope: f64) -> Self {
        let (corner, a, alpha) = Self::shelf_terms(sample_rate, freq_hz, gain_db, slope);
        let (cos, root) = (corner.cos, 2.0 * a.sqrt() * alpha);
        Self::normalized(
            a * ((a + 1.0) + (a - 1.0) * cos + root),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - root),
            (a + 1.0) - (a - 1.0) * cos + root,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - root,
        )
    }

    fn shelf_terms(sample_rate: u32, freq_hz: f64, gain_db: f64, slope: f64) -> (Corner, f64, f64) {
        let corner = Corner::new(sample_rate, freq_hz);
        let a = 10_f64.powf(gain_db / 40.0);
        let slope = slope.clamp(MIN_Q, MAX_SLOPE);
        let alpha = corner.sin / 2.0 * ((a + 1.0 / a) * (1.0 / slope - 1.0) + 2.0).max(0.0).sqrt();
        (corner, a, alpha)
    }

    /// A 12 dB per octave low pass at `freq_hz` (`q` 0.707 is flat, -3 dB at
    /// the corner).
    #[must_use]
    pub fn low_pass(sample_rate: u32, freq_hz: f64, q: f64) -> Self {
        let corner = Corner::new(sample_rate, freq_hz);
        let alpha = corner.sin / (2.0 * q.max(MIN_Q));
        let b = (1.0 - corner.cos) / 2.0;
        Self::normalized(b, 2.0 * b, b, 1.0 + alpha, -2.0 * corner.cos, 1.0 - alpha)
    }

    /// A 12 dB per octave high pass at `freq_hz`.
    #[must_use]
    pub fn high_pass(sample_rate: u32, freq_hz: f64, q: f64) -> Self {
        let corner = Corner::new(sample_rate, freq_hz);
        let alpha = corner.sin / (2.0 * q.max(MIN_Q));
        let b = (1.0 + corner.cos) / 2.0;
        Self::normalized(b, -2.0 * b, b, 1.0 + alpha, -2.0 * corner.cos, 1.0 - alpha)
    }

    /// `self` moved `t` (0 to 1) of the way to `to`, coefficient by
    /// coefficient. Always stable when both are: the stable denominators
    /// form a convex region (the stability triangle), so a blend of two
    /// stable filters is stable. Used to glide between settings without a
    /// jump on a running signal.
    #[must_use]
    pub fn lerp(&self, to: &Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: f64, b: f64| a + (b - a) * t;
        Self {
            b0: mix(self.b0, to.b0),
            b1: mix(self.b1, to.b1),
            b2: mix(self.b2, to.b2),
            a1: mix(self.a1, to.a1),
            a2: mix(self.a2, to.a2),
        }
    }

    /// Whether every coefficient is a number (settings that were not would
    /// silence the filter for good).
    #[must_use]
    pub fn is_finite(&self) -> bool {
        [self.b0, self.b1, self.b2, self.a1, self.a2]
            .iter()
            .all(|k| k.is_finite())
    }

    /// The gain of the filter at `freq_hz`, in dB (for tests and for drawing
    /// the EQ curve).
    #[must_use]
    pub fn response_db(&self, sample_rate: u32, freq_hz: f64) -> f64 {
        let w = 2.0 * PI * freq_hz / f64::from(sample_rate.max(1));
        // H(e^jw) = (b0 + b1 e^-jw + b2 e^-2jw) / (1 + a1 e^-jw + a2 e^-2jw)
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        let power = (num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im);
        10.0 * power.log10()
    }
}

/// One biquad running on every channel of a signal, each with its own state
/// (transposed direct form II).
#[derive(Debug, Clone)]
pub struct Biquad {
    coefficients: Coefficients,
    state: Vec<[f64; 2]>,
}

impl Biquad {
    /// A filter for `channels` channels.
    #[must_use]
    pub fn new(coefficients: Coefficients, channels: usize) -> Self {
        Self {
            coefficients,
            state: vec![[0.0; 2]; channels],
        }
    }

    /// Changes the coefficients and keeps the state, so a parameter change
    /// on a running signal does not restart the filter.
    pub fn set(&mut self, coefficients: Coefficients) {
        self.coefficients = coefficients;
    }

    /// The filter's coefficients (to draw or measure its curve).
    #[must_use]
    pub fn coefficients(&self) -> Coefficients {
        self.coefficients
    }

    /// Clears the state (before an unrelated signal).
    pub fn reset(&mut self) {
        self.state.fill([0.0; 2]);
    }

    /// Filters `channels` in place, planar (one slice per channel). Channels
    /// beyond the count given to [`Self::new`] are left as they are.
    pub fn process<C: AsMut<[f32]>>(&mut self, channels: &mut [C]) {
        let k = self.coefficients;
        for (samples, state) in channels.iter_mut().zip(&mut self.state) {
            let [mut s1, mut s2] = *state;
            for sample in samples.as_mut() {
                // A sample that is not a number would poison the state for
                // good: it is taken as silence.
                let x = if sample.is_finite() {
                    f64::from(*sample)
                } else {
                    0.0
                };
                let y = k.b0 * x + s1;
                s1 = k.b1 * x - k.a1 * y + s2;
                s2 = k.b2 * x - k.a2 * y;
                *sample = y as f32;
            }
            if s1.abs() < DENORMAL {
                s1 = 0.0;
            }
            if s2.abs() < DENORMAL {
                s2 = 0.0;
            }
            *state = [s1, s2];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    const RATE: u32 = 48_000;

    /// The measured gain of `coefficients` for a sine at `freq_hz`, in dB,
    /// after the filter settles.
    fn measured_db(coefficients: Coefficients, freq_hz: f32) -> f32 {
        let input = synth::sine(freq_hz, -12.0, 2.0, RATE);
        let mut output = vec![input.clone()];
        Biquad::new(coefficients, 1).process(&mut output);
        let settled = RATE as usize; // skip the first second
        synth::rms_db(&output[0][settled..]) - synth::rms_db(&input[settled..])
    }

    #[test]
    fn the_identity_leaves_the_signal_alone() {
        let input = synth::sine(440.0, -6.0, 0.1, RATE);
        let mut output = vec![input.clone()];
        Biquad::new(Coefficients::identity(), 1).process(&mut output);
        assert_eq!(output[0], input);
    }

    #[test]
    fn a_peak_reaches_its_gain_at_its_frequency_and_nothing_far_away() {
        let peak = Coefficients::peaking(RATE, 400.0, -2.0, 0.9);
        assert!((peak.response_db(RATE, 400.0) + 2.0).abs() < 1e-9);
        assert!(peak.response_db(RATE, 20.0).abs() < 0.05);
        assert!(peak.response_db(RATE, 15_000.0).abs() < 0.05);
        assert!((measured_db(peak, 400.0) + 2.0).abs() < 0.05);
        assert!(measured_db(peak, 8_000.0).abs() < 0.05);
    }

    #[test]
    fn shelves_lift_their_side_and_leave_the_other() {
        let low = Coefficients::low_shelf(RATE, 90.0, 3.0, 1.0);
        assert!((low.response_db(RATE, 15.0) - 3.0).abs() < 0.1);
        assert!(
            (low.response_db(RATE, 90.0) - 1.5).abs() < 0.01,
            "half the gain at the midpoint"
        );
        assert!(low.response_db(RATE, 5_000.0).abs() < 0.01);
        assert!((measured_db(low, 30.0) - 3.0).abs() < 0.15);

        let high = Coefficients::high_shelf(RATE, 9_000.0, 2.5, 1.0);
        assert!((high.response_db(RATE, 20_000.0) - 2.5).abs() < 0.15);
        assert!(high.response_db(RATE, 200.0).abs() < 0.01);
        assert!(
            (measured_db(high, 18_000.0) - high.response_db(RATE, 18_000.0) as f32).abs() < 0.05
        );
    }

    #[test]
    fn passes_are_3_db_down_at_the_corner_and_fall_12_db_per_octave() {
        let low_pass = Coefficients::low_pass(RATE, 5_500.0, std::f64::consts::FRAC_1_SQRT_2);
        assert!((low_pass.response_db(RATE, 5_500.0) + 3.01).abs() < 0.02);
        assert!(low_pass.response_db(RATE, 100.0).abs() < 0.01);
        let octave_up = low_pass.response_db(RATE, 11_000.0);
        assert!(octave_up < -10.0, "{octave_up}");
        assert!(measured_db(low_pass, 15_000.0) < -15.0);

        let high_pass = Coefficients::high_pass(RATE, 40.0, std::f64::consts::FRAC_1_SQRT_2);
        assert!((high_pass.response_db(RATE, 40.0) + 3.01).abs() < 0.02);
        assert!(high_pass.response_db(RATE, 5_000.0).abs() < 0.01);
    }

    #[test]
    fn filters_are_stable_and_go_silent_after_an_impulse() {
        for coefficients in [
            Coefficients::low_shelf(44_100, 70.0, 4.0, 1.0),
            Coefficients::peaking(44_100, 300.0, -6.0, 0.5),
            Coefficients::high_shelf(44_100, 12_000.0, 3.0, 1.0),
            Coefficients::low_pass(44_100, 5_500.0, 0.707),
        ] {
            let mut impulse = vec![vec![0.0_f32; 44_100]];
            impulse[0][0] = 1.0;
            Biquad::new(coefficients, 1).process(&mut impulse);
            let tail = &impulse[0][22_050..];
            assert!(
                tail.iter().all(|s| s.abs() < 1e-6),
                "{coefficients:?} rings on"
            );
        }
    }

    #[test]
    fn each_channel_has_its_own_state() {
        let tone = synth::sine(100.0, -6.0, 0.5, RATE);
        let mut stereo = vec![tone.clone(), vec![0.0; tone.len()]];
        Biquad::new(Coefficients::low_shelf(RATE, 90.0, 3.0, 1.0), 2).process(&mut stereo);
        assert!(stereo[1].iter().all(|s| *s == 0.0), "silence stays silent");
        assert!(synth::rms_db(&stereo[0]) > synth::rms_db(&tone));
    }

    #[test]
    fn the_shelf_slope_and_the_pass_q_shape_the_curve() {
        let steep = Coefficients::low_shelf(RATE, 90.0, 3.0, 1.0);
        let gentle = Coefficients::low_shelf(RATE, 90.0, 3.0, 0.5);
        assert!(gentle.response_db(RATE, 30.0) < steep.response_db(RATE, 30.0) - 0.1);
        assert!((measured_db(gentle, 30.0) - gentle.response_db(RATE, 30.0) as f32).abs() < 0.05);
        // Slopes past 1 are clamped: no resonance, no runaway.
        let clamped = Coefficients::low_shelf(RATE, 100.0, 20.0, 6.0);
        assert_eq!(clamped, Coefficients::low_shelf(RATE, 100.0, 20.0, 1.0));
        // A Q of 2 peaks about 6 dB at the corner.
        let resonant = Coefficients::low_pass(RATE, 2_000.0, 2.0);
        assert!((resonant.response_db(RATE, 2_000.0) - 20.0 * 2_f64.log10()).abs() < 0.05);
    }

    #[test]
    fn set_keeps_the_state_and_reset_clears_it() {
        let mut filter = Biquad::new(Coefficients::low_pass(RATE, 500.0, 0.707), 1);
        let mut impulse = vec![vec![1.0_f32, 0.0, 0.0, 0.0]];
        filter.process(&mut impulse);
        // New coefficients on a running signal: the old state still rings out.
        filter.set(Coefficients::identity());
        let mut after_set = vec![vec![0.0_f32; 4]];
        filter.process(&mut after_set);
        assert!(after_set[0][0] != 0.0, "the state carried over");
        filter.reset();
        let mut after_reset = vec![vec![0.0_f32; 4]];
        filter.process(&mut after_reset);
        assert!(after_reset[0].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn a_bad_sample_does_not_poison_the_filter() {
        let mut signal = synth::sine(440.0, -6.0, 0.5, RATE);
        signal[100] = f32::NAN;
        signal[200] = f32::INFINITY;
        let mut channels = vec![signal];
        Biquad::new(Coefficients::peaking(RATE, 440.0, 3.0, 1.0), 1).process(&mut channels);
        assert!(channels[0].iter().all(|s| s.is_finite()));
        assert!(
            synth::rms_db(&channels[0][RATE as usize / 4..]) > -9.0,
            "still filtering a tone"
        );
    }

    #[test]
    fn a_zero_sample_rate_does_not_panic() {
        let odd = Coefficients::peaking(0, 1_000.0, 3.0, 1.0);
        assert!(
            [odd.b0, odd.b1, odd.b2, odd.a1, odd.a2]
                .iter()
                .all(|k| k.is_finite())
        );
    }

    #[test]
    fn out_of_range_settings_are_clamped_not_broken() {
        let above_nyquist = Coefficients::peaking(RATE, 40_000.0, 3.0, 0.0);
        assert!(above_nyquist.response_db(RATE, 1_000.0).is_finite());
        let mut impulse = vec![vec![1.0_f32, 0.0, 0.0, 0.0]];
        Biquad::new(above_nyquist, 1).process(&mut impulse);
        assert!(impulse[0].iter().all(|s| s.is_finite()));
    }
}
