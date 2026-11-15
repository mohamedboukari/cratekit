//! Synthetic test audio: tones, kicks, chords and full loops with known
//! answers (BPM, key, level), so DSP code is tested without copyrighted audio.
//!
//! Compiled only for tests, or for other crates through the `test-helpers`
//! cargo feature; release builds never contain it.
//!
//! Every function is pure: the same inputs give the same samples. Output is
//! mono `f32` at the given sample rate, 0 dBFS = an amplitude of 1.0.
//!
//! # Examples
//!
//! ```
//! use cratekit_core::analysis::key::{Key, Mode, PitchClass};
//! use cratekit_core::synth::{peak_db, test_loop};
//!
//! // The SPEC acceptance loop: kick, bass and chords at 124 BPM in A minor (8A).
//! let a_minor = Key { tonic: PitchClass::A, mode: Mode::Minor };
//! let samples = test_loop(124.0, a_minor, 8.0, 44_100);
//! assert_eq!(samples.len(), 8 * 44_100);
//! assert!((peak_db(&samples) - -1.0).abs() < 0.01);
//! ```

use std::f64::consts::TAU;

use crate::analysis::key::Key;

/// Concert pitch: A4 (MIDI note 69) is 440 Hz.
const A4_HZ: f64 = 440.0;
/// One chord per bar of this many beats.
const BEATS_PER_BAR: f64 = 4.0;
/// Fade at the start and end of each note, so notes do not click.
const FADE_SECONDS: f32 = 0.01;
/// The chord progression as scale degrees (0 = tonic): I, IV, V, I.
const PROGRESSION: [usize; 4] = [0, 3, 4, 0];

/// Number of samples in `seconds` (negative durations give none).
fn sample_count(seconds: f32, sample_rate: u32) -> usize {
    (f64::from(seconds.max(0.0)) * f64::from(sample_rate)).round() as usize
}

