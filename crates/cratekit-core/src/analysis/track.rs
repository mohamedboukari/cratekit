//! Analyzing a whole track: everything the library stores about one file.

use std::path::Path;

use crate::CoreError;
use crate::analysis::energy::energy_score;
use crate::analysis::key_detect::{KeyDetector, KeyEstimate, ProfileKey};
use crate::analysis::loudness::{Loudness, measure_loudness_with};
use crate::analysis::tempo::{OnsetTempo, TempoDetector, TempoEstimate};
use crate::analysis::waveform::Waveform;
use crate::audio::{AudioBuffer, decode_file_with};
use crate::progress::{CancellationToken, ProgressSink};

/// Tempo and key are measured on at most this much of the middle of the
/// track (intros and outros are often beatless or in another key, and it is
/// faster). Loudness and energy use the whole track.
pub const SECTION_SECONDS: f64 = 90.0;

/// The share of the progress that decoding takes (the slowest step).
const DECODE_SHARE: f32 = 0.4;

/// Everything the analysis finds in one track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackAnalysis {
    /// Decoded length in seconds.
    pub duration_sec: f64,
    /// The file's sample rate.
    pub sample_rate: u32,
    /// The file's channel count.
    pub channels: u32,
    /// Tempo, confidence and first beat (in track time), if there is a beat.
    pub tempo: Option<TempoEstimate>,
    /// Key, confidence and chroma, if there are notes.
    pub key: Option<KeyEstimate>,
    /// Integrated loudness and true peak, unless the track is silent.
    pub loudness: Option<Loudness>,
    /// Energy 1 to 10, unless the track is silent or too short.
    pub energy: Option<u8>,
    /// The waveform columns of the whole track.
    pub waveform: Waveform,
}

