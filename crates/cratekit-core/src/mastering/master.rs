//! The whole mastering chain (SPEC Module 3, #25): the preset's EQ and
//! compressor at its intensity, then the makeup gain that lands on the
//! loudness target, found by measuring and correcting (the limiter makes
//! loudness move less than the gain, so one guess is not enough), then the
//! true-peak limiter, and a last check with the meter.
//!
//! Intensity never moves the target: the master lands on the loudness chosen
//! (`docs/ARCHITECTURE.md`, Mastering chain).

use crate::CoreError;
use crate::analysis::loudness::{Loudness, measure_loudness};
use crate::audio::AudioBuffer;
use crate::files::safe_file_name;
use crate::progress::{CancellationToken, ProgressSink};

use super::limiter::{LimiterSettings, limit};
use super::presets::{PresetChain, PresetName};

/// Close enough to the target, in LU (the SPEC accepts 0.5).
const TOLERANCE_LU: f32 = 0.3;
/// At most this many gain, limit and measure passes.
const MAX_PASSES: usize = 4;
/// The most makeup gain applied, in dB (a near-silent track is not blown up
/// without end).
const MAX_GAIN_DB: f32 = 40.0;
/// Loudness targets accepted, in LUFS: from very quiet to as loud as a
/// limiter at -1 dBTP can sensibly go.
const TARGET_RANGE: (f32, f32) = (-30.0, -5.0);
/// The share of the progress for the EQ and compressor pass.
const COLOUR_SHARE: f32 = 0.3;
/// Headroom when the last check has to trim, in dB.
const TRIM_MARGIN_DB: f32 = 0.05;

/// What to master with: the UI's `MasterSettings` without the export
/// format.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MasterSettings {
    /// The preset.
    pub preset: PresetName,
    /// 0 to 1: how much the EQ and compressor colour the sound.
    pub intensity: f32,
    /// The loudness the master lands on, in LUFS (clamped to -30 to -5).
    pub target_lufs: f32,
}

/// A mastered track and its measurements.
#[derive(Debug, Clone, PartialEq)]
pub struct Mastered {
    /// The mastered audio, same rate and channels as the source.
    pub audio: AudioBuffer,
    /// Its integrated loudness, in LUFS.
    pub lufs: f32,
    /// Its true peak, in dBTP (at or under -1.0).
    pub true_peak_dbtp: f32,
}

/// Measures planar channels, or says the track is silent.
fn loudness(channels: &[Vec<f32>], sample_rate: u32) -> Result<Loudness, CoreError> {
    let slices: Vec<&[f32]> = channels.iter().map(Vec::as_slice).collect();
    measure_loudness(&slices, sample_rate).ok_or(CoreError::InvalidAudio(
        "nothing to master: the track is silent",
    ))
}

/// `channels` times a gain in dB, as new channels.
fn with_gain(channels: &[Vec<f32>], gain_db: f32) -> Vec<Vec<f32>> {
    let gain = 10_f32.powf(gain_db / 20.0);
    channels
        .iter()
        .map(|channel| channel.iter().map(|sample| sample * gain).collect())
        .collect()
}

