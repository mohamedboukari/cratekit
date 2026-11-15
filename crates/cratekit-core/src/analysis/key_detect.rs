//! Musical key detection (SPEC, Module 2).
//!
//! [`KeyDetector`] is the interface the rest of Cratekit uses, so the
//! implementation can be swapped (`stratum-dsp` is the second one, behind
//! the `stratum` feature: see `analysis::stratum`). [`ProfileKey`]
//! is our own: it measures how much of each of the 12 notes the audio
//! contains (a chroma vector), then picks the key whose typical note
//! profile matches it best (Krumhansl-Kessler profiles).

use std::f64::consts::TAU;

use crate::analysis::key::{Key, Mode, PitchClass};

/// The key of a piece of audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyEstimate {
    /// The detected key (its Camelot code is `key.camelot()`).
    pub key: Key,
    /// How clearly this key beats the others, 0 to 1.
    pub confidence: f32,
    /// How loud each pitch class is relative to the loudest, C to B, 0 to 1
    /// (relative amplitude: a note at half the level of the loudest shows as
    /// 0.5). This is what the UI draws; matching uses power internally.
    pub chroma: [f32; 12],
}

/// Finds the musical key of mono audio.
///
/// Implementations analyze whatever they are given; choosing which part of a
/// track to analyze is the caller's job.
pub trait KeyDetector {
    /// The key of `samples` (mono, `sample_rate` Hz), or `None` when there
    /// are no notes to go on: silence, too short, or invalid samples.
    fn detect_key(&self, samples: &[f32], sample_rate: u32) -> Option<KeyEstimate>;
}

/// Krumhansl and Kessler's probe-tone ratings (1982): how well each note,
/// counted in semitones above the tonic, fits a major key. Published data.
const MAJOR_PROFILE: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
/// The same ratings for a minor key.
const MINOR_PROFILE: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
/// Notes measured: C3 (MIDI 48) up to B6 (MIDI 95), 4 octaves. Lower notes
/// sit too close together for the frame length to tell apart.
const LOWEST_NOTE: u8 = 48;
const HIGHEST_NOTE: u8 = 95;
/// Analysis frame length, in seconds (8192 samples at 44.1 kHz).
const FRAME_SECONDS: f64 = 0.186;

/// Our key detector: a chroma vector from a bank of note filters, matched
/// against the 24 Krumhansl-Kessler key profiles.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProfileKey;

impl KeyDetector for ProfileKey {
    fn detect_key(&self, samples: &[f32], sample_rate: u32) -> Option<KeyEstimate> {
        if sample_rate == 0 || samples.iter().any(|s| !s.is_finite()) {
            return None;
        }
        let power = chroma_power(samples, sample_rate)?;
        let chroma = relative_loudness(&power)?;
        let (key, confidence) = best_key(&power);
        Some(KeyEstimate {
            key,
            confidence,
            chroma,
        })
    }
}

/// The chroma the UI draws, C to B: each pitch class's loudness relative to
/// the loudest (0 to 1), or `None` when there are no notes (silence, too
/// short, invalid). Shared with the `stratum-dsp` key detector, which does
/// not report a chroma itself.
#[cfg(feature = "stratum")]
pub(crate) fn chroma(samples: &[f32], sample_rate: u32) -> Option<[f32; 12]> {
    if sample_rate == 0 || samples.iter().any(|s| !s.is_finite()) {
        return None;
    }
    relative_loudness(&chroma_power(samples, sample_rate)?)
}

/// Power per pitch class turned into loudness relative to the loudest.
fn relative_loudness(power: &[f64; 12]) -> Option<[f32; 12]> {
    let strongest = power.iter().copied().fold(0.0_f64, f64::max);
    if strongest <= f64::EPSILON {
        return None;
    }
    let mut chroma = [0.0_f32; 12];
    // Power is amplitude squared: the square root gives relative loudness.
    for (slot, value) in chroma.iter_mut().zip(power) {
        *slot = (value / strongest).sqrt() as f32;
    }
    Some(chroma)
}

/// Frequency of a MIDI note number (69 is A4 at 440 Hz).
fn midi_to_hz(note: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(note) - 69.0) / 12.0)
}

