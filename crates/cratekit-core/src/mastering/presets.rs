//! The mastering presets (SPEC Module 3: Club, Hip-hop, Pop, Lo-fi, Chill)
//! with the starting values of `docs/ARCHITECTURE.md`, Mastering chain, and
//! the intensity control: how much the EQ and compressor colour the sound,
//! from nothing (0) to the full preset (1). Intensity never changes the
//! loudness: the master lands on the target chosen (decided 2026-11-21, as
//! Ozone 12 and Logic's Mastering Assistant keep loudness separate).
//!
//! A [`PresetChain`] runs a preset's EQ and compressor on a signal: the part
//! of the chain before loudness targeting and the limiter (#25).

use std::f64::consts::FRAC_1_SQRT_2;

use serde::{Deserialize, Serialize};

use super::compressor::{Compressor, CompressorSettings};
use super::eq::{Biquad, Coefficients};

/// Where a low pass starts at intensity 0, as a fraction of the sample rate:
/// the top of the band, so at any rate it does nothing yet.
const OPEN_LOW_PASS_FRACTION: f64 = 0.49;
/// The soft knee every preset's compressor uses, in dB.
const KNEE_DB: f32 = 6.0;

/// A preset's name, as the UI sends and shows it (`"hip-hop"`, `"lo-fi"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub enum PresetName {
    /// Punchy low end, bright top; -8 LUFS.
    Club,
    /// Heavy sub, smooth mids; -9 LUFS.
    HipHop,
    /// Present vocals, airy top; -9 LUFS.
    Pop,
    /// Warm, soft top end (with a low pass); -12 LUFS.
    LoFi,
    /// Gentle and open; -12 LUFS.
    Chill,
}

impl PresetName {
    /// Every preset, in the order the Master tab lists them.
    pub const ALL: [Self; 5] = [Self::Club, Self::HipHop, Self::Pop, Self::LoFi, Self::Chill];

    /// The preset's starting values, at full intensity.
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
            intensity: 1.0,
        }
    }

    /// The name as the UI sends it and the library stores it (`"hip-hop"`).
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Club => "club",
            Self::HipHop => "hip-hop",
            Self::Pop => "pop",
            Self::LoFi => "lo-fi",
            Self::Chill => "chill",
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
    /// The shelf's midpoint (half the gain there), in Hz.
    pub freq_hz: f64,
    /// The gain on the shelf's side, in dB.
    pub gain_db: f64,
}

/// A peak band: `gain_db` around `freq_hz`, `q` wide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    /// The centre, in Hz.
    pub freq_hz: f64,
    /// The gain at the centre, in dB.
    pub gain_db: f64,
    /// The width: 0.7 to 1 is broad and musical.
    pub q: f64,
}

/// A preset's settings: three EQ bands, an optional low pass, the glue
/// compressor, the default loudness target, and the intensity they are at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    /// Which preset.
    pub name: PresetName,
    /// The low shelf.
    pub low_shelf: Shelf,
    /// The mid peak.
    pub mid: Peak,
    /// The high shelf.
    pub high_shelf: Shelf,
    /// A low pass on top of the EQ (Lo-fi's 5.5 kHz), its corner at full
    /// intensity, in Hz.
    pub low_pass_hz: Option<f64>,
    /// The glue compressor (soft knee, no makeup: loudness targeting sets
    /// the level).
    pub compressor: CompressorSettings,
    /// The loudness target the Master tab starts from, in LUFS.
    pub target_lufs: f32,
    /// 0 (no colour) to 1 (the full preset); see [`Preset::at`].
    pub intensity: f32,
}

/// An intensity brought into 0 to 1 (one that is not a number is the full
/// preset).
fn clamp_intensity(intensity: f32) -> f32 {
    if intensity.is_nan() {
        1.0
    } else {
        intensity.clamp(0.0, 1.0)
    }
}

impl Preset {
    /// The preset at `intensity` (0 to 1, the SPEC's 0 to 100 %, clamped):
    /// EQ gains times the intensity and ratio `1 + (ratio - 1) * intensity`;
    /// the low pass slides from the top of the band to its corner (in
    /// [`PresetChain`], which knows the sample rate). At 0 the EQ and
    /// compressor change nothing. The loudness target stays the same.
    #[must_use]
    pub fn at(&self, intensity: f32) -> Self {
        let full = self.name.preset();
        let k = clamp_intensity(intensity);
        let scale = f64::from(k);
        let mut scaled = full;
        scaled.low_shelf.gain_db *= scale;
        scaled.mid.gain_db *= scale;
        scaled.high_shelf.gain_db *= scale;
        scaled.compressor.ratio = 1.0 + (full.compressor.ratio - 1.0) * k;
        scaled.target_lufs = self.target_lufs;
        scaled.intensity = k;
        scaled
    }

