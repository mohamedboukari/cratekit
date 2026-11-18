//! Energy score (SPEC, Module 2): how intense a track feels, from 1 (calm) to
//! 10 (peak time), derived from loudness, onset density and brightness.
//!
//! This is a heuristic for ranking tracks against each other, not a physical
//! measurement. Each feature is mapped to 0 to 1 over a fixed range, the three
//! are mixed with fixed weights, and the result is spread over 1 to 10. The
//! ranges and weights are first guesses checked on synthetic audio; they get
//! re-tuned on real DJ tracks (ROADMAP #22).

use crate::analysis::tempo::{
    HOP_SECONDS, MIN_SECONDS, WINDOW_SECONDS, onset_envelope, onset_envelope_of,
};

/// Integrated loudness that maps to 0 (quiet ambient, an unmastered demo).
const QUIET_LUFS: f64 = -30.0;
/// Integrated loudness that maps to 1 (a hot club master).
const LOUD_LUFS: f64 = -6.0;
/// Onsets per second that map to 1 (16th notes at 120 BPM). Zero maps to 0.
const BUSY_ONSETS_PER_SECOND: f64 = 8.0;
/// Average frequency (see `brightness`) that maps to 0: all bass.
const DARK_HZ: f64 = 100.0;
/// Average frequency that maps to 1: as bright as a 4 kHz tone.
const BRIGHT_HZ: f64 = 4000.0;
/// Weights of loudness, onset density and brightness (they sum to 1).
const WEIGHTS: (f64, f64, f64) = (0.4, 0.35, 0.25);
/// Smallest rise in log energy, summed over `RISE_SECONDS`, that counts as a
/// new sound starting: about a doubling of the energy (+3 dB). The ripple of
/// a steady tone stays far below it.
const MIN_RISE: f64 = 0.7;
/// The rise of one onset is summed over this long (the onset envelope smooths
/// over 40 ms, so a hit rises over about that long).
const RISE_SECONDS: f64 = 0.04;
/// Two onsets closer than this count as one (a flam, a kick under a hat).
const MIN_GAP_SECONDS: f64 = 0.05;

/// The three features behind the score, each from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Features {
    loudness: f64,
    density: f64,
    brightness: f64,
}

impl Features {
    fn score(self) -> u8 {
        let (loudness, density, brightness) = WEIGHTS;
        let mixed =
            loudness * self.loudness + density * self.density + brightness * self.brightness;
        // `mixed` is within 0 to 1, so this is within 1 to 10.
        1 + (9.0 * mixed.clamp(0.0, 1.0)).round() as u8
    }
}

/// The energy score of a track, from 1 (calm) to 10 (peak time).
///
/// `samples` is the track in mono (channels averaged) at `sample_rate`, and
/// `integrated_lufs` its integrated loudness from
/// [`measure_loudness`](crate::analysis::loudness::measure_loudness).
///
/// Returns `None` when there is no usable audio: silence, less than 3
/// seconds, samples that are not finite numbers, or a loudness that is not a
/// finite number.
#[must_use]
pub fn energy_score(samples: &[f32], sample_rate: u32, integrated_lufs: f32) -> Option<u8> {
    features(samples, sample_rate, integrated_lufs).map(Features::score)
}

fn features(samples: &[f32], sample_rate: u32, integrated_lufs: f32) -> Option<Features> {
    if sample_rate == 0
        || !integrated_lufs.is_finite()
        || (samples.len() as f64) < MIN_SECONDS * f64::from(sample_rate)
        || samples.iter().any(|s| !s.is_finite())
    {
        return None;
    }
    let brightness = brightness(samples, sample_rate)?;
    let seconds = samples.len() as f64 / f64::from(sample_rate);
    let density = onset_count(samples, sample_rate) as f64 / seconds;
    Some(Features {
        loudness: unit(f64::from(integrated_lufs), QUIET_LUFS, LOUD_LUFS),
        density: unit(density, 0.0, BUSY_ONSETS_PER_SECOND),
        brightness: unit(
            brightness.max(f64::MIN_POSITIVE).log2(),
            DARK_HZ.log2(),
            BRIGHT_HZ.log2(),
        ),
    })
}

/// Maps `value` from `low..=high` onto 0 to 1, clamped at both ends.
fn unit(value: f64, low: f64, high: f64) -> f64 {
    ((value - low) / (high - low)).clamp(0.0, 1.0)
}

/// The RMS frequency of the signal, in Hz: the frequency of the single sine
/// whose sample-to-sample change, relative to its level, matches the
/// signal's. It averages the square of the frequency, weighted by power, so
/// quiet high content (hats, cymbals) pulls it up more than a plain average
/// would, which suits an energy score.
///
/// For a sine of frequency `f`, the RMS of `x[n] - x[n - 1]` is
/// `2 sin(pi f / rate)` times the RMS of `x`; this inverts that. Bright
/// sounds (hats, cymbals, distorted synths) change fast from one sample to the
/// next, bass changes slowly. No FFT needed, and a tone reads the same at any
/// sample rate. `None` for silence.
fn brightness(samples: &[f32], sample_rate: u32) -> Option<f64> {
    let power: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
    if power <= 0.0 {
        return None;
    }
    let change: f64 = samples
        .windows(2)
        .map(|pair| (f64::from(pair[1]) - f64::from(pair[0])).powi(2))
        .sum();
    let ratio = (change / power).sqrt();
    // Above 2 the signal flips sign on every sample: the Nyquist frequency.
    let half_angle = (ratio / 2.0).min(1.0).asin();
    Some(half_angle * f64::from(sample_rate) / std::f64::consts::PI)
}

