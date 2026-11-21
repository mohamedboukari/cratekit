//! Mastering commands: the presets the Master tab lists (#24), the export
//! (#25, a job with progress and cancel since #28), the live preview for A/B
//! (#26), and reference matching in both (#27).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use cratekit_audio::MasterPreview;
use cratekit_core::audio::{WavFormat, decode_file};
use cratekit_core::library::TrackId;
use cratekit_core::mastering::master::{MasterSettings, master_file_name};
use cratekit_core::mastering::presets::PresetName;
use cratekit_core::mastering::reference::{MatchCurve, Spectrum, long_term_spectrum};
use cratekit_core::progress::{CancellationToken, NoProgress};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::AppError;
use crate::jobs::{Jobs, MasterRequest};
use crate::library::{LibraryState, with_library};
use crate::stems::output_dir;

/// A preset as the Master tab lists it, the UI's `MasterPreset`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct MasterPresetDto {
    name: PresetName,
    /// English; the UI words known presets itself.
    label: &'static str,
    /// The default loudness target, in LUFS.
    target_lufs: f32,
    /// One English line on the sound.
    description: &'static str,
}

/// Every preset, with its default target (from `PresetName::preset`).
fn presets() -> Vec<MasterPresetDto> {
    PresetName::ALL
        .iter()
        .map(|&name| MasterPresetDto {
            name,
            label: name.label(),
            target_lufs: name.preset().target_lufs,
            description: name.description(),
        })
        .collect()
}

/// The presets for the Master tab.
#[tauri::command]
pub(crate) fn master_presets() -> Result<Vec<MasterPresetDto>, AppError> {
    Ok(presets())
}

/// The UI's `MasterSettings`: what to master with, and the file format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct MasterSettingsDto {
    preset: PresetName,
    /// 0 to 1: how much the EQ and compressor colour the sound.
    intensity: f32,
    /// The loudness the master lands on, in LUFS.
    target_lufs: f32,
    /// The track whose sound to match (#27); none, or the track itself, for
    /// no matching.
    reference_track_id: Option<TrackId>,
    /// 16 (with TPDF dither) or 24.
    bit_depth: u8,
}

/// The UI's `MasterResult`: the file written and what the meter reads.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct MasterResultDto {
    path: String,
    lufs: f32,
    true_peak_db: f32,
    /// The target aimed at (clamped to -30 to -5 LUFS).
    target_lufs: f32,
    /// Whether the master landed within 0.5 LU of it; false when the limiter
    /// could not get there on this track (the closest pass is kept).
    target_reached: bool,
}

/// What the library records with a master: the settings actually used.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedSettings {
    preset: PresetName,
    intensity: f32,
    target_lufs: f32,
    bit_depth: u8,
    /// The reference matched, if any.
    reference_track_id: Option<TrackId>,
}

impl MasterSettingsDto {
    /// The reference to match for track `id`: none when it is the track
    /// itself (matching a track to itself changes nothing).
    fn reference_for(&self, id: TrackId) -> Option<TrackId> {
        self.reference_track_id.filter(|&reference| reference != id)
    }
}

/// Spectra cached at most; the cache is emptied past this (a session rarely
/// previews this many tracks, and each entry is small).
const MAX_SPECTRA: usize = 256;