    /// The low pass's corner at `sample_rate`, or `None` when there is none
    /// (or at intensity 0): from 0.49 of the rate at 0 to its own corner at
    /// 1, on a log scale (a slide the ear hears as even).
    fn low_pass_corner(&self, sample_rate: u32) -> Option<f64> {
        let open = f64::from(sample_rate.max(1)) * OPEN_LOW_PASS_FRACTION;
        let k = f64::from(self.intensity);
        self.low_pass_hz
            .filter(|_| k > 0.0)
            .map(|corner| open.powf(1.0 - k) * corner.min(open).powf(k))
    }
}

/// The four filters of a preset at a sample rate: low shelf, mid peak, high
/// shelf, low pass (or a filter that changes nothing).
fn filter_coefficients(sample_rate: u32, preset: &Preset) -> [Coefficients; 4] {
    let (low, mid, high) = (preset.low_shelf, preset.mid, preset.high_shelf);
    [
        Coefficients::low_shelf(sample_rate, low.freq_hz, low.gain_db, 1.0),
        Coefficients::peaking(sample_rate, mid.freq_hz, mid.gain_db, mid.q),
        Coefficients::high_shelf(sample_rate, high.freq_hz, high.gain_db, 1.0),
        preset
            .low_pass_corner(sample_rate)
            .map_or_else(Coefficients::identity, |corner| {
                Coefficients::low_pass(sample_rate, corner, FRAC_1_SQRT_2)
            }),
    ]
}

/// A preset's EQ and compressor running on a signal, in the chain's order
/// (EQ, then compressor). Processing never allocates, and [`Self::set`]
/// changes the preset or intensity without restarting the filters, so the
/// live preview (#26) can turn the intensity while it plays.
#[derive(Debug, Clone)]
pub struct PresetChain {
    sample_rate: u32,
    filters: [Biquad; 4],
    compressor: Compressor,
}

impl PresetChain {
    /// The chain for `preset` (at its intensity, see [`Preset::at`]) and
    /// `channels` channels; a signal with more channels is not filtered on
    /// the extra ones (they are still compressed, with the others).
    #[must_use]
    pub fn new(sample_rate: u32, channels: usize, preset: &Preset) -> Self {
        let filters = filter_coefficients(sample_rate, preset)
            .map(|coefficients| Biquad::new(coefficients, channels));
        Self {
            sample_rate,
            filters,
            compressor: Compressor::new(sample_rate, preset.compressor),
        }
    }

    /// Switches to `preset` (another preset or intensity), keeping the
    /// filter and compressor state.
    pub fn set(&mut self, preset: &Preset) {
        for (filter, coefficients) in self
            .filters
            .iter_mut()
            .zip(filter_coefficients(self.sample_rate, preset))
        {
            filter.set(coefficients);
        }
        self.compressor.set(self.sample_rate, preset.compressor);
    }

