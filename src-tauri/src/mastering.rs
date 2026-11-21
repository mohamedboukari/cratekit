//! Mastering commands: the presets the Master tab lists (#24) and the export
//! (#25). The live preview (#26) comes next.

use std::fs;

use cratekit_core::CoreError;
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
            lufs: -8.1,
            true_peak_db: -1.2,
        };
        let json = serde_json::to_value(result).expect("serializes");
        assert_eq!(json["truePeakDb"], serde_json::json!(-1.2_f32));
        assert_eq!(json["path"], "/m/x.wav");
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
    let file = folder.join(master_file_name(
        &detail.track.artist,
        &detail.track.title,
        settings.preset,
    ));
    let core = MasterSettings {
        preset: settings.preset,
        intensity: settings.intensity,
        target_lufs: settings.target_lufs,
    };
    let written = file.clone();
    let mastered = tauri::async_runtime::spawn_blocking(move || -> Result<_, AppError> {
        let audio = decode_file(&detail.path)?;
        let mastered = master(&audio, &core, &|_| {}, &CancellationToken::new())?;
        fs::create_dir_all(&folder).map_err(CoreError::from)?;
        write_wav(&written, &mastered.audio, format)?;
        Ok(mastered)
    })
    .await
    .map_err(|err| AppError {
        code: "internal",
        message: err.to_string(),
    })??;

    let settings_json = serde_json::to_string(&settings).map_err(|err| AppError {
        code: "internal",
        message: err.to_string(),
    })?;
    let preset = serde_json::to_value(settings.preset)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let (recorded, lufs) = (file.clone(), mastered.lufs);
    with_library(app, move |library| {
        library.store_master(id, &preset, &settings_json, &recorded, lufs)
    })
    .await?;
    Ok(MasterResultDto {
        path: file.to_string_lossy().into_owned(),
        lufs: mastered.lufs,
        true_peak_db: mastered.true_peak_dbtp,
    })
}
