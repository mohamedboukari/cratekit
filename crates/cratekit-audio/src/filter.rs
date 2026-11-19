//! The DJ filter: one knob, low-pass to the left, high-pass to the right,
//! the untouched signal at the centre.
//!
//! It is a state-variable filter in the topology-preserving form described
//! by Vadim Zavalishin ("The Art of VA Filter Design") and Andrew Simper
//! (Cytomic, "Linear trapezoidal integrated SVF"): its cutoff can move every
//! sample without clicks or instability, and one filter gives both the
//! low-pass and the high-pass output. 12 dB per octave, Butterworth Q (no
//! resonance peak, so a loud track does not clip).
//!
//! Runs inside the audio callback: scalar state only, no allocation, no
//! locks, no panics.

/// Knob values closer to the centre than this leave the sound untouched
/// (the UI's knob text says "Off" below the same 2%).
pub const DEAD_ZONE: f32 = 0.02;

/// Low-pass cutoff at the start of the travel and at full left.
const LOW_PASS_OPEN_HZ: f64 = 20_000.0;
const LOW_PASS_CLOSED_HZ: f64 = 80.0;
/// High-pass cutoff at the start of the travel and at full right.
const HIGH_PASS_OPEN_HZ: f64 = 20.0;
const HIGH_PASS_CLOSED_HZ: f64 = 8_000.0;
/// Cutoffs stay below this share of the sample rate.
const MAX_CUTOFF_OF_RATE: f64 = 0.45;
/// Butterworth: flat pass band, no resonance peak.
const Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// Time constant of the knob smoothing: the value is about 63% of the way
/// to a new setting after this long and 95% after three times it, so
/// turning the knob fast never clicks.
const SMOOTHING_SECONDS: f64 = 0.02;
/// The filtered signal fades in over this share of the travel past the
/// dead zone, so switching between low-pass and high-pass at the centre
/// happens while the filter is not heard.
const BLEND_TRAVEL: f64 = 0.1;
/// Filter state below this is flushed to zero: tiny "denormal" floats are
/// very slow on some CPUs.
const DENORMAL: f64 = 1e-20;

/// Which output of the state-variable filter is heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    LowPass,
    HighPass,
}

/// One channel's filter memory: the states of the two integrators.
#[derive(Debug, Clone, Copy, Default)]
struct Memory {
    ic1: f64,
    ic2: f64,
}

/// The filter for one stereo deck.
#[derive(Debug, Clone)]
pub struct DjFilter {
    sample_rate: f64,
    /// Where the knob was set (-1 to 1).
    target: f64,
    /// Where the knob is now on its way to `target`.
    value: f64,
    /// How far `value` moves towards `target` each sample.
    smoothing: f64,
    side: Side,
    /// How much of the filtered signal is heard (0 to 1).
    wet: f64,
    /// Coefficients for the current cutoff.
    k: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    left: Memory,
    right: Memory,
}