    /// The EQ's gain at `freq_hz` in dB (the curve the Master tab can draw).
    #[must_use]
    pub fn eq_response_db(&self, freq_hz: f64) -> f64 {
        self.filters
            .iter()
            .map(|filter| filter.coefficients().response_db(self.sample_rate, freq_hz))
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

    /// The gain a preset at `intensity` gives a quiet tone, in dB: quiet
    /// enough that the compressor stays out of it.
    fn tone_gain_db(name: PresetName, intensity: f32, freq_hz: f32) -> f32 {
        let input = synth::sine(freq_hz, -40.0, 2.0, RATE);
        let mut output = vec![input.clone()];
        PresetChain::new(RATE, 1, &name.preset().at(intensity)).process(&mut output);
        let settled = RATE as usize;
        synth::rms_db(&output[0][settled..]) - synth::rms_db(&input[settled..])
    }

    /// One row of the table in docs/ARCHITECTURE.md: low shelf, mid peak
    /// with Q, high shelf, low pass, threshold / ratio / attack / release,
    /// target.
    type Row = (
        (f64, f64),
        (f64, f64, f64),
        (f64, f64),
        Option<f64>,
        (f32, f32, f32, f32),
        f32,
    );

    #[test]
    fn every_preset_holds_its_row_of_the_architecture_table() {
        let table: [(PresetName, Row); 5] = [
            (
                PresetName::Club,
                (
                    (90.0, 3.0),
                    (400.0, -2.0, 0.9),
                    (9_000.0, 2.5),
                    None,
                    (-18.0, 3.0, 10.0, 200.0),
                    -8.0,
                ),
            ),
            (
                PresetName::HipHop,
                (
                    (70.0, 4.0),
                    (300.0, -1.5, 0.9),
                    (10_000.0, 1.5),
                    None,
                    (-20.0, 3.5, 15.0, 250.0),
                    -9.0,
                ),
            ),
            (
                PresetName::Pop,
                (
                    (100.0, 1.5),
                    (2_500.0, 1.5, 0.8),
                    (12_000.0, 3.0),
                    None,
                    (-16.0, 2.5, 10.0, 150.0),
                    -9.0,
                ),
            ),
            (
                PresetName::LoFi,
                (
                    (120.0, 2.0),
                    (1_000.0, -1.0, 0.7),
                    (6_000.0, -6.0),
                    Some(5_500.0),
                    (-22.0, 4.0, 20.0, 300.0),
                    -12.0,
                ),
            ),
            (
                PresetName::Chill,
                (
                    (80.0, 1.0),
                    (3_000.0, -1.0, 0.8),
                    (10_000.0, 1.0),
                    None,
                    (-14.0, 2.0, 20.0, 300.0),
                    -12.0,
                ),
            ),
        ];
        assert_eq!(
            table.map(|(name, _)| name),
            PresetName::ALL,
            "the Master tab's order"
        );
        for (
            name,
            (
                (ls_f, ls_g),
                (mid_f, mid_g, mid_q),
                (hs_f, hs_g),
                low_pass,
                (threshold, ratio, attack, release),
                target,
            ),
        ) in table
        {
            let p = name.preset();
            assert_eq!(p.name, name);
            assert_eq!(
                p.low_shelf,
                Shelf {
                    freq_hz: ls_f,
                    gain_db: ls_g
                },
                "{name:?}"
            );
            assert_eq!(
                p.mid,
                Peak {
                    freq_hz: mid_f,
                    gain_db: mid_g,
                    q: mid_q
                },
                "{name:?}"
            );
            assert_eq!(
                p.high_shelf,
                Shelf {
                    freq_hz: hs_f,
                    gain_db: hs_g
                },
                "{name:?}"
            );
            assert_eq!(p.low_pass_hz, low_pass, "{name:?}");
            let c = p.compressor;
            assert_eq!(
                (c.threshold_db, c.ratio, c.attack_ms, c.release_ms),
                (threshold, ratio, attack, release),
                "{name:?}"
            );
            assert_eq!((c.knee_db, c.makeup_db), (6.0, 0.0), "{name:?}");
            assert_eq!(p.target_lufs, target, "{name:?}");
            assert_eq!(p.intensity, 1.0);
        }
    }

    #[test]
    fn names_serialize_as_the_ui_sends_them() {
        let names: Vec<serde_json::Value> = PresetName::ALL
            .iter()
            .map(|n| serde_json::to_value(n).unwrap())
            .collect();
        assert_eq!(names, ["club", "hip-hop", "pop", "lo-fi", "chill"]);
        for name in PresetName::ALL {
            assert_eq!(
                serde_json::to_value(name).unwrap(),
                name.id(),
                "id() is the serde name"
            );
        }
        let back: PresetName = serde_json::from_value(serde_json::json!("lo-fi")).unwrap();
        assert_eq!(back, PresetName::LoFi);
    }

    #[test]
    fn intensity_scales_the_eq_and_the_ratio_but_never_the_target() {
        let club = PresetName::Club.preset();
        let half = club.at(0.5);
        assert_eq!(half.low_shelf.gain_db, 1.5);
        assert_eq!(half.mid.gain_db, -1.0);
        assert_eq!(half.compressor.ratio, 2.0, "1 + (3 - 1) / 2");
        assert_eq!(half.compressor.threshold_db, club.compressor.threshold_db);
        assert_eq!(
            half.target_lufs, club.target_lufs,
            "the loudness stays the one chosen"
        );
        assert_eq!(half.intensity, 0.5);
        // Scaling again starts from the full preset, not from the half one.
        assert_eq!(half.at(1.0), club);
        let off = club.at(0.0);
        assert_eq!(off.high_shelf.gain_db, 0.0);
        assert_eq!(off.compressor.ratio, 1.0);
        assert_eq!(club.at(7.0), club, "clamped to 1");
        assert_eq!(club.at(f32::NEG_INFINITY), club.at(0.0), "clamped to 0");
        assert_eq!(club.at(f32::NAN), club, "not a number: the full preset");
    }

    #[test]
    fn the_low_pass_slides_from_the_top_of_the_band_at_any_rate() {
        let lofi = PresetName::LoFi.preset();
        assert_eq!(lofi.at(0.0).low_pass_corner(RATE), None);
        assert_eq!(lofi.at(1.0).low_pass_corner(RATE), Some(5_500.0));
        let middle = lofi.at(0.5).low_pass_corner(RATE).unwrap_or(0.0);
        let expected = (0.49 * f64::from(RATE) * 5_500.0).sqrt();
        assert!((middle - expected).abs() < 1.0, "{middle} vs {expected}");
        // A touch of intensity barely touches the audible band, at 48 or 96 kHz.
        for rate in [48_000, 96_000] {
            let chain = PresetChain::new(rate, 1, &lofi.at(0.01));
            let lp = chain.filters[3].coefficients();
            assert!(lp.response_db(rate, 15_000.0) > -0.3, "{rate} Hz");
        }
    }

    #[test]
    fn intensity_zero_leaves_the_eq_and_compressor_out() {
        let a_minor = Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        };
        let mix = synth::test_loop(126.0, a_minor, 2.0, RATE);
        for name in PresetName::ALL {
            let mut stereo = vec![mix.clone(), mix.clone()];
            PresetChain::new(RATE, 2, &name.preset().at(0.0)).process(&mut stereo);
            for channel in &stereo {
                for (out, inp) in channel.iter().zip(&mix) {
                    assert!((out - inp).abs() < 1e-5, "{name:?}: {out} vs {inp}");
                }
            }
        }
    }

