//! The mastering presets (SPEC Module 3: Club, Hip-hop, Pop, Lo-fi, Chill)
//! with the starting values of `docs/ARCHITECTURE.md`, Mastering chain, and
//! the intensity control: 0 changes nothing, 1 is the full preset.
//!
//! A [`PresetChain`] runs a preset's EQ and compressor on a signal: the part
//! of the chain before loudness targeting and the limiter (#25).

use std::f64::consts::FRAC_1_SQRT_2;

use serde::{Deserialize, Serialize};

use super::compressor::{Compressor, CompressorSettings};
use super::eq::{Biquad, Coefficients};

/// Where a low pass starts from at intensity 0: above hearing, so it does
/// nothing yet.
const OPEN_LOW_PASS_HZ: f64 = 20_000.0;
/// The soft knee every preset's compressor uses, in dB.
const KNEE_DB: f32 = 6.0;

/// A preset's name, as the UI sends and shows it (`"hip-hop"`, `"lo-fi"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub enum PresetName {
    Club,
    HipHop,
    Pop,
    LoFi,
    Chill,
}

impl PresetName {
    /// Every preset, in the order the Master tab lists them.
    pub const ALL: [Self; 5] = [Self::Club, Self::HipHop, Self::Pop, Self::LoFi, Self::Chill];

    /// The preset's starting values.
    #[must_use]
    pub fn preset(self) -> Preset {
        let compressor = |threshold_db, ratio, attack_ms, release_ms| CompressorSettings {
            threshold_db,
            ratio,
            attack_ms,
            release_ms,
            knee_db: KNEE_DB,
            makeup_db: 0.0,
        };
        let shelf = |freq_hz, gain_db| Shelf { freq_hz, gain_db };
        let peak = |freq_hz, gain_db, q| Peak {
            freq_hz,
            gain_db,
            q,
        };
        let (low_shelf, mid, high_shelf, low_pass_hz, compressor, target_lufs) = match self {
            Self::Club => (
                shelf(90.0, 3.0),
                peak(400.0, -2.0, 0.9),
                shelf(9_000.0, 2.5),
                None,
                compressor(-18.0, 3.0, 10.0, 200.0),
                -8.0,
            ),
            Self::HipHop => (
                shelf(70.0, 4.0),
                peak(300.0, -1.5, 0.9),
                shelf(10_000.0, 1.5),
                None,
                compressor(-20.0, 3.5, 15.0, 250.0),
                -9.0,
            ),
            Self::Pop => (
                shelf(100.0, 1.5),
                peak(2_500.0, 1.5, 0.8),
                shelf(12_000.0, 3.0),
                None,
                compressor(-16.0, 2.5, 10.0, 150.0),
                -9.0,
            ),
            Self::LoFi => (
                shelf(120.0, 2.0),
                peak(1_000.0, -1.0, 0.7),
                shelf(6_000.0, -6.0),
                Some(5_500.0),
                compressor(-22.0, 4.0, 20.0, 300.0),
                -12.0,
            ),
            Self::Chill => (
                shelf(80.0, 1.0),
                peak(3_000.0, -1.0, 0.8),
                shelf(10_000.0, 1.0),
                None,
                compressor(-14.0, 2.0, 20.0, 300.0),
                -12.0,
            ),
        };
        Preset {
            name: self,
            low_shelf,
            mid,
            high_shelf,
            low_pass_hz,
            compressor,
            target_lufs,
        }
    }

    /// The English name; the UI words known presets itself (i18n) and shows
    /// this for any other.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Club => "Club",
            Self::HipHop => "Hip-hop",
            Self::Pop => "Pop",
            Self::LoFi => "Lo-fi",
            Self::Chill => "Chill",
        }
    }

    /// One English line on the sound, as [`Self::label`].
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::Club => "Punchy low end, bright top",
            Self::HipHop => "Heavy sub, smooth mids",
            Self::Pop => "Present vocals, airy top",
            Self::LoFi => "Warm, soft top end",
            Self::Chill => "Gentle and open",
        }
    }
}