/// Masters `audio`: the preset at its intensity, the loudness target within
/// 0.3 LU when the limiter allows it (the closest pass otherwise), and a true
/// peak at or under -1.0 dBTP. Progress goes from 0 to 1; cancellation is
/// checked about every second of audio and between passes.
///
/// # Errors
///
/// [`CoreError::InvalidAudio`] when the track is silent, or
/// [`CoreError::Cancelled`].
pub fn master(
    audio: &AudioBuffer,
    settings: &MasterSettings,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<Mastered, CoreError> {
    let rate = audio.sample_rate();
    let frames = audio.frames();
    let target = if settings.target_lufs.is_finite() {
        settings.target_lufs.clamp(TARGET_RANGE.0, TARGET_RANGE.1)
    } else {
        settings.preset.preset().target_lufs
    };

    // The preset's EQ and compressor, a second of audio at a time.
    let mut coloured: Vec<Vec<f32>> = audio.channels().to_vec();
    let preset = settings.preset.preset().at(settings.intensity);
    let mut chain = PresetChain::new(rate, coloured.len(), &preset);
    let block = rate.max(1) as usize;
    let mut start = 0;
    while start < frames {
        cancel.check()?;
        let end = (start + block).min(frames);
        let mut slices: Vec<&mut [f32]> = coloured.iter_mut().map(|c| &mut c[start..end]).collect();
        chain.process(&mut slices);
        start = end;
        progress.progress(COLOUR_SHARE * start as f32 / frames.max(1) as f32);
    }

    // Makeup gain, limit, measure; correct the gain by the miss and again.
    let before = loudness(&coloured, rate)?.integrated_lufs;
    let mut gain_db = (target - before).min(MAX_GAIN_DB);
    let mut best: Option<(f32, Vec<Vec<f32>>, Loudness)> = None;
    for pass in 0..MAX_PASSES {
        cancel.check()?;
        let mut out = with_gain(&coloured, gain_db);
        limit(&mut out, rate, LimiterSettings::default());
        let measured = loudness(&out, rate)?;
        let miss = target - measured.integrated_lufs;
        progress
            .progress(COLOUR_SHARE + (1.0 - COLOUR_SHARE) * (pass + 1) as f32 / MAX_PASSES as f32);
        let closer = best
            .as_ref()
            .is_none_or(|(error, _, _)| miss.abs() < *error);
        if closer {
            best = Some((miss.abs(), out, measured));
        }
        if miss.abs() <= TOLERANCE_LU {
            break;
        }
        gain_db = (gain_db + miss).min(MAX_GAIN_DB);
    }
    let Some((_, mut out, mut measured)) = best else {
        return Err(CoreError::InvalidAudio(
            "nothing to master: the track is silent",
        ));
    };

    // The last check: the meter must read -1.0 dBTP or under.
    let ceiling = LimiterSettings::default().ceiling_dbtp;
    if measured.true_peak_dbtp > ceiling {
        out = with_gain(&out, ceiling - measured.true_peak_dbtp - TRIM_MARGIN_DB);
        measured = loudness(&out, rate)?;
    }
    progress.progress(1.0);
    Ok(Mastered {
        audio: AudioBuffer::new(rate, out)?,
        lufs: measured.integrated_lufs,
        true_peak_dbtp: measured.true_peak_dbtp,
    })
}

/// The file name of a master: `<artist> - <title> (<preset> master).wav`,
/// the title alone without an artist, safe on every system.
#[must_use]
pub fn master_file_name(artist: &str, title: &str, preset: PresetName) -> String {
    let label = if artist.trim().is_empty() {
        title.trim().to_owned()
    } else {
        format!("{} - {}", artist.trim(), title.trim())
    };
    let name = format!("{label} ({} master)", preset.label());
    format!("{}.wav", safe_file_name(&name, "master"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::key::{Key, Mode, PitchClass};
    use crate::synth;

    const RATE: u32 = 48_000;

    fn stereo_loop(peak_db: f32) -> AudioBuffer {
        let a_minor = Key {
            tonic: PitchClass::A,
            mode: Mode::Minor,
        };
        let mix = synth::normalize(synth::test_loop(126.0, a_minor, 4.0, RATE), peak_db);
        let other = mix.iter().map(|s| s * 0.9).collect();
        AudioBuffer::new(RATE, vec![mix, other]).unwrap()
    }

    fn settings(preset: PresetName, intensity: f32, target_lufs: f32) -> MasterSettings {
        MasterSettings {
            preset,
            intensity,
            target_lufs,
        }
    }

    fn run(audio: &AudioBuffer, settings: &MasterSettings) -> Mastered {
        master(audio, settings, &|_| {}, &CancellationToken::new()).unwrap()
    }

    /// The SPEC acceptance, measured again on the output with the meter.
    fn assert_acceptance(mastered: &Mastered, target: f32, what: &str) {
        let measured = loudness(mastered.audio.channels(), RATE).unwrap();
        assert!(
            (measured.integrated_lufs - target).abs() <= 0.5,
            "{what}: {} LUFS for {target}",
            measured.integrated_lufs
        );
        assert!(
            measured.true_peak_dbtp <= -1.0,
            "{what}: {} dBTP",
            measured.true_peak_dbtp
        );
        assert!(
            (mastered.lufs - measured.integrated_lufs).abs() < 1e-3,
            "reports what it measured"
        );
        assert!((mastered.true_peak_dbtp - measured.true_peak_dbtp).abs() < 1e-3);
    }

    #[test]
    fn lands_on_the_target_under_the_ceiling() {
        let source = stereo_loop(-1.0);
        for (preset, target) in [
            (PresetName::Club, -8.0),
            (PresetName::HipHop, -9.0),
            (PresetName::Chill, -14.0),
        ] {
            let mastered = run(&source, &settings(preset, 1.0, target));
            assert_acceptance(&mastered, target, &format!("{preset:?}"));
            assert_eq!(mastered.audio.channel_count(), 2);
            assert_eq!(mastered.audio.frames(), source.frames());
        }
    }

    #[test]
    fn raises_a_quiet_track_and_lowers_a_hot_one() {
        let quiet = stereo_loop(-30.0);
        assert_acceptance(
            &run(&quiet, &settings(PresetName::Pop, 1.0, -9.0)),
            -9.0,
            "quiet",
        );
        let hot = stereo_loop(6.0);
        assert_acceptance(
            &run(&hot, &settings(PresetName::LoFi, 1.0, -14.0)),
            -14.0,
            "hot",
        );
    }

    #[test]
    fn intensity_never_moves_the_loudness() {
        let source = stereo_loop(-1.0);
        for intensity in [0.0, 0.6] {
            let mastered = run(&source, &settings(PresetName::Club, intensity, -10.0));
            assert_acceptance(&mastered, -10.0, &format!("intensity {intensity}"));
        }
    }

    #[test]
    fn an_unreachable_target_stops_at_the_closest_pass_under_the_ceiling() {
        let source = stereo_loop(-1.0);
        // Clamped to -5 LUFS, still very loud for this loop: it does what it can.
        let mastered = run(&source, &settings(PresetName::Club, 1.0, 0.0));
        assert!(
            mastered.true_peak_dbtp <= -1.0,
            "{}",
            mastered.true_peak_dbtp
        );
        assert!(mastered.lufs > -9.0, "pushed hard: {}", mastered.lufs);
    }

    #[test]
    fn reports_progress_and_can_be_cancelled() {
        let source = stereo_loop(-1.0);
        let seen = std::sync::Mutex::new(Vec::new());
        let record = |fraction: f32| seen.lock().unwrap().push(fraction);
        master(
            &source,
            &settings(PresetName::Club, 1.0, -9.0),
            &record,
            &CancellationToken::new(),
        )
        .unwrap();
        let seen = seen.into_inner().unwrap();
        assert!(seen.windows(2).all(|w| w[1] >= w[0]), "only forward");
        assert_eq!(seen.last(), Some(&1.0));

        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = master(
            &source,
            &settings(PresetName::Club, 1.0, -9.0),
            &|_| {},
            &cancel,
        );
        assert!(matches!(result, Err(CoreError::Cancelled)));
    }

    #[test]
    fn a_silent_track_is_an_error() {
        let silence = AudioBuffer::new(RATE, vec![vec![0.0; RATE as usize * 2]; 2]).unwrap();
        let result = master(
            &silence,
            &settings(PresetName::Club, 1.0, -9.0),
            &|_| {},
            &CancellationToken::new(),
        );
        assert!(matches!(result, Err(CoreError::InvalidAudio(_))));
    }

    #[test]
    fn master_files_are_named_after_the_track_and_preset() {
        assert_eq!(
            master_file_name("DJ Test", "Night Drive", PresetName::Club),
            "DJ Test - Night Drive (Club master).wav"
        );
        assert_eq!(
            master_file_name("  ", "Solo", PresetName::LoFi),
            "Solo (Lo-fi master).wav"
        );
        assert_eq!(
            master_file_name("A/B", "C:D", PresetName::Pop),
            "A_B - C_D (Pop master).wav"
        );
    }
}