    #[test]
    fn club_shapes_lows_mids_and_top_at_full_intensity() {
        let low = tone_gain_db(PresetName::Club, 1.0, 40.0);
        let mid = tone_gain_db(PresetName::Club, 1.0, 400.0);
        let top = tone_gain_db(PresetName::Club, 1.0, 16_000.0);
        assert!((low - 3.0).abs() < 0.25, "40 Hz: {low}");
        assert!((mid + 2.0).abs() < 0.35, "400 Hz: {mid}");
        assert!((top - 2.5).abs() < 0.25, "16 kHz: {top}");
        // The chain's own curve says the same, here and elsewhere.
        let chain = PresetChain::new(RATE, 1, &PresetName::Club.preset());
        assert!((chain.eq_response_db(40.0) as f32 - low).abs() < 0.1);
        assert!((chain.eq_response_db(400.0) as f32 - mid).abs() < 0.1);
        assert!((chain.eq_response_db(16_000.0) as f32 - top).abs() < 0.1);
        // Half the intensity, about half the lift.
        let half = tone_gain_db(PresetName::Club, 0.5, 40.0);
        assert!((half - low / 2.0).abs() < 0.2, "{half} vs {low}");
    }

    #[test]
    fn lofi_darkens_the_top() {
        let top = tone_gain_db(PresetName::LoFi, 1.0, 12_000.0);
        assert!(top < -20.0, "12 kHz: {top}");
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

    #[test]
    fn set_changes_the_sound_without_restarting_the_chain() {
        // Half way: the filters hold state (at 0 they are exact pass-throughs).
        let mut chain = PresetChain::new(RATE, 1, &PresetName::Club.preset().at(0.5));
        let mut tone = vec![synth::sine(40.0, -6.0, 0.5, RATE)];
        chain.process(&mut tone);
        chain.set(&PresetName::Club.preset());
        let fresh = PresetChain::new(RATE, 1, &PresetName::Club.preset());
        for freq in [40.0, 400.0, 16_000.0] {
            assert!(
                (chain.eq_response_db(freq) - fresh.eq_response_db(freq)).abs() < 1e-9,
                "the full preset now at {freq} Hz"
            );
        }
        // The filters kept running: silence after the switch still rings out.
        let mut silence = vec![vec![0.0_f32; 64]];
        chain.process(&mut silence);
        assert!(silence[0].iter().any(|s| *s != 0.0));
    }
}
