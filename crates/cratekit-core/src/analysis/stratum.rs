//! Tempo and key detection through the `stratum-dsp` crate, behind our own
//! [`TempoDetector`] and [`KeyDetector`] traits, so it can be swapped or
//! compared with [`OnsetTempo`](crate::analysis::tempo::OnsetTempo) and
//! [`ProfileKey`](crate::analysis::key_detect::ProfileKey).
//!
//! `stratum-dsp` analyzes tempo, key and beat grid in one pass; each wrapper
//! runs that pass and keeps its part. It reports no chroma, so
//! [`StratumKey`] adds ours for the UI.
//!
//! Its key can differ between runs on the same audio: when the top keys are
//! nearly tied, `stratum-dsp` picks one in an order that changes from run to
//! run. Our own detectors always give the same answer.
//!
//! Behind the `stratum` cargo feature, off by default: on our synthetic test
//! loops our own detectors are more accurate and about 5 times faster (see
//! docs/analysis.md), so the app does not ship this dependency for now.

use stratum_dsp::{AnalysisConfig, AnalysisResult, analyze_audio};

use crate::analysis::key::{Key, Mode, PitchClass};
use crate::analysis::key_detect::{self, KeyDetector, KeyEstimate};
use crate::analysis::tempo::{self, TempoDetector, TempoEstimate};

/// Tempo from `stratum-dsp`, folded into our 70 to 180 BPM range.
#[derive(Debug, Clone, Copy, Default)]
pub struct StratumTempo;

/// Key from `stratum-dsp`, with our chroma added.
#[derive(Debug, Clone, Copy, Default)]
pub struct StratumKey;

/// Runs `stratum-dsp`, or `None` for input it cannot use. We apply the same
/// minimum length as our own detector first (so both kinds agree on what is
/// "too short"), and treat any of its errors as "no answer".
fn analyze(samples: &[f32], sample_rate: u32) -> Option<AnalysisResult> {
    if sample_rate == 0
        || (samples.len() as f64) < tempo::MIN_SECONDS * f64::from(sample_rate)
        || samples.iter().any(|s| !s.is_finite())
    {
        return None;
    }
    analyze_audio(samples, sample_rate, AnalysisConfig::default()).ok()
}

impl TempoDetector for StratumTempo {
    fn detect_tempo(&self, samples: &[f32], sample_rate: u32) -> Option<TempoEstimate> {
        let result = analyze(samples, sample_rate)?;
        // With no usable beat, stratum-dsp answers its minimum tempo (40 BPM)
        // at zero confidence instead of an error: that is "no answer".
        let fallback =
            result.bpm <= AnalysisConfig::default().min_bpm && result.bpm_confidence <= 0.0;
        if !(result.bpm.is_finite() && result.bpm > 0.0) || fallback {
            return None;
        }
        Some(TempoEstimate {
            bpm: tempo::fold_into_range(f64::from(result.bpm)) as f32,
            confidence: result.bpm_confidence.clamp(0.0, 1.0),
        })
    }
}

impl KeyDetector for StratumKey {
    fn detect_key(&self, samples: &[f32], sample_rate: u32) -> Option<KeyEstimate> {
        let result = analyze(samples, sample_rate)?;
        // When it skips or fails key detection, stratum-dsp answers C major
        // with zero clarity instead of an error: that is "no answer".
        if result.key_clarity <= 0.0 {
            return None;
        }
        Some(KeyEstimate {
            key: to_key(result.key)?,
            // Its own `key_confidence` came back 0 on every input we tried, so
            // we report its key clarity (how clearly tonal the audio is, 0 to 1).
            confidence: result.key_clarity.clamp(0.0, 1.0),
            chroma: key_detect::chroma(samples, sample_rate)?,
        })
    }
}

/// `stratum-dsp` numbers keys 0 (C) to 11 (B), like our pitch classes.
fn to_key(key: stratum_dsp::Key) -> Option<Key> {
    let (index, mode) = match key {
        stratum_dsp::Key::Major(n) => (n, Mode::Major),
        stratum_dsp::Key::Minor(n) => (n, Mode::Minor),
    };
    let index = u8::try_from(index).ok().filter(|&i| i < 12)?;
    Some(Key {
        tonic: PitchClass::from_index(index),
        mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    const RATE: u32 = 44_100;

    #[test]
    fn maps_every_stratum_key_onto_ours() {
        for n in 0..12_u32 {
            let tonic = PitchClass::from_index(u8::try_from(n).unwrap());
            assert_eq!(
                to_key(stratum_dsp::Key::Major(n)),
                Some(Key {
                    tonic,
                    mode: Mode::Major
                })
            );
            assert_eq!(
                to_key(stratum_dsp::Key::Minor(n)),
                Some(Key {
                    tonic,
                    mode: Mode::Minor
                })
            );
        }
        assert_eq!(to_key(stratum_dsp::Key::Major(12)), None);
    }

    #[test]
    fn detects_tempo_and_key_of_a_clear_loop() {
        let c_major = Key {
            tonic: PitchClass::C,
            mode: Mode::Major,
        };
        let audio = synth::test_loop(124.0, c_major, 10.0, RATE);
        let tempo = StratumTempo.detect_tempo(&audio, RATE).unwrap();
        // This checks the wrapping (right neighbourhood, not half or double,
        // folded into range), not stratum-dsp's precision: it is less precise
        // than ours on short clips (126 here; see docs/analysis.md).
        assert!((tempo.bpm - 124.0).abs() <= 3.0, "got {}", tempo.bpm);
        assert!((70.0..=180.0).contains(&tempo.bpm));
        // A key, a real confidence and our chroma come back. Which key is
        // stratum-dsp's call (it can vary between runs on near-ties); the
        // mapping itself is checked exhaustively above, and accuracy is
        // compared in docs/analysis.md.
        let key = StratumKey.detect_key(&audio, RATE).unwrap();
        assert!(
            key.confidence > 0.0 && key.confidence <= 1.0,
            "{}",
            key.confidence
        );
        let loudest = key.chroma.iter().copied().fold(0.0_f32, f32::max);
        assert!((loudest - 1.0).abs() < 1e-6);
        assert!(key.chroma.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn its_fallback_answers_become_no_answer() {
        // Too short for us: stratum-dsp would answer 40 BPM (folded to 80).
        assert_eq!(
            StratumTempo.detect_tempo(&synth::sine(440.0, -6.0, 0.2, RATE), RATE),
            None
        );
        // At a very low rate it skips key detection and answers C major.
        let low_rate = synth::sine(440.0, -6.0, 4.0, 1000);
        assert_eq!(StratumKey.detect_key(&low_rate, 1000), None);
    }

    #[test]
    fn unusable_input_gives_no_answer() {
        assert_eq!(StratumTempo.detect_tempo(&[], RATE), None);
        assert_eq!(StratumKey.detect_key(&[0.5; 100], 0), None);
        assert_eq!(StratumTempo.detect_tempo(&[f32::NAN; 44_100], RATE), None);
    }
}
