//! The "glue" compressor of the mastering chain: an RMS detector shared by
//! all channels (so the stereo image never shifts), a soft knee, attack and
//! release on the gain, and makeup gain.
//!
//! Processing never allocates, so the live preview (#26) can run it in the
//! audio callback.

/// The RMS detector's time constant: long enough to follow loudness rather
/// than single peaks, short enough for a 10 ms attack to mean something.
const RMS_WINDOW_MS: f32 = 10.0;
/// Levels are measured in dB above this floor, so silence is not minus
/// infinity.
const SILENCE: f32 = 1e-12;

/// A compressor's settings (the preset table in `docs/ARCHITECTURE.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressorSettings {
    /// Level (dB RMS) above which the gain goes down. A full-scale sine is
    /// about -3 dB RMS.
    pub threshold_db: f32,
    /// How much a level above the threshold is reduced: 4 means 4 dB over
    /// becomes 1 dB over. At least 1 (no compression).
    pub ratio: f32,
    /// How fast the gain goes down when the level rises, in ms.
    pub attack_ms: f32,
    /// How fast it comes back when the level falls, in ms.
    pub release_ms: f32,
    /// Width (dB) of the soft knee around the threshold; 0 is a hard knee.
    pub knee_db: f32,
    /// Gain added after compression, in dB.
    pub makeup_db: f32,
}

impl Default for CompressorSettings {
    /// The Club preset's compressor, without makeup.
    fn default() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 3.0,
            attack_ms: 10.0,
            release_ms: 200.0,
            knee_db: 6.0,
            makeup_db: 0.0,
        }
    }
}

/// The gain change (dB, zero or negative) for a level, from the static curve:
/// none below the knee, `(level - threshold) * (1 / ratio - 1)` above it, and
/// a smooth curve between.
fn curve_db(level_db: f32, threshold_db: f32, ratio: f32, knee_db: f32) -> f32 {
    let over = level_db - threshold_db;
    let slope = 1.0 / ratio - 1.0;
    if knee_db > 0.0 && over.abs() <= knee_db / 2.0 {
        slope * (over + knee_db / 2.0).powi(2) / (2.0 * knee_db)
    } else if over > 0.0 {
        slope * over
    } else {
        0.0
    }
}

/// A one-pole smoothing coefficient for a time constant at a sample rate:
/// each sample keeps this fraction of the distance to the target.
fn pole(time_ms: f32, sample_rate: u32) -> f32 {
    let samples = time_ms.max(0.01) / 1000.0 * sample_rate.max(1) as f32;
    (-1.0 / samples).exp()
}

/// An RMS compressor for planar audio, one gain for all channels.
#[derive(Debug, Clone)]
pub struct Compressor {
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
    makeup_db: f32,
    attack: f32,
    release: f32,
    rms: f32,
    /// The mean square the detector follows.
    mean_square: f32,
    /// The current gain change in dB (zero or negative), smoothed.
    reduction_db: f32,
}

impl Compressor {
    /// A compressor at `sample_rate`. Settings outside their range are
    /// clamped (ratio at least 1, knee at least 0).
    #[must_use]
    pub fn new(sample_rate: u32, settings: CompressorSettings) -> Self {
        Self {
            threshold_db: settings.threshold_db,
            ratio: settings.ratio.max(1.0),
            knee_db: settings.knee_db.max(0.0),
            makeup_db: settings.makeup_db,
            attack: pole(settings.attack_ms, sample_rate),
            release: pole(settings.release_ms, sample_rate),
            rms: pole(RMS_WINDOW_MS, sample_rate),
            mean_square: 0.0,
            reduction_db: 0.0,
        }
    }

    /// The gain reduction right now, in dB (zero or negative), for a meter.
    #[must_use]
    pub fn reduction_db(&self) -> f32 {
        self.reduction_db
    }