/// Which version of a file a spectrum was measured from: a file rewritten
/// in place (a re-bounced mix) changes size or modification time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileStamp {
    fn of(path: &Path) -> Option<Self> {
        fs::metadata(path).ok().map(|meta| Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// A track's measured spectrum and the file version it came from.
#[derive(Debug, Clone)]
struct Measured {
    path: PathBuf,
    stamp: FileStamp,
    spectrum: Spectrum,
}

/// One track's cache slot: its own lock, so a track is measured once at a
/// time (a second preview waits and reuses the result) while other tracks
/// are not held up.
type Slot = Arc<Mutex<Option<Measured>>>;

/// The mastering commands' state: the tracks' long-term spectra for
/// reference matching in the preview (measured once per file version:
/// decoding it is most of the work), and which preview is the newest.
#[derive(Debug, Default)]
pub(crate) struct MasteringState {
    spectra: Mutex<HashMap<TrackId, Slot>>,
    preview_seq: AtomicU64,
}

/// Locks a mutex; a panic elsewhere leaves plain cached data usable.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Decodes `path` and measures its long-term spectrum.
fn measure_file(path: &Path) -> Result<Spectrum, AppError> {
    let audio = decode_file(path)?;
    Ok(long_term_spectrum(
        &audio,
        &NoProgress,
        &CancellationToken::new(),
    )?)
}

impl MasteringState {
    /// The long-term spectrum of track `id`, whose file is `path`: from the
    /// cache when that version of the file was measured, else measured and
    /// kept. Failures are not kept.
    fn spectrum(&self, id: TrackId, path: &Path) -> Result<Spectrum, AppError> {
        self.spectrum_with(id, path, measure_file)
    }

    /// [`Self::spectrum`] with the measuring step given (tests count it).
    fn spectrum_with(
        &self,
        id: TrackId,
        path: &Path,
        measure: impl Fn(&Path) -> Result<Spectrum, AppError>,
    ) -> Result<Spectrum, AppError> {
        let slot = {
            let mut spectra = locked(&self.spectra);
            if spectra.len() >= MAX_SPECTRA && !spectra.contains_key(&id) {
                spectra.clear();
            }
            Arc::clone(spectra.entry(id).or_default())
        };
        // Held while measuring: a second caller for this track waits here.
        let mut measured = locked(&slot);
        let stamp = FileStamp::of(path);
        if let Some(cached) = measured
            .as_ref()
            .filter(|cached| cached.path == path && Some(cached.stamp) == stamp)
        {
            return Ok(cached.spectrum);
        }
        let spectrum = measure(path)?;
        *measured = stamp.map(|stamp| Measured {
            path: path.to_path_buf(),
            stamp,
            spectrum,
        });
        Ok(spectrum)
    }

    /// A number for a new preview; [`Self::is_newest`] tells whether a later
    /// one started since.
    fn next_preview(&self) -> u64 {
        self.preview_seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn is_newest(&self, preview: u64) -> bool {
        self.preview_seq.load(Ordering::Relaxed) == preview
    }
}

/// The live mastered side for `settings` on a track measured at `lufs`,
/// matching a reference with `curve`: the preset at its intensity, makeup
/// toward the target, and the level match that takes it back (no makeup
/// when the track was not analysed).
fn preview_for(
    settings: &MasterSettingsDto,
    lufs: Option<f64>,
    curve: MatchCurve,
) -> MasterPreview {
    let core = MasterSettings {
        preset: settings.preset,
        intensity: settings.intensity,
        target_lufs: settings.target_lufs,
        reference: None,
    };
    let target = core.target();
    let makeup_db = lufs
        .filter(|value| value.is_finite())
        .map_or(0.0, |value| target - value as f32);
    MasterPreview {
        preset: settings
            .preset
            .preset()
            .at(settings.intensity)
            .with_reference(curve),
        makeup_db,
        level_match_db: -makeup_db,
        // The same rule as the export: the clipper only for loud targets.
        clip: core.clips(),
    }
}

/// Sets the live mastered side of A/B (#26) for track `id`, the one on the
/// deck (a call for another track is ignored): heard in the mastered mode,
/// at about the original's loudness while level match is on. Fast: nothing
/// is rendered, the engine masters as it plays. With a reference, the match
/// curve comes from both tracks' spectra, measured once and cached (the
/// first preview with a new reference takes a second or so); a preview that
/// finishes after a newer one started is dropped.
#[tauri::command]
pub(crate) async fn master_preview(
    app: AppHandle,
    id: TrackId,
    settings: MasterSettingsDto,
) -> Result<(), AppError> {
    let preview = app.state::<MasteringState>().next_preview();
    let reference = settings.reference_for(id);
    let (lufs, paths) = with_library(app.clone(), move |library| {
        let detail = library.track(id)?;
        let reference = reference
            .map(|reference| {
                Ok::<_, cratekit_core::CoreError>((reference, library.track(reference)?.path))
            })
            .transpose()?;
        Ok((
            detail.track.lufs,
            reference.map(|reference| (detail.path, reference)),
        ))
    })
    .await?;
    let curve = match paths {
        None => Ok(MatchCurve::FLAT),
        Some((track_path, (reference, reference_path))) => {
            let state = app.clone();
            tauri::async_runtime::spawn_blocking(move || -> Result<MatchCurve, AppError> {
                let cache = state.state::<MasteringState>();
                let track = cache.spectrum(id, &track_path)?;
                let theirs = cache.spectrum(reference, &reference_path)?;
                Ok(MatchCurve::between(&track, &theirs))
            })
            .await
            .map_err(|err| AppError {
                code: "internal",
                message: err.to_string(),
            })?
        }
    };
    if !app.state::<MasteringState>().is_newest(preview) {
        return Ok(());
    }
    // A reference that cannot be read: the B side still follows the other
    // settings (without the match), and the error says why.
    let applied = curve.as_ref().copied().unwrap_or(MatchCurve::FLAT);
    crate::playback::set_master_preview(app, id, preview_for(&settings, lufs, applied)).await?;
    curve.map(|_| ())
}

/// The WAV format for a bit depth: 24-bit, or 16-bit with TPDF dither.
fn wav_format(bit_depth: u8) -> Result<WavFormat, AppError> {
    match bit_depth {
        16 => Ok(WavFormat::Pcm16),
        24 => Ok(WavFormat::Pcm24),
        other => Err(AppError {
            code: "invalid_setting",
            message: format!("bit depth {other}: masters are 16 or 24-bit"),
        }),
    }
}

/// Masters a track and writes it into `<output folder>/Masters/` as
/// `<artist> - <title> (<preset> master).wav`, replacing an earlier export
/// of the same name; records it in the library (the track shows as
/// mastered) and returns the file, its loudness and its true peak. The
/// original file is only read. Runs as a job (kind `master`): progress
/// arrives as `job_progress`, and `job_cancel` stops it, which returns the
/// `cancelled` error.
#[tauri::command]
pub(crate) async fn master_export(
    app: AppHandle,
    id: TrackId,
    settings: MasterSettingsDto,
) -> Result<MasterResultDto, AppError> {
    let format = wav_format(settings.bit_depth)?;
    let reference = settings.reference_for(id);
    let (detail, output_setting, reference_path) = with_library(app.clone(), move |library| {
        let reference_path = reference
            .map(|reference| library.track(reference).map(|detail| detail.path))
            .transpose()?;
        Ok((
            library.track(id)?,
            library.settings()?.output_folder,
            reference_path,
        ))
    })
    .await?;
    let folder = output_dir(&app, &output_setting)?.join("Masters");
    // Made first: an unwritable folder fails at once, not after the render.
    fs::create_dir_all(&folder).map_err(|err| AppError {
        code: "io",
        message: format!("{}: {err}", folder.display()),
    })?;
    let file = folder.join(master_file_name(
        &detail.track.artist,
        &detail.track.title,
        settings.preset,
    ));
    // Hard rule 4: the original is only ever read.
    if file == detail.path {
        return Err(AppError {
            code: "io",
            message: format!("{} is the original file", file.display()),
        });
    }
    let core = MasterSettings {
        preset: settings.preset,
        intensity: settings.intensity,
        target_lufs: settings.target_lufs,
        // Measured from the reference's file inside the job.
        reference: None,
    };
    let recorded = RecordedSettings {
        preset: settings.preset,
        intensity: settings.intensity.clamp(0.0, 1.0),
        target_lufs: core.target(),
        bit_depth: settings.bit_depth,
        reference_track_id: reference,
    };
    let request = MasterRequest {
        track_id: id,
        source: detail.path,
        file,
        reference: reference_path,
        settings: core,
        format,
        recorded: serde_json::to_string(&recorded).map_err(|err| AppError {
            code: "internal",
            message: err.to_string(),
        })?,
    };
    // The audio stays on the worker thread: only the numbers come back.
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<LibraryState>();
        let mastered = app
            .state::<Jobs>()
            .run_master(&request, state.library()?, &app)?;
        Ok(MasterResultDto {
            path: request.file.to_string_lossy().into_owned(),
            lufs: mastered.lufs,
            true_peak_db: mastered.true_peak_dbtp,
            target_lufs: mastered.target_lufs,
            target_reached: mastered.target_reached,
        })
    })
    .await
    .map_err(|err| AppError {
        code: "internal",
        message: err.to_string(),
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_settings_the_ui_sends() {
        let settings: MasterSettingsDto = serde_json::from_value(serde_json::json!({
            "preset": "lo-fi",
            "intensity": 0.6,
            "targetLufs": -12.0,
            "referenceTrackId": null,
            "bitDepth": 16
        }))
        .expect("deserializes");
        assert_eq!(settings.preset, PresetName::LoFi);
        assert_eq!(settings.target_lufs, -12.0);
        assert_eq!(settings.bit_depth, 16);
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/cratekit-core/tests/fixtures")
            .join(name)
    }

    fn with_reference(reference: Option<TrackId>) -> MasterSettingsDto {
        MasterSettingsDto {
            preset: PresetName::Club,
            intensity: 0.6,
            target_lufs: -8.0,
            reference_track_id: reference,
            bit_depth: 24,
        }
    }

    #[test]
    fn a_track_is_never_its_own_reference() {
        assert_eq!(with_reference(Some(4)).reference_for(3), Some(4));
        assert_eq!(with_reference(Some(3)).reference_for(3), None);
        assert_eq!(with_reference(None).reference_for(3), None);
    }

    #[test]
    fn the_preview_carries_the_reference_curve() {
        let curve = MatchCurve::new(&[2.0; cratekit_core::mastering::reference::BANDS]);
        let preview = preview_for(&with_reference(Some(4)), Some(-14.0), curve);
        assert_eq!(preview.preset.reference, curve);
        assert_eq!(preview.preset.intensity, 0.6);
        let flat = preview_for(&with_reference(None), Some(-14.0), MatchCurve::FLAT);
        assert!(flat.preset.reference.is_flat());
    }

    #[test]
    fn the_recorded_settings_name_the_reference() {
        let recorded = RecordedSettings {
            preset: PresetName::Pop,
            intensity: 0.5,
            target_lufs: -9.0,
            bit_depth: 16,
            reference_track_id: Some(7),
        };
        let json = serde_json::to_value(recorded).expect("serializes");
        assert_eq!(json["referenceTrackId"], 7);
        assert_eq!(json["preset"], "pop");
    }

    /// A measuring step that counts its calls and takes `delay`.
    fn counting(
        calls: &std::sync::atomic::AtomicUsize,
        delay: std::time::Duration,
    ) -> impl Fn(&Path) -> Result<Spectrum, AppError> + '_ {
        move |path| {
            calls.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(delay);
            measure_file(path)
        }
    }

    #[test]
    fn spectra_are_measured_once_per_file_version() {
        let dir = std::env::temp_dir().join(format!("cratekit-spectra-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("track.wav");
        fs::copy(fixture("tone.wav"), &file).unwrap();
        let (state, calls) = (
            MasteringState::default(),
            std::sync::atomic::AtomicUsize::new(0),
        );
        let measure = counting(&calls, std::time::Duration::ZERO);

        let first = state.spectrum_with(1, &file, &measure).unwrap();
        assert_eq!(state.spectrum_with(1, &file, &measure).unwrap(), first);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "the second call comes from the cache"
        );

        // Re-bounced under the same name: measured again.
        fs::copy(fixture("mono-48k.wav"), &file).unwrap();
        let rebounced = state.spectrum_with(1, &file, &measure).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_ne!(rebounced, first);

        // A failure is not kept: the next call tries again.
        let missing = dir.join("missing.wav");
        assert_eq!(
            state
                .spectrum_with(2, &missing, &measure)
                .expect_err("fails")
                .code,
            "io"
        );
        assert!(state.spectrum_with(2, &missing, &measure).is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 4);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn previews_at_once_measure_a_track_only_once() {
        let (state, calls) = (
            MasteringState::default(),
            std::sync::atomic::AtomicUsize::new(0),
        );
        let measure = counting(&calls, std::time::Duration::from_millis(100));
        let path = fixture("tone.wav");
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| state.spectrum_with(1, &path, &measure).unwrap());
            }
        });
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn only_the_newest_preview_is_applied() {
        let state = MasteringState::default();
        let slow = state.next_preview();
        assert!(state.is_newest(slow));
        let fast = state.next_preview();
        assert!(
            !state.is_newest(slow),
            "an older preview finishing late is dropped"
        );
        assert!(state.is_newest(fast));
    }

    #[test]
    fn the_preview_aims_at_the_target_and_level_matches_back() {
        let settings = |preset, intensity, target_lufs| MasterSettingsDto {
            preset,
            intensity,
            target_lufs,
            reference_track_id: None,
            bit_depth: 24,
        };
        let club = preview_for(
            &settings(PresetName::Club, 0.6, -8.0),
            Some(-14.0),
            MatchCurve::FLAT,
        );
        assert!((club.makeup_db - 6.0).abs() < 1e-5);
        assert!((club.level_match_db + 6.0).abs() < 1e-5);
        assert_eq!(club.preset, PresetName::Club.preset().at(0.6));
        // A target out of range is clamped as in the export.
        let loud = preview_for(
            &settings(PresetName::Club, 1.0, 0.0),
            Some(-14.0),
            MatchCurve::FLAT,
        );
        assert!((loud.makeup_db - 9.0).abs() < 1e-5);
        // Not analysed yet: no makeup, nothing to match.
        let unknown = preview_for(
            &settings(PresetName::Chill, 1.0, -12.0),
            None,
            MatchCurve::FLAT,
        );
        assert_eq!((unknown.makeup_db, unknown.level_match_db), (0.0, 0.0));
        // The clipper only for loud targets, as in the export.
        assert!(club.clip);
        assert!(!unknown.clip);
    }

    #[test]
    fn only_16_and_24_bit_masters_are_written() {
        assert_eq!(wav_format(16).ok(), Some(WavFormat::Pcm16));
        assert_eq!(wav_format(24).ok(), Some(WavFormat::Pcm24));
        assert_eq!(
            wav_format(32).map_err(|err| err.code),
            Err("invalid_setting")
        );
    }

    #[test]
    fn the_result_serializes_as_the_ui_reads_it() {
        let result = MasterResultDto {
            path: "/m/x.wav".into(),
            lufs: -9.2,
            true_peak_db: -1.2,
            target_lufs: -8.0,
            target_reached: false,
        };
        let json = serde_json::to_value(result).expect("serializes");
        assert_eq!(json["truePeakDb"], serde_json::json!(-1.2_f32));
        assert_eq!(json["path"], "/m/x.wav");
        assert_eq!(json["targetLufs"], serde_json::json!(-8.0_f32));
        assert_eq!(json["targetReached"], false);
    }

    #[test]
    fn lists_every_preset_as_the_ui_reads_it() {
        let json = serde_json::to_value(presets()).expect("serializes");
        assert_eq!(json.as_array().map(Vec::len), Some(5));
        assert_eq!(
            json[0],
            serde_json::json!({
                "name": "club",
                "label": "Club",
                "targetLufs": -8.0,
                "description": "Punchy low end, bright top"
            })
        );
        assert_eq!(json[3]["name"], "lo-fi");
        assert_eq!(json[4]["targetLufs"], -12.0);
    }
}