/// Total power of each pitch class (C to B) across the clip, or `None` when
/// the clip is shorter than one frame.
///
/// Each frame is tapered at both ends (a Hann window) so a note's energy
/// does not smear into its neighbours, then a Goertzel filter per note
/// measures the power at that note's frequency. Power (not amplitude) keeps
/// strong notes dominant, so bass overtones do not tip a minor key to major.
fn chroma_power(samples: &[f32], sample_rate: u32) -> Option<[f64; 12]> {
    let frame_len = (FRAME_SECONDS * f64::from(sample_rate)).round() as usize;
    if frame_len < 2 || samples.len() < frame_len {
        return None;
    }
    let window: Vec<f64> = (0..frame_len)
        .map(|n| 0.5 - 0.5 * (TAU * n as f64 / (frame_len - 1) as f64).cos())
        .collect();
    let notes: Vec<(usize, f64)> = (LOWEST_NOTE..=HIGHEST_NOTE)
        .map(|note| {
            let coeff = 2.0 * (TAU * midi_to_hz(note) / f64::from(sample_rate)).cos();
            (usize::from(note % 12), coeff)
        })
        .collect();
    let mut power = [0.0_f64; 12];
    let mut frame = vec![0.0_f64; frame_len];
    for chunk in samples.chunks_exact(frame_len) {
        for ((slot, &sample), &w) in frame.iter_mut().zip(chunk).zip(&window) {
            *slot = f64::from(sample) * w;
        }
        for &(pitch_class, coeff) in &notes {
            power[pitch_class] += goertzel_power(&frame, coeff);
        }
    }
    Some(power)
}

/// Power of one frequency in `frame` (Goertzel algorithm), where
/// `coeff = 2 cos(2 pi f / sample_rate)`.
fn goertzel_power(frame: &[f64], coeff: f64) -> f64 {
    let (mut previous, mut before) = (0.0_f64, 0.0_f64);
    for &sample in frame {
        let current = sample + coeff * previous - before;
        before = previous;
        previous = current;
    }
    previous.powi(2) + before.powi(2) - coeff * previous * before
}

/// Pearson correlation between two 12-value profiles: 1 when they rise and
/// fall together, 0 when unrelated.
fn correlation(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let mean_a = a.iter().sum::<f64>() / 12.0;
    let mean_b = b.iter().sum::<f64>() / 12.0;
    let (mut cross, mut var_a, mut var_b) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let (dx, dy) = (x - mean_a, y - mean_b);
        cross += dx * dy;
        var_a += dx * dx;
        var_b += dy * dy;
    }
    let spread = (var_a * var_b).sqrt();
    if spread > 0.0 { cross / spread } else { 0.0 }
}