    /// Compresses `channels` in place, planar (one slice per channel, all the
    /// same length; a shorter channel ends the processing at its length).
    pub fn process<C: AsMut<[f32]>>(&mut self, channels: &mut [C]) {
        let frames = channels
            .iter_mut()
            .map(|c| c.as_mut().len())
            .min()
            .unwrap_or(0);
        let count = channels.len().max(1) as f32;
        for frame in 0..frames {
            let square = channels
                .iter_mut()
                .map(|c| c.as_mut()[frame].powi(2))
                .sum::<f32>()
                / count;
            self.mean_square = square + (self.mean_square - square) * self.rms;
            let level_db = 10.0 * (self.mean_square + SILENCE).log10();
            let target = curve_db(level_db, self.threshold_db, self.ratio, self.knee_db);
            let pole = if target < self.reduction_db {
                self.attack
            } else {
                self.release
            };
            self.reduction_db = target + (self.reduction_db - target) * pole;
            let gain = 10_f32.powf((self.reduction_db + self.makeup_db) / 20.0);
            for channel in channels.iter_mut() {
                channel.as_mut()[frame] *= gain;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    const RATE: u32 = 48_000;

    fn settings(threshold_db: f32, ratio: f32) -> CompressorSettings {
        CompressorSettings {
            threshold_db,
            ratio,
            attack_ms: 10.0,
            release_ms: 100.0,
            knee_db: 6.0,
            makeup_db: 0.0,
        }
    }

    /// The RMS level of the last half second.
    fn settled_db(samples: &[f32]) -> f32 {
        synth::rms_db(&samples[samples.len() - RATE as usize / 2..])
    }

    #[test]
    fn the_static_curve_follows_the_ratio_with_a_smooth_knee() {
        assert_eq!(curve_db(-30.0, -18.0, 3.0, 6.0), 0.0);
        assert!(
            (curve_db(-6.0, -18.0, 3.0, 6.0) + 8.0).abs() < 1e-5,
            "12 dB over at 3:1"
        );
        assert!((curve_db(-6.0, -18.0, 3.0, 0.0) + 8.0).abs() < 1e-5);
        // At the threshold the knee already takes a little.
        let at = curve_db(-18.0, -18.0, 3.0, 6.0);
        assert!(at < 0.0 && at > -1.0, "{at}");
        // The knee meets the straight line at its edges.
        let edge = curve_db(-15.0, -18.0, 3.0, 6.0);
        assert!((edge - (1.0 / 3.0 - 1.0) * 3.0).abs() < 1e-5);
        assert_eq!(
            curve_db(0.0, -18.0, 1.0, 6.0),
            0.0,
            "ratio 1 compresses nothing"
        );
    }

    #[test]
    fn a_quiet_signal_passes_unchanged() {
        let input = synth::sine(440.0, -30.0, 1.0, RATE);
        let mut output = vec![input.clone()];
        Compressor::new(RATE, settings(-18.0, 4.0)).process(&mut output);
        for (out, inp) in output[0].iter().zip(&input) {
            assert!((out - inp).abs() < 1e-6);
        }
    }

    #[test]
    fn a_steady_loud_tone_is_reduced_by_the_ratio() {
        // A -6 dBFS sine is -9.03 dB RMS: 10.97 dB over -20, reduced by 3/4.
        let input = synth::sine(440.0, -6.0, 2.0, RATE);
        let mut output = vec![input.clone()];
        let mut compressor = Compressor::new(RATE, settings(-20.0, 4.0));
        compressor.process(&mut output);
        let reduction = settled_db(&output[0]) - settled_db(&input);
        let expected = -(-9.03 + 20.0) * 0.75;
        assert!(
            (reduction - expected).abs() < 0.15,
            "{reduction} vs {expected}"
        );
        assert!((compressor.reduction_db() - expected).abs() < 0.2);
    }

    #[test]
    fn makeup_gain_adds_after_compression() {
        let input = synth::sine(440.0, -6.0, 2.0, RATE);
        let (mut plain, mut lifted) = (vec![input.clone()], vec![input]);
        Compressor::new(RATE, settings(-20.0, 4.0)).process(&mut plain);
        let mut with_makeup = settings(-20.0, 4.0);
        with_makeup.makeup_db = 4.0;
        Compressor::new(RATE, with_makeup).process(&mut lifted);
        let added = settled_db(&lifted[0]) - settled_db(&plain[0]);
        assert!((added - 4.0).abs() < 0.01, "{added}");
    }

    #[test]
    fn attack_is_quick_and_release_is_slow() {
        // Quiet, then loud for 1 s, then quiet again.
        let quiet = synth::sine(440.0, -40.0, 1.0, RATE);
        let loud = synth::sine(440.0, -6.0, 1.0, RATE);
        let input: Vec<f32> = quiet.iter().chain(&loud).chain(&quiet).copied().collect();
        let mut compressor = Compressor::new(RATE, settings(-20.0, 4.0));
        let mut reductions = Vec::with_capacity(input.len());
        for sample in &input {
            let mut frame = [[*sample]];
            compressor.process(&mut frame);
            reductions.push(compressor.reduction_db());
        }
        let full = reductions[2 * RATE as usize - 1];
        // 50 ms after the step up, most of the reduction is there.
        let after_attack = reductions[RATE as usize + RATE as usize / 20];
        assert!(after_attack < full * 0.9, "{after_attack} vs {full}");
        // 50 ms after the step down, most of it is still there (100 ms release).
        let after_drop = reductions[2 * RATE as usize + RATE as usize / 20];
        assert!(after_drop < full * 0.4, "{after_drop} vs {full}");
        // And it is gone 1 s later.
        assert!(reductions.last().is_some_and(|r| *r > -0.01));
    }

    #[test]
    fn both_channels_get_the_same_gain() {
        let loud = synth::sine(440.0, -6.0, 1.0, RATE);
        let quiet = synth::sine(440.0, -30.0, 1.0, RATE);
        let mut stereo = vec![loud.clone(), quiet.clone()];
        Compressor::new(RATE, settings(-20.0, 4.0)).process(&mut stereo);
        let left = settled_db(&stereo[0]) - settled_db(&loud);
        let right = settled_db(&stereo[1]) - settled_db(&quiet);
        assert!(left < -3.0, "the loud side drives the compressor");
        assert!(
            (left - right).abs() < 1e-3,
            "{left} vs {right}: the image must not shift"
        );
    }
}