/// Decodes `path` and analyzes it: loudness and energy over the whole track,
/// tempo (with the first beat) and key over its middle `SECTION_SECONDS`.
/// Reports progress to `progress` (0 to 1) and stops with
/// [`CoreError::Cancelled`] soon after `cancel` is cancelled.
///
/// A track with no beat, no notes or no sound is not an error: those fields
/// are `None`.
///
/// # Errors
///
/// The errors of [`decode_file`](crate::audio::decode_file), and
/// [`CoreError::Cancelled`].
pub fn analyze_file(
    path: &Path,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<TrackAnalysis, CoreError> {
    progress.progress(0.0);
    let decoding = |fraction: f32| progress.progress(DECODE_SHARE * fraction);
    let audio = decode_file_with(path, &decoding, cancel)?;
    progress.progress(DECODE_SHARE);
    cancel.check()?;
    analyze_buffer(&audio, progress, cancel)
}

/// [`analyze_file`] on audio already decoded. Progress runs from 0.4 to 1.
///
/// # Errors
///
/// [`CoreError::Cancelled`].
pub fn analyze_buffer(
    audio: &AudioBuffer,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<TrackAnalysis, CoreError> {
    let rate = audio.sample_rate();
    let measuring = |fraction: f32| progress.progress(DECODE_SHARE + 0.15 * fraction);
    let loudness = measure_loudness_with(&audio.channel_slices(), rate, &measuring, cancel)?;
    progress.progress(0.55);
    cancel.check()?;

    let mono = audio.to_mono();
    let energy = loudness.and_then(|loudness| energy_score(&mono, rate, loudness.integrated_lufs));
    progress.progress(0.65);
    cancel.check()?;

    let (start, section) = middle_section(&mono, rate);
    let tempo = OnsetTempo.detect_tempo(section, rate).map(|tempo| {
        // The detector places the first beat in the section; move it to the
        // start of the track (the same grid, one period or less from 0).
        let section_start = start as f64 / f64::from(rate);
        let period = 60.0 / f64::from(tempo.bpm);
        TempoEstimate {
            first_beat_sec: tempo
                .first_beat_sec
                .map(|first| (section_start + f64::from(first)).rem_euclid(period) as f32),
            ..tempo
        }
    });
    progress.progress(0.8);
    cancel.check()?;

    let key = ProfileKey.detect_key(section, rate);
    let waveform = Waveform::of(audio);
    progress.progress(1.0);
    Ok(TrackAnalysis {
        duration_sec: audio.duration_seconds(),
        sample_rate: rate,
        channels: u32::try_from(audio.channel_count()).unwrap_or(u32::MAX),
        tempo,
        key,
        loudness,
        energy,
        waveform,
    })
}

/// The middle `SECTION_SECONDS` of `samples` (all of them when shorter), and
/// where it starts.
fn middle_section(samples: &[f32], sample_rate: u32) -> (usize, &[f32]) {
    let length = (SECTION_SECONDS * f64::from(sample_rate)) as usize;
    if samples.len() <= length {
        return (0, samples);
    }
    let start = (samples.len() - length) / 2;
    (start, &samples[start..start + length])
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Instant;

    use super::*;
    use crate::analysis::key::{Key, Mode, PitchClass};
    use crate::progress::NoProgress;
    use crate::synth::{kick, sine, test_loop};
    use crate::testing::fixture;

    const RATE: u32 = 44_100;
    const A_MINOR: Key = Key {
        tonic: PitchClass::A,
        mode: Mode::Minor,
    };

    fn stereo(samples: Vec<f32>) -> AudioBuffer {
        AudioBuffer::new(RATE, vec![samples.clone(), samples]).expect("a valid buffer")
    }

    #[test]
    fn finds_tempo_key_loudness_and_energy_of_a_loop() {
        let audio = stereo(test_loop(124.0, A_MINOR, 30.0, RATE));
        let found =
            analyze_buffer(&audio, &NoProgress, &CancellationToken::new()).expect("analysis runs");
        let tempo = found.tempo.expect("a beat");
        assert!((tempo.bpm - 124.0).abs() <= 0.5, "{}", tempo.bpm);
        let first = tempo.first_beat_sec.expect("a first beat");
        let period = 60.0 / 124.0;
        assert!(
            first.min(period - first) <= 0.01,
            "the loop starts on a beat: {first}"
        );
        assert_eq!(found.key.map(|key| key.key), Some(A_MINOR));
        assert!(found.loudness.is_some() && found.energy.is_some());
        assert!((found.duration_sec - 30.0).abs() < 1e-6);
        assert_eq!((found.sample_rate, found.channels), (RATE, 2));
    }

    #[test]
    fn the_first_beat_is_in_track_time_when_the_section_starts_later() {
        // 100 s of kicks starting 0.2 s in: the analyzed section starts at
        // 5 s, but the first beat is still reported near the start.
        let mut samples = vec![0.0; (0.2 * f64::from(RATE)) as usize];
        samples.extend(kick(128.0, 100.0, RATE));
        let found = analyze_buffer(&stereo(samples), &NoProgress, &CancellationToken::new())
            .expect("analysis runs");
        let tempo = found.tempo.expect("a beat");
        let period = 60.0 / tempo.bpm;
        let expected = 0.2_f32.rem_euclid(period);
        let first = tempo.first_beat_sec.expect("a first beat");
        let error = (first - expected)
            .abs()
            .min(period - (first - expected).abs());
        assert!(error <= 0.01, "first beat {first}, expected {expected}");
    }

    #[test]
    fn a_tone_has_a_key_but_no_beat_and_silence_has_neither() {
        let tone = analyze_buffer(
            &stereo(sine(440.0, -12.0, 10.0, RATE)),
            &NoProgress,
            &CancellationToken::new(),
        )
        .expect("analysis runs");
        assert!(tone.tempo.is_none());
        assert!(tone.loudness.is_some());
        let silence = analyze_buffer(
            &stereo(vec![0.0; 10 * RATE as usize]),
            &NoProgress,
            &CancellationToken::new(),
        )
        .expect("analysis runs");
        assert!(silence.tempo.is_none() && silence.key.is_none());
        assert!(silence.loudness.is_none() && silence.energy.is_none());
    }

    #[test]
    fn reports_progress_up_to_one_and_stops_when_cancelled() {
        let seen = Mutex::new(Vec::new());
        let sink = |fraction: f32| seen.lock().expect("lock").push(fraction);
        analyze_file(&fixture("tone.wav"), &sink, &CancellationToken::new())
            .expect("analysis runs");
        let seen = seen.into_inner().expect("lock");
        assert_eq!(seen.first(), Some(&0.0));
        assert_eq!(seen.last(), Some(&1.0));
        assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "{seen:?}");

        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = analyze_file(&fixture("tone.wav"), &NoProgress, &cancel);
        assert!(matches!(result, Err(CoreError::Cancelled)), "{result:?}");
    }

    #[test]
    fn a_six_minute_track_analyzes_in_under_five_seconds() {
        // SPEC Module 2: under 5 s in a release build on a 2020 laptop CPU,
        // checked with `cargo nextest run --release`. Debug builds (CI) are
        // many times slower and their speed depends on the machine, so they
        // analyze a one-minute track with no time limit, as a smoke test.
        let seconds = if cfg!(debug_assertions) { 60.0 } else { 360.0 };
        let audio = stereo(test_loop(124.0, A_MINOR, seconds, RATE));
        let start = Instant::now();
        let found =
            analyze_buffer(&audio, &NoProgress, &CancellationToken::new()).expect("analysis runs");
        let elapsed = start.elapsed().as_secs_f64();
        assert!(found.tempo.is_some());
        if !cfg!(debug_assertions) {
            assert!(elapsed < 5.0, "took {elapsed:.2} s");
        }
    }
}