/// How many new sounds start in `samples`, at least `MIN_GAP_SECONDS` apart.
///
/// Onsets are found twice and merged: once in the signal itself, which
/// catches kicks, bass notes and chord changes, and once in its
/// sample-to-sample difference, which favours high frequencies. A hi-hat over
/// a sustained bass and chord bed barely changes the total energy, but it
/// doubles the energy of the difference, so the second pass sees it.
fn onset_count(samples: &[f32], sample_rate: u32) -> usize {
    let rate = f64::from(sample_rate);
    let hop = (rate * HOP_SECONDS).round().max(1.0) as usize;
    let window = (rate * WINDOW_SECONDS).round().max(1.0) as usize;
    let mut frames = onset_frames(&onset_envelope(samples, hop, window));
    // The difference is computed as it is read, not stored: no second copy
    // of the track.
    let change = onset_envelope_of(
        samples.len().saturating_sub(1),
        |n| (f64::from(samples[n + 1]) - f64::from(samples[n])).powi(2),
        hop,
        window,
    );
    frames.extend(onset_frames(&change));
    frames.sort_unstable();
    let gap = frame_count(MIN_GAP_SECONDS);
    let mut count = 0;
    let mut last: Option<usize> = None;
    for frame in frames {
        if last.is_none_or(|previous| frame - previous >= gap) {
            count += 1;
            last = Some(frame);
        }
    }
    count
}

/// Envelope frames (every `HOP_SECONDS`) in `seconds`, at least one.
fn frame_count(seconds: f64) -> usize {
    (seconds / HOP_SECONDS).round().max(1.0) as usize
}