/// Peak amplitude for a level in dBFS: -6 dB is about 0.501.
fn db_to_amplitude(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Frequency of a MIDI note number (60 is middle C, 69 is A4).
fn midi_to_hz(note: u8) -> f64 {
    A4_HZ * 2f64.powf((f64::from(note) - 69.0) / 12.0)
}

/// A gain from 0 to 1 that fades in and out over `FADE_SECONDS`.
fn fade_gain(n: usize, len: usize, sample_rate: u32) -> f32 {
    let fade = sample_count(FADE_SECONDS, sample_rate).max(1);
    let from_end = len.saturating_sub(n + 1);
    let edge = n.min(from_end);
    if edge >= fade {
        1.0
    } else {
        edge as f32 / fade as f32
    }
}

/// Adds a sine note into `out`, starting at sample `start`, with fades.
fn add_note(out: &mut [f32], start: usize, len: usize, hz: f64, amplitude: f32, sample_rate: u32) {
    let step = TAU * hz / f64::from(sample_rate);
    for n in 0..len {
        let Some(slot) = out.get_mut(start + n) else {
            break;
        };
        let value = (step * n as f64).sin() as f32;
        *slot += value * amplitude * fade_gain(n, len, sample_rate);
    }
}

/// A pure tone of `freq_hz` whose peak is `level_db` dBFS.
#[must_use]
pub fn sine(freq_hz: f32, level_db: f32, seconds: f32, sample_rate: u32) -> Vec<f32> {
    let amplitude = db_to_amplitude(level_db);
    let step = TAU * f64::from(freq_hz) / f64::from(sample_rate);
    (0..sample_count(seconds, sample_rate))
        .map(|n| (step * n as f64).sin() as f32 * amplitude)
        .collect()
}

/// Start samples of events every `every_beats` beats, the first `offset_beats`
/// beats in, before `total`. Each start is worked out from its own index, so
/// positions never drift over long durations. Empty for a BPM that is not a
/// finite positive number, or so fast that the events would be less than a
/// sample apart (this also keeps the loop bounded).
fn beat_starts(
    bpm: f32,
    offset_beats: f64,
    every_beats: f64,
    total: usize,
    sample_rate: u32,
) -> Vec<usize> {
    if !(bpm.is_finite() && bpm > 0.0) {
        return Vec::new();
    }
    let beat_samples = 60.0 / f64::from(bpm) * f64::from(sample_rate);
    if beat_samples * every_beats < 1.0 {
        return Vec::new();
    }
    (0_u32..)
        .map(|k| ((f64::from(k) * every_beats + offset_beats) * beat_samples).round() as usize)
        .take_while(|&start| start < total)
        .collect()
}

/// One synthetic kick drum on every beat, the first at sample 0.
///
/// Each kick is a sine that drops from 120 Hz to 50 Hz in about 100 ms and
/// fades out quickly (with a short fade at the very end, so it does not
/// click), so the energy jumps exactly on each beat (every `60 / bpm`
/// seconds) and is near silence just before the next one.
#[must_use]
pub fn kick(bpm: f32, seconds: f32, sample_rate: u32) -> Vec<f32> {
    let total = sample_count(seconds, sample_rate);
    let mut out = vec![0.0; total];
    let kick_len = sample_count(0.25, sample_rate);
    let tail = sample_count(FADE_SECONDS, sample_rate).max(1);
    let rate = f64::from(sample_rate);
    for start in beat_starts(bpm, 0.0, 1.0, total, sample_rate) {
        let mut phase = 0.0_f64;
        for n in 0..kick_len {
            let Some(slot) = out.get_mut(start + n) else {
                break;
            };
            let t = n as f64 / rate;
            let hz = 50.0 + 70.0 * (-t / 0.03).exp();
            // Fade out over the last few milliseconds only: the attack stays sharp.
            let left = kick_len - n;
            let tail_gain = if left < tail {
                left as f64 / tail as f64
            } else {
                1.0
            };
            *slot += (phase.sin() * (-t / 0.05).exp() * 0.9 * tail_gain) as f32;
            phase += TAU * hz / rate;
        }
    }
    out
}

/// Short noise bursts (hi-hat or snare-like), `per_beat` of them per beat at
/// `bpm`, starting `offset_beats` into each beat, each peaking at about
/// `level_db` dBFS and fading out within 40 ms.
///
/// The noise comes from a fixed simple generator, so it is the same on every
/// run.
#[must_use]
pub fn hats(
    bpm: f32,
    per_beat: u32,
    offset_beats: f32,
    level_db: f32,
    seconds: f32,
    sample_rate: u32,
) -> Vec<f32> {
    let total = sample_count(seconds, sample_rate);
    let mut out = vec![0.0; total];
    if per_beat == 0 {
        return out;
    }
    let step = 1.0 / f64::from(per_beat);
    let starts = beat_starts(
        bpm * per_beat as f32,
        f64::from(offset_beats) / step,
        1.0,
        total,
        sample_rate,
    );
    let burst = sample_count(0.04, sample_rate);
    let amplitude = db_to_amplitude(level_db);
    let rate = f64::from(sample_rate);
    let mut state: u32 = 0x9E37_79B9;
    for start in starts {
        for n in 0..burst {
            let Some(slot) = out.get_mut(start + n) else {
                break;
            };
            // xorshift: a tiny, fixed pseudo-random sequence.
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = (f64::from(state) / f64::from(u32::MAX)) * 2.0 - 1.0;
            let envelope = (-(n as f64 / rate) / 0.008).exp();
            *slot += (noise * envelope) as f32 * amplitude;
        }
    }
    out
}

/// The 7 notes of `key` as semitones above its tonic, lowest first.
fn scale_offsets(key: Key) -> Vec<u8> {
    let in_key = key.scale();
    (0..12_u8)
        .filter(|offset| in_key[usize::from((key.tonic.index() + offset) % 12)])
        .collect()
}

/// A I, IV, V, I chord progression in `key` (i, iv, v, i in minor), one
/// chord per bar of 4 beats at `bpm`, voiced from middle C upwards.
///
/// Chords change on bar lines, like a real loop, so the changes line up with
/// the kicks. Every note is in the key and all 7 scale notes are played, so
/// a key detector has a clear answer; the tonic chord starts and ends each
/// cycle.
#[must_use]
pub fn chords(key: Key, bpm: f32, seconds: f32, sample_rate: u32) -> Vec<f32> {
    let total = sample_count(seconds, sample_rate);
    let mut out = vec![0.0; total];
    let offsets = scale_offsets(key);
    let bars = beat_starts(bpm, 0.0, BEATS_PER_BAR, total, sample_rate);
    if offsets.len() != 7 {
        return out;
    }
    let root_note = 60 + key.tonic.index();
    for (bar, &start) in bars.iter().enumerate() {
        let end = bars.get(bar + 1).copied().unwrap_or(total);
        let degree = PROGRESSION[bar % PROGRESSION.len()];
        // A triad stacks every other scale note: degree, degree + 2, degree + 4.
        for step in [0, 2, 4] {
            let index = degree + step;
            let octave = u8::try_from(index / 7).unwrap_or(0);
            let note = root_note + offsets[index % 7] + 12 * octave;
            add_note(
                &mut out,
                start,
                end - start,
                midi_to_hz(note),
                0.2,
                sample_rate,
            );
        }
    }
    out
}

/// The key's root note, two octaves below middle C, on every off-beat.
#[must_use]
pub fn bass(key: Key, bpm: f32, seconds: f32, sample_rate: u32) -> Vec<f32> {
    let total = sample_count(seconds, sample_rate);
    let mut out = vec![0.0; total];
    let starts = beat_starts(bpm, 0.5, 1.0, total, sample_rate);
    let note_len = sample_count(0.4 * 60.0 / bpm, sample_rate);
    let hz = midi_to_hz(36 + key.tonic.index());
    for start in starts {
        add_note(&mut out, start, note_len, hz, 0.5, sample_rate);
    }
    out
}

/// The SPEC acceptance loop: kick, bass and chords at `bpm` in `key`,
/// mixed and normalized to a -1 dBFS peak.
#[must_use]
pub fn test_loop(bpm: f32, key: Key, seconds: f32, sample_rate: u32) -> Vec<f32> {
    let kick = kick(bpm, seconds, sample_rate);
    let bass = bass(key, bpm, seconds, sample_rate);
    let chords = chords(key, bpm, seconds, sample_rate);
    normalize(mix(&[&kick, &bass, &chords]), -1.0)
}

/// Sums sources sample by sample; the result is as long as the longest.
#[must_use]
pub fn mix(parts: &[&[f32]]) -> Vec<f32> {
    let len = parts.iter().map(|p| p.len()).max().unwrap_or(0);
    let mut out = vec![0.0; len];
    for part in parts {
        for (slot, value) in out.iter_mut().zip(part.iter()) {
            *slot += value;
        }
    }
    out
}

/// Scales `samples` so the loudest one peaks at `peak_db` dBFS. Silence
/// stays silent.
#[must_use]
pub fn normalize(mut samples: Vec<f32>, peak_db: f32) -> Vec<f32> {
    let peak = samples.iter().fold(0.0_f32, |max, s| max.max(s.abs()));
    if peak > 0.0 {
        let gain = db_to_amplitude(peak_db) / peak;
        for sample in &mut samples {
            *sample *= gain;
        }
    }
    samples
}

/// The highest absolute sample in dBFS (negative infinity for silence).
#[must_use]
pub fn peak_db(samples: &[f32]) -> f32 {
    let peak = samples.iter().fold(0.0_f32, |max, s| max.max(s.abs()));
    20.0 * peak.log10()
}

/// The root-mean-square level in dBFS (negative infinity for silence). A
/// full-scale sine is about -3 dBFS RMS.
#[must_use]
pub fn rms_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return f32::NEG_INFINITY;
    }
    let mean_square =
        samples.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / samples.len() as f64;
    (10.0 * mean_square.log10()) as f32
}