/// The key whose profile, turned to start on its tonic, correlates best with
/// the chroma; and a confidence: how far the winner is ahead of the
/// runner-up, compared with how far it is ahead of the average key.
fn best_key(power: &[f64; 12]) -> (Key, f32) {
    let mut scored: Vec<(Key, f64)> = Vec::with_capacity(24);
    for tonic in PitchClass::ALL {
        for (mode, profile) in [(Mode::Major, &MAJOR_PROFILE), (Mode::Minor, &MINOR_PROFILE)] {
            // The profile value for pitch class `pc` is the rating of the
            // interval from the tonic up to `pc`.
            let mut turned = [0.0_f64; 12];
            for (pc, slot) in turned.iter_mut().enumerate() {
                let interval = (pc + 12 - usize::from(tonic.index())) % 12;
                *slot = profile[interval];
            }
            scored.push((Key { tonic, mode }, correlation(power, &turned)));
        }
    }
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let best = scored.first().map_or(f64::NAN, |&(_, s)| s);
    let second = scored.get(1).map_or(best, |&(_, s)| s);
    let average = scored.iter().map(|&(_, s)| s).sum::<f64>() / scored.len() as f64;
    let confidence = if best > average {
        ((best - second) / (best - average)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let key = scored.first().map_or(
        Key {
            tonic: PitchClass::C,
            mode: Mode::Major,
        },
        |&(k, _)| k,
    );
    (key, confidence as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    const RATE: u32 = 44_100;
    const TEMPOS: [f32; 4] = [92.0, 124.0, 128.0, 140.0];

    fn detect(samples: &[f32]) -> Option<KeyEstimate> {
        ProfileKey.detect_key(samples, RATE)
    }

    fn all_keys() -> Vec<Key> {
        PitchClass::ALL
            .iter()
            .flat_map(|&tonic| [Mode::Major, Mode::Minor].map(|mode| Key { tonic, mode }))
            .collect()
    }

    #[test]
    fn finds_every_key_from_its_chords() {
        for key in all_keys() {
            let found = detect(&synth::chords(key, 120.0, 8.0, RATE)).unwrap();
            assert_eq!(found.key, key, "chords in {key}");
            assert_eq!(found.key.camelot(), key.camelot());
        }
    }

    #[test]
    fn spec_acceptance_bpm_and_key_on_the_synthetic_loops() {
        use crate::analysis::tempo::{OnsetTempo, TempoDetector};
        let keys = [
            Key {
                tonic: PitchClass::C,
                mode: Mode::Major,
            },
            Key {
                tonic: PitchClass::A,
                mode: Mode::Minor,
            },
            Key {
                tonic: PitchClass::FSharp,
                mode: Mode::Major,
            },
            Key {
                tonic: PitchClass::DSharp,
                mode: Mode::Minor,
            },
        ];
        for bpm in TEMPOS {
            for key in keys {
                let audio = synth::test_loop(bpm, key, 10.0, RATE);
                let tempo = OnsetTempo.detect_tempo(&audio, RATE).unwrap();
                assert!(
                    (tempo.bpm - bpm).abs() <= 0.5,
                    "{key} at {bpm}: {} BPM",
                    tempo.bpm
                );
                let found = detect(&audio).unwrap();
                assert_eq!(found.key, key, "{key} at {bpm}");
                assert_eq!(found.key.camelot().to_string(), key.camelot().to_string());
            }
        }
    }

    #[test]
    fn the_chroma_is_normalized_and_peaks_in_the_tonic_triad() {
        let key = Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        };
        let found = detect(&synth::chords(key, 120.0, 8.0, RATE)).unwrap();
        let max = found.chroma.iter().copied().fold(0.0_f32, f32::max);
        assert!((max - 1.0).abs() < 1e-6);
        assert!(found.chroma.iter().all(|&v| (0.0..=1.0).contains(&v)));
        let strongest = (0..12)
            .max_by(|&a, &b| found.chroma[a].total_cmp(&found.chroma[b]))
            .unwrap();
        // A, C or E.
        assert!(
            [9, 0, 4].contains(&strongest),
            "strongest pitch class {strongest}"
        );
        assert!((0.0..=1.0).contains(&found.confidence));
    }

    #[test]
    fn a_single_note_shows_up_on_its_own_pitch_class() {
        let found = detect(&synth::sine(440.0, -6.0, 2.0, RATE)).unwrap();
        let strongest = (0..12)
            .max_by(|&a, &b| found.chroma[a].total_cmp(&found.chroma[b]))
            .unwrap();
        assert_eq!(strongest, usize::from(PitchClass::A.index()));
    }

    #[test]
    fn no_notes_means_no_key() {
        assert_eq!(detect(&[]), None);
        assert_eq!(detect(&vec![0.0; 2 * RATE as usize]), None);
        assert_eq!(
            detect(&synth::sine(440.0, -6.0, 0.05, RATE)),
            None,
            "too short"
        );
        let mut broken = synth::sine(440.0, -6.0, 2.0, RATE);
        broken[10] = f32::INFINITY;
        assert_eq!(detect(&broken), None, "non-finite sample");
        assert_eq!(
            ProfileKey.detect_key(&synth::sine(440.0, -6.0, 2.0, RATE), 0),
            None
        );
    }

    #[test]
    fn the_same_audio_always_gives_the_same_key() {
        let key = Key {
            tonic: PitchClass::E,
            mode: Mode::Minor,
        };
        let audio = synth::chords(key, 120.0, 8.0, RATE);
        assert_eq!(detect(&audio), detect(&audio));
    }

    #[test]
    fn correlation_is_one_for_the_same_shape_and_zero_for_flat() {
        assert!((correlation(&MAJOR_PROFILE, &MAJOR_PROFILE) - 1.0).abs() < 1e-12);
        assert_eq!(correlation(&[1.0; 12], &MAJOR_PROFILE), 0.0);
    }
}