impl DjFilter {
    /// A filter at the centre (off) for audio at `sample_rate`.
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate.max(1));
        let mut filter = Self {
            sample_rate: rate,
            target: 0.0,
            value: 0.0,
            smoothing: 1.0 - (-1.0 / (SMOOTHING_SECONDS * rate)).exp(),
            side: Side::LowPass,
            wet: 0.0,
            k: 1.0 / Q,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            left: Memory::default(),
            right: Memory::default(),
        };
        filter.update();
        filter
    }

    /// Sets the knob: -1 full low-pass, 0 off, 1 full high-pass. Values
    /// outside are clamped; a value that is not a number counts as 0.
    pub fn set(&mut self, value: f32) {
        self.target = if value.is_finite() {
            f64::from(value.clamp(-1.0, 1.0))
        } else {
            0.0
        };
    }

    /// Sets the knob at once, with no smoothing: only for a filter that is
    /// not heard yet (an engine that is starting).
    pub fn jump(&mut self, value: f32) {
        self.set(value);
        self.value = self.target;
        self.update();
    }

    /// Filters one stereo frame.
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        if self.value != self.target {
            let step = (self.target - self.value) * self.smoothing;
            self.value = if step.abs() < 1e-7 {
                self.target
            } else {
                self.value + step
            };
            self.update();
        }
        // The filter always runs, so its memory is current when it fades in.
        let filtered_left = self.tick(Channel::Left, f64::from(left));
        let filtered_right = self.tick(Channel::Right, f64::from(right));
        if self.wet <= 0.0 {
            return (left, right);
        }
        let dry = 1.0 - self.wet;
        (
            (dry * f64::from(left) + self.wet * filtered_left) as f32,
            (dry * f64::from(right) + self.wet * filtered_right) as f32,
        )
    }

    /// Recomputes side, blend and coefficients from `value`.
    fn update(&mut self) {
        let magnitude = self.value.abs();
        let dead = f64::from(DEAD_ZONE);
        // 0 at the edge of the dead zone, 1 at the end of the travel.
        let travel = ((magnitude - dead) / (1.0 - dead)).clamp(0.0, 1.0);
        self.wet = if magnitude < dead {
            0.0
        } else {
            (travel / BLEND_TRAVEL).min(1.0)
        };
        self.side = if self.value < 0.0 {
            Side::LowPass
        } else {
            Side::HighPass
        };
        // Exponential sweep: equal knob steps sound like equal pitch steps.
        let cutoff = match self.side {
            Side::LowPass => {
                LOW_PASS_OPEN_HZ * (LOW_PASS_CLOSED_HZ / LOW_PASS_OPEN_HZ).powf(travel)
            }
            Side::HighPass => {
                HIGH_PASS_OPEN_HZ * (HIGH_PASS_CLOSED_HZ / HIGH_PASS_OPEN_HZ).powf(travel)
            }
        }
        .min(MAX_CUTOFF_OF_RATE * self.sample_rate);
        let g = (std::f64::consts::PI * cutoff / self.sample_rate).tan();
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    /// One sample through one channel's filter; returns the heard output.
    fn tick(&mut self, channel: Channel, input: f64) -> f64 {
        let (a1, a2, a3, k, side) = (self.a1, self.a2, self.a3, self.k, self.side);
        let memory = match channel {
            Channel::Left => &mut self.left,
            Channel::Right => &mut self.right,
        };
        let v3 = input - memory.ic2;
        let v1 = a1 * memory.ic1 + a2 * v3;
        let v2 = memory.ic2 + a2 * memory.ic1 + a3 * v3;
        memory.ic1 = flush(2.0 * v1 - memory.ic1);
        memory.ic2 = flush(2.0 * v2 - memory.ic2);
        match side {
            Side::LowPass => v2,
            Side::HighPass => input - k * v1 - v2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Channel {
    Left,
    Right,
}

/// Zero for a value too small to matter (or not a number), else the value.
fn flush(value: f64) -> f64 {
    if value.abs() < DENORMAL || !value.is_finite() {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(frequency: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|n| {
                (0.5 * (std::f64::consts::TAU * frequency * n as f64 / f64::from(rate)).sin())
                    as f32
            })
            .collect()
    }

    /// Runs `input` (the same on both channels) through a filter set to
    /// `knob` (already settled) and returns the left output.
    fn run(knob: f32, input: &[f32], rate: u32) -> Vec<f32> {
        let mut filter = DjFilter::new(rate);
        filter.set(knob);
        // Let the smoothing reach the knob value first.
        for _ in 0..rate {
            filter.process(0.0, 0.0);
        }
        input.iter().map(|&x| filter.process(x, x).0).collect()
    }

    /// Level of the second half (after the filter has settled), in dB
    /// relative to the input's level.
    fn gain_db(output: &[f32], input: &[f32]) -> f64 {
        let rms = |s: &[f32]| {
            let tail = &s[s.len() / 2..];
            (tail
                .iter()
                .map(|&x| f64::from(x) * f64::from(x))
                .sum::<f64>()
                / tail.len() as f64)
                .sqrt()
        };
        20.0 * (rms(output) / rms(input)).log10()
    }

    #[test]
    fn the_centre_leaves_the_sound_untouched() {
        let input = sine(1_000.0, RATE, 4_800);
        for knob in [0.0, 0.019, -0.019] {
            assert_eq!(run(knob, &input, RATE), input, "knob {knob}");
        }
    }

    #[test]
    fn full_low_pass_cuts_highs_and_keeps_lows() {
        let highs = sine(5_000.0, RATE, 9_600);
        let lows = sine(40.0, RATE, 48_000);
        assert!(gain_db(&run(-1.0, &highs, RATE), &highs) < -24.0);
        assert!(gain_db(&run(-1.0, &lows, RATE), &lows) > -3.5);
    }

    #[test]
    fn full_high_pass_cuts_lows_and_keeps_highs() {
        let lows = sine(60.0, RATE, 48_000);
        let highs = sine(15_000.0, RATE, 9_600);
        assert!(gain_db(&run(1.0, &lows, RATE), &lows) < -24.0);
        assert!(gain_db(&run(1.0, &highs, RATE), &highs) > -3.5);
    }

    #[test]
    fn turning_further_cuts_more() {
        let tone = sine(2_000.0, RATE, 9_600);
        let cut = |knob: f32| gain_db(&run(knob, &tone, RATE), &tone);
        assert!(cut(-0.3) > cut(-0.6));
        assert!(cut(-0.6) > cut(-0.9));
        let bass = sine(200.0, RATE, 24_000);
        let cut = |knob: f32| gain_db(&run(knob, &bass, RATE), &bass);
        assert!(cut(0.3) > cut(0.6));
        assert!(cut(0.6) > cut(0.9));
    }

    #[test]
    fn a_jump_across_the_whole_knob_does_not_click() {
        let input = sine(1_000.0, RATE, 48_000);
        // The largest step of the plain sine.
        let natural = input
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        let mut filter = DjFilter::new(RATE);
        let mut previous = 0.0_f32;
        let mut largest = 0.0_f32;
        for (n, &x) in input.iter().enumerate() {
            match n {
                4_800 => filter.set(-1.0),
                14_400 => filter.set(1.0),
                28_800 => filter.set(0.0),
                _ => {}
            }
            let (left, _) = filter.process(x, x);
            largest = largest.max((left - previous).abs());
            previous = left;
        }
        assert!(
            largest < 1.5 * natural,
            "largest step {largest}, plain sine {natural}"
        );
    }

    #[test]
    fn stays_stable_at_every_common_rate() {
        for rate in [44_100, 48_000, 88_200, 96_000, 192_000] {
            let input = sine(440.0, rate, rate as usize / 2);
            for knob in [-1.0, -0.5, 0.5, 1.0] {
                let output = run(knob, &input, rate);
                assert!(
                    output.iter().all(|x| x.is_finite() && x.abs() < 1.0),
                    "rate {rate}, knob {knob}"
                );
            }
        }
    }

    #[test]
    fn out_of_range_values_are_clamped_and_nan_is_off() {
        let input = sine(1_000.0, RATE, 4_800);
        assert_eq!(run(f32::NAN, &input, RATE), input);
        assert_eq!(run(-5.0, &input, RATE), run(-1.0, &input, RATE));
        assert_eq!(run(5.0, &input, RATE), run(1.0, &input, RATE));
    }

    #[test]
    fn jump_sets_the_knob_with_no_sweep() {
        let tone = sine(5_000.0, RATE, 9_600);
        let mut filter = DjFilter::new(RATE);
        filter.jump(-1.0);
        let output: Vec<f32> = tone.iter().map(|&x| filter.process(x, x).0).collect();
        // Already fully closed from the first samples, not sweeping down.
        let early = &output[..480];
        let peak = early.iter().fold(0.0_f32, |p, x| p.max(x.abs()));
        assert!(peak < 0.05, "peak in the first 10 ms: {peak}");
    }

    #[test]
    fn silence_settles_to_exact_zero() {
        let mut filter = DjFilter::new(RATE);
        filter.set(-0.8);
        for x in sine(100.0, RATE, 4_800) {
            filter.process(x, x);
        }
        let mut last = (1.0, 1.0);
        for _ in 0..RATE {
            last = filter.process(0.0, 0.0);
        }
        assert_eq!(last, (0.0, 0.0));
    }
}