/// How much of `freq_hz` is in `samples` (Goertzel algorithm): the power of
/// that one frequency, normalized by length so different lengths compare.
///
/// Measure one steady stretch at a time (one note, one chord): over a longer
/// signal where notes stop and restart, their phases add up or cancel, so the
/// number no longer reflects how much the note was played.
#[must_use]
pub fn tone_power(samples: &[f32], freq_hz: f32, sample_rate: u32) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let coeff = 2.0 * (TAU * f64::from(freq_hz) / f64::from(sample_rate)).cos();
    let (mut previous, mut before) = (0.0_f64, 0.0_f64);
    for sample in samples {
        let current = f64::from(*sample) + coeff * previous - before;
        before = previous;
        previous = current;
    }
    let power = previous.powi(2) + before.powi(2) - coeff * previous * before;
    (power / (samples.len() as f64).powi(2)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::key::{Mode, PitchClass};

    const RATE: u32 = 44_100;
    /// The SPEC acceptance tempos.
    const TEMPOS: [f32; 4] = [92.0, 124.0, 128.0, 140.0];

    fn key(tonic: PitchClass, mode: Mode) -> Key {
        Key { tonic, mode }
    }

    #[test]
    fn sine_peaks_at_the_requested_level_and_frequency() {
        for level in [0.0, -6.0, -20.0] {
            let tone = sine(440.0, level, 1.0, RATE);
            assert_eq!(tone.len(), RATE as usize);
            assert!((peak_db(&tone) - level).abs() < 0.01, "level {level}");
        }
        // Upward zero crossings over one second count the cycles.
        let tone = sine(440.0, -6.0, 1.0, RATE);
        let cycles = tone
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        assert!((cycles as f32 - 440.0).abs() <= 1.0, "{cycles} cycles");
    }

    #[test]
    fn a_full_scale_sine_is_about_minus_3_db_rms() {
        let tone = sine(1000.0, 0.0, 1.0, RATE);
        assert!((rms_db(&tone) - -3.01).abs() < 0.05);
    }

    /// Indices of 10 ms windows where the energy jumps (a new kick starts).
    fn onsets(samples: &[f32]) -> Vec<usize> {
        let window = (RATE / 100) as usize;
        let energy: Vec<f32> = samples
            .chunks(window)
            .map(|w| w.iter().map(|s| s * s).sum())
            .collect();
        let loudest = energy.iter().fold(0.0_f32, |a, &b| a.max(b));
        (0..energy.len())
            .filter(|&i| {
                let before = if i == 0 { 0.0 } else { energy[i - 1] };
                energy[i] > loudest * 0.1 && energy[i] > before * 10.0
            })
            .collect()
    }

    #[test]
    fn kicks_land_exactly_on_every_beat_at_the_spec_tempos() {
        let seconds = 10.0;
        for bpm in TEMPOS {
            let found = onsets(&kick(bpm, seconds, RATE));
            let beat = 60.0 / bpm;
            let expected: Vec<f32> = (0..)
                .map(|k| k as f32 * beat)
                .take_while(|&t| t < seconds)
                .collect();
            assert_eq!(found.len(), expected.len(), "{bpm} BPM");
            for (&window, &t) in found.iter().zip(&expected) {
                let expected_window = (t * 100.0).floor() as usize;
                assert!(
                    window.abs_diff(expected_window) <= 1,
                    "{bpm} BPM: onset in window {window}, expected {expected_window}"
                );
            }
        }
    }

    /// Power of one pitch class across octaves 2 to 6, measured one bar at a
    /// time (one steady chord per window) and summed, as `tone_power` asks.
    fn pitch_class_power(samples: &[f32], pitch: PitchClass, bpm: f32) -> f32 {
        let bars = beat_starts(bpm, 0.0, BEATS_PER_BAR, samples.len(), RATE);
        bars.iter()
            .enumerate()
            .map(|(b, &start)| {
                let end = bars.get(b + 1).copied().unwrap_or(samples.len());
                let window = &samples[start..end];
                (2_u8..=6)
                    .map(|octave| {
                        let note = 12 * (octave + 1) + pitch.index();
                        tone_power(window, midi_to_hz(note) as f32, RATE)
                    })
                    .sum::<f32>()
            })
            .sum()
    }

    #[test]
    fn chords_play_only_the_key_and_favour_its_tonic_over_the_relative_key() {
        let keys = [
            key(PitchClass::C, Mode::Major),
            key(PitchClass::A, Mode::Minor),
            key(PitchClass::FSharp, Mode::Major),
            key(PitchClass::DSharp, Mode::Minor),
        ];
        for bpm in [120.0, 124.0] {
            for k in keys {
                let audio = chords(k, bpm, 16.0, RATE);
                let in_key = k.scale();
                let powers: Vec<f32> = PitchClass::ALL
                    .iter()
                    .map(|&p| pitch_class_power(&audio, p, bpm))
                    .collect();
                let at = |offset: u8| powers[usize::from((k.tonic.index() + offset) % 12)];
                let weakest_in = (0..12)
                    .filter(|&i| in_key[i])
                    .map(|i| powers[i])
                    .fold(f32::INFINITY, f32::min);
                let loudest_out = (0..12)
                    .filter(|&i| !in_key[i])
                    .map(|i| powers[i])
                    .fold(0.0, f32::max);
                assert!(
                    weakest_in > loudest_out * 10.0,
                    "{k} at {bpm}: in-key {weakest_in} vs out {loudest_out}"
                );
                // I-IV-V-I: the tonic and the fifth sound in 3 of 4 bars, more than any other note.
                let strongest = (0_u8..12)
                    .max_by(|&a, &b| at(a).total_cmp(&at(b)))
                    .unwrap_or(0);
                assert!(
                    [0, 7].contains(&strongest),
                    "{k} at {bpm}: strongest offset {strongest}"
                );
                // The relative key's tonic (3 up in minor, 9 up in major) must be weaker.
                let relative = match k.mode {
                    Mode::Major => 9,
                    Mode::Minor => 3,
                };
                assert!(
                    at(0) > at(relative) * 1.2,
                    "{k} at {bpm}: tonic vs relative tonic"
                );
            }
        }
    }

    #[test]
    fn chord_changes_fall_on_bar_lines() {
        for bpm in TEMPOS {
            let bars = beat_starts(bpm, 0.0, BEATS_PER_BAR, 10 * RATE as usize, RATE);
            let bar = 4.0 * 60.0 / bpm;
            for (b, &start) in bars.iter().enumerate() {
                let expected = (b as f32 * bar * RATE as f32).round() as usize;
                assert!(start.abs_diff(expected) <= 1, "{bpm} BPM bar {b}");
            }
        }
    }

    #[test]
    fn bass_plays_the_root_on_every_off_beat() {
        let a_minor = key(PitchClass::A, Mode::Minor);
        for bpm in TEMPOS {
            let audio = bass(a_minor, bpm, 10.0, RATE);
            let found = onsets(&audio);
            let beat = 60.0 / bpm;
            let expected: Vec<f32> = (0..)
                .map(|k| (k as f32 + 0.5) * beat)
                .take_while(|&t| t < 10.0)
                .collect();
            assert_eq!(found.len(), expected.len(), "{bpm} BPM");
            for (&window, &t) in found.iter().zip(&expected) {
                let expected_window = (t * 100.0).floor() as usize;
                assert!(
                    window.abs_diff(expected_window) <= 1,
                    "{bpm} BPM: {window} vs {expected_window}"
                );
            }
            // Over the first note, A2 (MIDI 45, 110 Hz) is far stronger than its
            // neighbours a semitone away.
            let start = (0.5 * beat * RATE as f32).round() as usize;
            let first_note = &audio[start..start + (0.4 * beat * RATE as f32) as usize];
            let note = |midi| tone_power(first_note, midi_to_hz(midi) as f32, RATE);
            assert!(note(45) > 10.0 * note(44).max(note(46)), "{bpm} BPM pitch");
        }
    }

    #[test]
    fn the_test_loop_keeps_the_kicks_on_the_beat() {
        let audio = test_loop(128.0, key(PitchClass::A, Mode::Minor), 8.0, RATE);
        let kick_only = onsets(&kick(128.0, 8.0, RATE));
        // Every kick start is still an energy jump in the full mix.
        let window = (RATE / 100) as usize;
        let energy: Vec<f32> = audio
            .chunks(window)
            .map(|w| w.iter().map(|s| s * s).sum())
            .collect();
        for &w in &kick_only {
            let before = if w == 0 { 0.0 } else { energy[w - 1] };
            assert!(energy[w] > before, "no onset at window {w}");
        }
    }

    #[test]
    fn the_test_loop_is_full_length_finite_and_peaks_at_minus_1_db() {
        for bpm in TEMPOS {
            let audio = test_loop(bpm, key(PitchClass::A, Mode::Minor), 8.0, RATE);
            assert_eq!(audio.len(), 8 * RATE as usize);
            assert!(audio.iter().all(|s| s.is_finite()), "{bpm} BPM");
            assert!((peak_db(&audio) - -1.0).abs() < 0.01, "{bpm} BPM");
        }
    }

    #[test]
    fn mix_and_normalize_keep_lengths_and_handle_silence() {
        let mixed = mix(&[&[0.5, 0.5], &[0.25, 0.25, 0.25]]);
        assert_eq!(mixed, vec![0.75, 0.75, 0.25]);
        assert!(mix(&[]).is_empty());
        let silent = normalize(vec![0.0; 4], -1.0);
        assert_eq!(silent, vec![0.0; 4]);
        assert_eq!(peak_db(&silent), f32::NEG_INFINITY);
        assert_eq!(rms_db(&[]), f32::NEG_INFINITY);
        let loud = normalize(vec![0.1, -0.2], 0.0);
        assert!((peak_db(&loud) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn invalid_inputs_give_silence_not_a_panic() {
        assert!(kick(0.0, 1.0, RATE).iter().all(|&s| s == 0.0));
        assert!(
            bass(key(PitchClass::C, Mode::Major), -10.0, 1.0, RATE)
                .iter()
                .all(|&s| s == 0.0)
        );
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.0e12] {
            assert!(kick(bad, 1.0, RATE).iter().all(|&s| s == 0.0), "kick {bad}");
            let a = key(PitchClass::A, Mode::Minor);
            assert!(
                bass(a, bad, 1.0, RATE).iter().all(|&s| s == 0.0),
                "bass {bad}"
            );
            assert!(
                chords(a, bad, 1.0, RATE).iter().all(|&s| s == 0.0),
                "chords {bad}"
            );
        }
        assert!(sine(440.0, 0.0, -1.0, RATE).is_empty());
        assert_eq!(tone_power(&[], 440.0, RATE), 0.0);
    }
}