/// The frames where a new sound starts: peaks of an onset envelope (from the
/// tempo detector's `onset_envelope`) where the energy rises by at
/// least `MIN_RISE` within `RISE_SECONDS`, the largest within
/// `MIN_GAP_SECONDS` either side.
fn onset_frames(envelope: &[f64]) -> Vec<usize> {
    let span = frame_count(RISE_SECONDS);
    let gap = frame_count(MIN_GAP_SECONDS);

    // Total rise over the last `span` frames, for every frame.
    let mut rise = Vec::with_capacity(envelope.len());
    let mut sum = 0.0;
    for (i, &value) in envelope.iter().enumerate() {
        sum += value;
        if i >= span {
            sum -= envelope[i - span];
        }
        rise.push(sum);
    }

    // A peak: big enough and the largest within `gap` frames either side
    // (the first one wins a tie).
    rise.iter()
        .enumerate()
        .filter(|&(i, &value)| {
            value >= MIN_RISE
                && rise[i.saturating_sub(gap)..i]
                    .iter()
                    .all(|&before| before < value)
                && rise[i + 1..(i + 1 + gap).min(rise.len())]
                    .iter()
                    .all(|&after| after <= value)
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::key::{Key, Mode, PitchClass};
    use crate::analysis::loudness::measure_loudness;
    use crate::synth::{hats, kick, mix, normalize, sine, test_loop};

    const RATE: u32 = 44_100;
    const SECONDS: f32 = 12.0;
    const A_MINOR: Key = Key {
        tonic: PitchClass::A,
        mode: Mode::Minor,
    };

    fn lufs(samples: &[f32]) -> f32 {
        measure_loudness(&[samples], RATE)
            .expect("test audio has a loudness")
            .integrated_lufs
    }

    fn score(samples: &[f32]) -> u8 {
        energy_score(samples, RATE, lufs(samples)).expect("test audio has an energy")
    }

    fn features_of(samples: &[f32]) -> Features {
        features(samples, RATE, lufs(samples)).expect("test audio has features")
    }

    fn scaled(samples: &[f32], gain_db: f32) -> Vec<f32> {
        let gain = 10.0_f32.powf(gain_db / 20.0);
        samples.iter().map(|s| s * gain).collect()
    }

    #[test]
    fn the_score_always_stays_between_1_and_10() {
        let features = [
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            (0.5, 0.2, 0.9),
            (-3.0, 7.0, 0.5),
        ];
        let scores: Vec<u8> = features
            .iter()
            .map(|&(loudness, density, brightness)| {
                Features {
                    loudness,
                    density,
                    brightness,
                }
                .score()
            })
            .collect();
        assert_eq!(scores[0], 1);
        assert_eq!(scores[1], 10);
        assert!(scores.iter().all(|s| (1..=10).contains(s)), "{scores:?}");
    }

    #[test]
    fn brightness_reads_the_frequency_of_a_tone_at_any_sample_rate() {
        for rate in [44_100, 48_000, 96_000] {
            for hz in [100.0, 1000.0, 4000.0] {
                let tone = sine(hz, -6.0, 3.0, rate);
                let measured = brightness(&tone, rate).expect("a tone is not silent");
                let error = (measured - f64::from(hz)).abs() / f64::from(hz);
                assert!(error < 0.01, "{hz} Hz at {rate} Hz read as {measured}");
            }
        }
        assert_eq!(brightness(&[0.0; 1000], RATE), None);
    }

    #[test]
    fn counts_one_onset_per_kick_and_at_most_one_for_a_steady_tone() {
        for bpm in [90.0, 124.0, 174.0] {
            let kicks = kick(bpm, SECONDS, RATE);
            let expected = (f64::from(SECONDS) * f64::from(bpm) / 60.0).ceil() as usize;
            let counted = onset_count(&kicks, RATE);
            assert!(
                counted.abs_diff(expected) <= 1,
                "{bpm} BPM: counted {counted}, expected {expected}"
            );
        }
        for hz in [40.0, 110.0, 440.0] {
            let tone = sine(hz, -12.0, SECONDS, RATE);
            assert!(onset_count(&tone, RATE) <= 1, "{hz} Hz tone");
        }
    }

    #[test]
    fn a_calm_tone_scores_low_and_a_full_loop_scores_higher_than_kicks_alone() {
        let calm = sine(80.0, -30.0, SECONDS, RATE);
        let kicks = normalize(kick(124.0, SECONDS, RATE), -1.0);
        let full = test_loop(124.0, A_MINOR, SECONDS, RATE);
        let (calm, kicks, full) = (score(&calm), score(&kicks), score(&full));
        assert!(calm <= 2, "calm tone scored {calm}");
        assert!(calm < kicks && kicks < full, "{calm} < {kicks} < {full}");
    }

    #[test]
    fn adding_hats_raises_the_energy() {
        // Mixed in, not re-normalized: a peak normalize would turn the whole
        // loop down to make room for the hats and hide the change.
        let base = test_loop(124.0, A_MINOR, SECONDS, RATE);
        let hats = hats(124.0, 2, 0.5, -10.0, SECONDS, RATE);
        let with_hats = mix(&[&base, &hats]);
        let (before, after) = (features_of(&base), features_of(&with_hats));
        assert!(
            after.brightness > before.brightness,
            "{before:?} -> {after:?}"
        );
        assert!(after.loudness >= before.loudness, "{before:?} -> {after:?}");
        assert!(score(&with_hats) > score(&base), "{before:?} -> {after:?}");
    }

    #[test]
    fn more_hits_per_second_raise_the_density() {
        let slow = features_of(&kick(90.0, SECONDS, RATE));
        let fast = features_of(&kick(174.0, SECONDS, RATE));
        assert!(fast.density > slow.density, "{slow:?} -> {fast:?}");
    }

    #[test]
    fn hi_hats_over_a_full_mix_count_as_onsets() {
        // Sixteenth-note hats on the off sixteenths, under the loop's level: they barely
        // move the total energy, so only the high-frequency pass sees them.
        let base = test_loop(120.0, A_MINOR, SECONDS, RATE);
        let hats = hats(120.0, 4, 0.25, -12.0, SECONDS, RATE);
        let with_hats = mix(&[&base, &hats]);
        let (before, after) = (onset_count(&base, RATE), onset_count(&with_hats, RATE));
        // 12 s at 120 BPM is 96 sixteenths; the hats fill every one after the first.
        assert!(
            after >= 90 && after > before + 40,
            "{before} -> {after} onsets"
        );
        assert!(features_of(&with_hats).density > features_of(&base).density);
    }

    #[test]
    fn a_louder_copy_never_scores_lower() {
        let full = test_loop(124.0, A_MINOR, SECONDS, RATE);
        let quieter = scaled(&full, -12.0);
        assert!(score(&full) > score(&quieter));
        let (loud, quiet) = (features_of(&full), features_of(&quieter));
        assert!(loud.loudness > quiet.loudness);
        assert!(
            (loud.brightness - quiet.brightness).abs() < 1e-6,
            "level-independent"
        );
    }

    #[test]
    fn unusable_audio_gives_no_score() {
        let full = test_loop(124.0, A_MINOR, SECONDS, RATE);
        let short = &full[..RATE as usize];
        let mut broken = full.clone();
        broken[100] = f32::NAN;
        assert_eq!(
            energy_score(&vec![0.0; full.len()], RATE, -20.0),
            None,
            "silence"
        );
        assert_eq!(energy_score(short, RATE, -10.0), None, "too short");
        assert_eq!(energy_score(&broken, RATE, -10.0), None, "NaN sample");
        assert_eq!(
            energy_score(&full, RATE, f32::NEG_INFINITY),
            None,
            "no loudness"
        );
        assert_eq!(energy_score(&full, 0, -10.0), None, "no sample rate");
    }
}