/// A shelf band: `gain_db` below (low shelf) or above (high shelf) `freq_hz`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shelf {
    pub freq_hz: f64,
    pub gain_db: f64,
}

/// A peak band: `gain_db` around `freq_hz`, `q` wide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    pub freq_hz: f64,
    pub gain_db: f64,
    pub q: f64,
}

/// A preset's settings: three EQ bands, an optional low pass, the glue
/// compressor and the default loudness target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    pub name: PresetName,
    pub low_shelf: Shelf,
    pub mid: Peak,
    pub high_shelf: Shelf,
    /// A low pass on top of the EQ (Lo-fi's 5.5 kHz), in Hz.
    pub low_pass_hz: Option<f64>,
    pub compressor: CompressorSettings,
    /// The loudness this preset aims at, in LUFS.
    pub target_lufs: f32,
}

/// An intensity brought into 0 to 1 (one that is not a number is the full
/// preset).
fn clamp_intensity(intensity: f32) -> f32 {
    if intensity.is_finite() {
        intensity.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

impl Preset {
    /// The preset at `intensity` (0 to 1, the SPEC's 0 to 100 %): EQ gains
    /// times the intensity, ratio `1 + (ratio - 1) * intensity`, and a low
    /// pass that slides (on a log scale) from 20 kHz down to its corner. At 0
    /// the chain changes nothing.
    #[must_use]
    pub fn at(&self, intensity: f32) -> Self {
        let k = clamp_intensity(intensity);
        let scale = f64::from(k);
        let mut scaled = *self;
        scaled.low_shelf.gain_db *= scale;
        scaled.mid.gain_db *= scale;
        scaled.high_shelf.gain_db *= scale;
        scaled.compressor.ratio = 1.0 + (self.compressor.ratio - 1.0) * k;
        scaled.low_pass_hz = self
            .low_pass_hz
            .filter(|_| k > 0.0)
            .map(|corner| OPEN_LOW_PASS_HZ.powf(1.0 - scale) * corner.powf(scale));
        scaled
    }

    /// The loudness to aim at for a source measured at `source_lufs`: the
    /// source's own at intensity 0, the preset's target at 1, in between
    /// along the way (the SPEC's "makeup" part of intensity, for #25).
    #[must_use]
    pub fn target_at(&self, source_lufs: f32, intensity: f32) -> f32 {
        let k = clamp_intensity(intensity);
        source_lufs + (self.target_lufs - source_lufs) * k
    }
}

/// A preset's EQ and compressor running on a signal, in the chain's order
/// (EQ, then compressor). Processing never allocates.
#[derive(Debug, Clone)]
pub struct PresetChain {
    filters: Vec<Biquad>,
    compressor: Compressor,
}

impl PresetChain {
    /// The chain for `preset` (already at its intensity, see [`Preset::at`]).
    #[must_use]
    pub fn new(sample_rate: u32, channels: usize, preset: &Preset) -> Self {
        let (low, mid, high) = (preset.low_shelf, preset.mid, preset.high_shelf);
        let mut filters = vec![
            Biquad::new(
                Coefficients::low_shelf(sample_rate, low.freq_hz, low.gain_db, 1.0),
                channels,
            ),
            Biquad::new(
                Coefficients::peaking(sample_rate, mid.freq_hz, mid.gain_db, mid.q),
                channels,
            ),
            Biquad::new(
                Coefficients::high_shelf(sample_rate, high.freq_hz, high.gain_db, 1.0),
                channels,
            ),
        ];
        if let Some(corner) = preset.low_pass_hz {
            filters.push(Biquad::new(
                Coefficients::low_pass(sample_rate, corner, FRAC_1_SQRT_2),
                channels,
            ));
        }
        Self {
            filters,
            compressor: Compressor::new(sample_rate, preset.compressor),
        }
    }

    /// The EQ's gain at `freq_hz` in dB (the curve the Master tab can draw).
    #[must_use]
    pub fn eq_response_db(&self, sample_rate: u32, freq_hz: f64) -> f64 {
        self.filters
            .iter()
            .map(|filter| filter.coefficients().response_db(sample_rate, freq_hz))
            .sum()
    }

    /// The compressor's gain reduction right now, in dB, for a meter.
    #[must_use]
    pub fn reduction_db(&self) -> f32 {
        self.compressor.reduction_db()
    }

    /// Runs the EQ then the compressor on `channels` in place, planar.
    pub fn process<C: AsMut<[f32]>>(&mut self, channels: &mut [C]) {
        for filter in &mut self.filters {
            filter.process(channels);
        }
        self.compressor.process(channels);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::key::{Key, Mode, PitchClass};
    use crate::synth;

    const RATE: u32 = 48_000;

    /// The gain a full preset (at `intensity`) gives a quiet tone, in dB:
    /// quiet enough that the compressor stays out of it.
    fn tone_gain_db(name: PresetName, intensity: f32, freq_hz: f32) -> f32 {
        let input = synth::sine(freq_hz, -40.0, 2.0, RATE);
        let mut output = vec![input.clone()];
        PresetChain::new(RATE, 1, &name.preset().at(intensity)).process(&mut output);
        let settled = RATE as usize;
        synth::rms_db(&output[0][settled..]) - synth::rms_db(&input[settled..])
    }

    #[test]
    fn the_presets_hold_the_architecture_table() {
        let club = PresetName::Club.preset();
        assert_eq!(
            club.low_shelf,
            Shelf {
                freq_hz: 90.0,
                gain_db: 3.0
            }
        );
        assert_eq!(
            club.mid,
            Peak {
                freq_hz: 400.0,
                gain_db: -2.0,
                q: 0.9
            }
        );
        assert_eq!(
            club.high_shelf,
            Shelf {
                freq_hz: 9_000.0,
                gain_db: 2.5
            }
        );
        assert_eq!(club.low_pass_hz, None);
        let c = club.compressor;
        assert_eq!(
            (c.threshold_db, c.ratio, c.attack_ms, c.release_ms),
            (-18.0, 3.0, 10.0, 200.0)
        );
        assert_eq!(club.target_lufs, -8.0);

        let lofi = PresetName::LoFi.preset();
        assert_eq!(
            lofi.high_shelf,
            Shelf {
                freq_hz: 6_000.0,
                gain_db: -6.0
            }
        );
        assert_eq!(lofi.low_pass_hz, Some(5_500.0));
        assert_eq!(lofi.compressor.ratio, 4.0);

        let targets: Vec<f32> = PresetName::ALL
            .iter()
            .map(|n| n.preset().target_lufs)
            .collect();
        assert_eq!(targets, [-8.0, -9.0, -9.0, -12.0, -12.0]);
        let ratios: Vec<f32> = PresetName::ALL
            .iter()
            .map(|n| n.preset().compressor.ratio)
            .collect();
        assert_eq!(ratios, [3.0, 3.5, 2.5, 4.0, 2.0]);
        for name in PresetName::ALL {
            assert_eq!(name.preset().name, name);
        }
    }

    #[test]
    fn names_serialize_as_the_ui_sends_them() {
        let names: Vec<serde_json::Value> = PresetName::ALL
            .iter()
            .map(|n| serde_json::to_value(n).unwrap())
            .collect();
        assert_eq!(names, ["club", "hip-hop", "pop", "lo-fi", "chill"]);
        let back: PresetName = serde_json::from_value(serde_json::json!("lo-fi")).unwrap();
        assert_eq!(back, PresetName::LoFi);
    }

    #[test]
    fn intensity_scales_the_eq_the_ratio_and_the_low_pass() {
        let club = PresetName::Club.preset();
        let half = club.at(0.5);
        assert_eq!(half.low_shelf.gain_db, 1.5);
        assert_eq!(half.mid.gain_db, -1.0);
        assert_eq!(half.compressor.ratio, 2.0, "1 + (3 - 1) / 2");
        assert_eq!(half.compressor.threshold_db, club.compressor.threshold_db);
        let off = club.at(0.0);
        assert_eq!(off.high_shelf.gain_db, 0.0);
        assert_eq!(off.compressor.ratio, 1.0);
        assert_eq!(club.at(7.0), club, "clamped to 1");
        assert_eq!(club.at(f32::NAN), club, "not a number: the full preset");

        let lofi = PresetName::LoFi.preset();
        assert_eq!(lofi.at(0.0).low_pass_hz, None);
        assert_eq!(lofi.at(1.0).low_pass_hz, Some(5_500.0));
        let middle = lofi.at(0.5).low_pass_hz.unwrap_or(0.0);
        assert!(
            (middle - (20_000.0_f64 * 5_500.0).sqrt()).abs() < 1.0,
            "{middle}"
        );
    }

    #[test]
    fn the_loudness_target_moves_with_intensity() {
        let club = PresetName::Club.preset();
        assert_eq!(club.target_at(-14.0, 0.0), -14.0);
        assert_eq!(club.target_at(-14.0, 1.0), -8.0);
        assert_eq!(club.target_at(-14.0, 0.5), -11.0);
    }

    #[test]
    fn intensity_zero_changes_nothing() {
        let a_minor = Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        };
        let mix = synth::test_loop(126.0, a_minor, 2.0, RATE);
        for name in PresetName::ALL {
            let mut stereo = vec![mix.clone(), mix.clone()];
            PresetChain::new(RATE, 2, &name.preset().at(0.0)).process(&mut stereo);
            for (out, inp) in stereo[0].iter().zip(&mix) {
                assert!((out - inp).abs() < 1e-5, "{name:?}: {out} vs {inp}");
            }
        }
    }

    #[test]
    fn club_lifts_the_lows_and_the_top_at_full_intensity() {
        let low = tone_gain_db(PresetName::Club, 1.0, 40.0);
        let mid = tone_gain_db(PresetName::Club, 1.0, 1_500.0);
        let top = tone_gain_db(PresetName::Club, 1.0, 16_000.0);
        assert!((low - 3.0).abs() < 0.4, "40 Hz: {low}");
        assert!(mid.abs() < 0.6, "1.5 kHz: {mid}");
        assert!((top - 2.5).abs() < 0.4, "16 kHz: {top}");
        // The chain's own curve says the same.
        let chain = PresetChain::new(RATE, 1, &PresetName::Club.preset());
        assert!((chain.eq_response_db(RATE, 40.0) as f32 - low).abs() < 0.1);
        // Half the intensity, about half the lift.
        let half = tone_gain_db(PresetName::Club, 0.5, 40.0);
        assert!((half - low / 2.0).abs() < 0.2, "{half} vs {low}");
    }

    #[test]
    fn lofi_darkens_the_top() {
        let top = tone_gain_db(PresetName::LoFi, 1.0, 12_000.0);
        assert!(top < -15.0, "12 kHz: {top}");
        let body = tone_gain_db(PresetName::LoFi, 1.0, 500.0);
        assert!(body.abs() < 1.5, "500 Hz: {body}");
    }

    #[test]
    fn the_compressor_works_on_loud_input() {
        // A -3 dBFS sine is -6.03 dB RMS: 11.97 dB over Club's -18, at 3:1.
        let mut loud = vec![synth::sine(1_500.0, -3.0, 2.0, RATE)];
        let mut chain = PresetChain::new(RATE, 1, &PresetName::Club.preset());
        chain.process(&mut loud);
        let expected = -(-6.03_f32 + 18.0) * (1.0 - 1.0 / 3.0);
        assert!(
            (chain.reduction_db() - expected).abs() < 0.5,
            "{} vs {expected}",
            chain.reduction_db()
        );
    }
}
