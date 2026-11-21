//! Mastering commands: the presets the Master tab lists (#24), the export
//! (#25) and the live preview for A/B (#26).

use std::fs;

use cratekit_audio::MasterPreview;
use cratekit_core::audio::{WavFormat, decode_file, write_wav};
use cratekit_core::library::TrackId;
use cratekit_core::mastering::master::{MasterSettings, master, master_file_name};
use cratekit_core::mastering::presets::PresetName;
use cratekit_core::progress::CancellationToken;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::AppError;
use crate::library::with_library;
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
    /// Reference matching comes with #27; recorded with the master until then.
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
}

/// The live mastered side for `settings` on a track measured at `lufs`: the
/// preset at its intensity, makeup toward the target, and the level match
/// that takes it back (no makeup when the track was not analysed).
fn preview_for(settings: &MasterSettingsDto, lufs: Option<f64>) -> MasterPreview {
    let target = MasterSettings {
        preset: settings.preset,
        intensity: settings.intensity,
        target_lufs: settings.target_lufs,
    }
    .target();
    let makeup_db = lufs
        .filter(|value| value.is_finite())
        .map_or(0.0, |value| target - value as f32);
    MasterPreview {
        preset: settings.preset.preset().at(settings.intensity),
        makeup_db,
        level_match_db: -makeup_db,
    }
}

/// Sets the live mastered side of A/B (#26) for track `id`, the one on the
/// deck (a call for another track is ignored): heard in the mastered mode,
/// at about the original's loudness while level match is on. Fast: nothing
/// is rendered, the engine masters as it plays.
#[tauri::command]
pub(crate) async fn master_preview(
    app: AppHandle,
    id: TrackId,
    settings: MasterSettingsDto,
) -> Result<(), AppError> {
    let lufs = with_library(app.clone(), move |library| {
        Ok(library.track(id)?.track.lufs)
    })
    .await?;
    crate::playback::set_master_preview(app, id, preview_for(&settings, lufs)).await
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
/// original file is only read.
#[tauri::command]
pub(crate) async fn master_export(
    app: AppHandle,
    id: TrackId,
    settings: MasterSettingsDto,
) -> Result<MasterResultDto, AppError> {
    let format = wav_format(settings.bit_depth)?;
    let (detail, output_setting) = with_library(app.clone(), move |library| {
        Ok((library.track(id)?, library.settings()?.output_folder))
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
    };
    let written = file.clone();
    let mastered = tauri::async_runtime::spawn_blocking(move || -> Result<_, AppError> {
        let audio = decode_file(&detail.path)?;
        let mastered = master(audio, &core, &|_| {}, &CancellationToken::new())?;
        write_wav(&written, &mastered.audio, format).map_err(|err| AppError {
            code: "io",
            message: format!("{}: {err}", written.display()),
        })?;
        Ok(mastered)
    })
    .await
    .map_err(|err| AppError {
        code: "internal",
        message: err.to_string(),
    })??;

    let recorded = RecordedSettings {
        preset: settings.preset,
        intensity: settings.intensity.clamp(0.0, 1.0),
        target_lufs: mastered.target_lufs,
        bit_depth: settings.bit_depth,
    };
    let settings_json = serde_json::to_string(&recorded).map_err(|err| AppError {
        code: "internal",
        message: err.to_string(),
    })?;
    let (path, lufs) = (file.clone(), mastered.lufs);
    with_library(app, move |library| {
        library.store_master(id, settings.preset.id(), &settings_json, &path, lufs)
    })
    .await?;
    Ok(MasterResultDto {
        path: file.to_string_lossy().into_owned(),
        lufs: mastered.lufs,
        true_peak_db: mastered.true_peak_dbtp,
        target_lufs: mastered.target_lufs,
        target_reached: mastered.target_reached,
    })
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

    #[test]
    fn the_preview_aims_at_the_target_and_level_matches_back() {
        let settings = |preset, intensity, target_lufs| MasterSettingsDto {
            preset,
            intensity,
            target_lufs,
            reference_track_id: None,
            bit_depth: 24,
        };
        let club = preview_for(&settings(PresetName::Club, 0.6, -8.0), Some(-14.0));
        assert!((club.makeup_db - 6.0).abs() < 1e-5);
        assert!((club.level_match_db + 6.0).abs() < 1e-5);
        assert_eq!(club.preset, PresetName::Club.preset().at(0.6));
        // A target out of range is clamped as in the export.
        let loud = preview_for(&settings(PresetName::Club, 1.0, 0.0), Some(-14.0));
        assert!((loud.makeup_db - 9.0).abs() < 1e-5);
        // Not analysed yet: no makeup, nothing to match.
        let unknown = preview_for(&settings(PresetName::Chill, 1.0, -12.0), None);
        assert_eq!((unknown.makeup_db, unknown.level_match_db), (0.0, 0.0));
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
