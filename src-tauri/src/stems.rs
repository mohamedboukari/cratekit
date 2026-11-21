//! Stems commands (#33, #35, #32): splitting tracks (a job on the queue), what
//! the Stems tab shows, exporting the stems, and the backend they split on.
//! Stems are written to `stems/<track id>/` in the app data folder as 16-bit
//! WAV files and recorded in the library; an export copies them into the
//! output folder.

use std::path::PathBuf;

use cratekit_core::CoreError;
use cratekit_core::analysis::waveform::{OVERVIEW_COLUMNS, WaveformView};
use cratekit_core::library::TrackId;
use cratekit_core::settings::StemsBackend;
use cratekit_core::stems::StemSeparator;
use cratekit_core::stems::cache::export_folder;
use cratekit_core::stems::htdemucs::HtDemucs;
use cratekit_core::stems::models::HTDEMUCS;
use cratekit_core::stems::runtime::{LIBRARY_FILE, load};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::AppError;
use crate::jobs::{JobKind, Jobs};
use crate::library::{LibraryState, with_library};
use crate::models::models_dir;

/// Development builds have no bundled library: this variable points at one
/// (the path `.github/scripts/fetch-onnxruntime.sh` prints).
const LIBRARY_VARIABLE: &str = "CRATEKIT_ONNXRUNTIME";

/// The folder the stems are written into, one sub-folder per track.
pub(crate) fn stems_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, AppError> {
    Ok(app.path().app_data_dir()?.join("stems"))
}

/// Where exports go: the Output folder setting, or `<Music>/Cratekit` when it
/// is empty (decided 2026-11-21).
pub(crate) fn output_dir<R: Runtime>(
    app: &AppHandle<R>,
    setting: &str,
) -> Result<PathBuf, AppError> {
    if !setting.trim().is_empty() {
        return Ok(PathBuf::from(setting));
    }
    Ok(app.path().audio_dir()?.join("Cratekit"))
}

/// ONNX Runtime's library: `CRATEKIT_ONNXRUNTIME` when set, else the copy the
/// installer bundles in the resources.
fn library_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, AppError> {
    if let Some(path) = std::env::var_os(LIBRARY_VARIABLE) {
        return Ok(PathBuf::from(path));
    }
    Ok(app
        .path()
        .resource_dir()?
        .join("onnxruntime")
        .join(LIBRARY_FILE))
}

/// Why the model cannot be used yet.
fn model_missing() -> AppError {
    AppError {
        code: "model_missing",
        message: "the stems model is not downloaded".to_owned(),
    }
}

/// The backend that splits stems, as the UI's `StemsBackendStatus.active`.
/// Only the CPU in v0.1 (decided 2026-11-21, #32): CoreML joins when an ONNX
/// Runtime release is fast on the CPU and runs htdemucs on CoreML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub(crate) enum StemsActiveBackend {
    Cpu,
}

/// Why stems run on the CPU, as a code the UI words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub(crate) enum StemsCpuReason {
    /// "CPU only" is chosen in Settings.
    CpuChosen,
    /// This version splits on the CPU on every platform.
    GpuUnsupported,
}

/// The UI's `StemsBackendStatus`: the active backend and, when it is the
/// CPU, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub(crate) struct StemsBackendStatusDto {
    active: StemsActiveBackend,
    reason: Option<StemsCpuReason>,
}

/// Which backend a split uses with this preference. Both preferences give the
/// CPU in v0.1; only the reason differs.
fn backend_status(preference: StemsBackend) -> StemsBackendStatusDto {
    let reason = match preference {
        StemsBackend::Cpu => StemsCpuReason::CpuChosen,
        StemsBackend::Auto => StemsCpuReason::GpuUnsupported,
    };
    StemsBackendStatusDto {
        active: StemsActiveBackend::Cpu,
        reason: Some(reason),
    }
}

/// The backend stems split on, for the status bar, the Stems tab and Settings.
#[tauri::command]
pub(crate) async fn stems_backend_status(
    app: AppHandle,
) -> Result<StemsBackendStatusDto, AppError> {
    let settings = with_library(app, |library| library.settings()).await?;
    Ok(backend_status(settings.stems_backend))
}

/// The separator for a stems job: ONNX Runtime loaded (once per process) and
/// the downloaded model.
pub(crate) fn separator<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Box<dyn StemSeparator>, AppError> {
    let runtime = load(&library_path(app)?).map_err(|err| AppError {
        code: "stems_unavailable",
        message: err.to_string(),
    })?;
    let models = models_dir(app)?;
    if !HTDEMUCS.is_downloaded(&models) {
        return Err(model_missing());
    }
    let separator = HtDemucs::new(runtime, &HTDEMUCS.dir(&models))?;
    Ok(Box::new(separator))
}

/// Queues splitting a track into stems; returns the job id. Rejected right
/// away with `model_missing` when the model is not downloaded.
#[tauri::command]
pub(crate) fn stems_split(
    app: AppHandle,
    jobs: tauri::State<'_, Jobs>,
    library: tauri::State<'_, LibraryState>,
    id: TrackId,
) -> Result<String, AppError> {
    library.library()?;
    if !HTDEMUCS.is_downloaded(&models_dir(&app)?) {
        return Err(model_missing());
    }
    Ok(jobs.queue(JobKind::Stems, vec![id], &app))
}

/// The four stems' waveforms, as the UI's `StemsInfo.waveforms`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub(crate) struct StemWaveformViews {
    vocals: WaveformView,
    drums: WaveformView,
    bass: WaveformView,
    other: WaveformView,
}

/// A split track, as the UI's `StemsInfo`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct StemsInfoDto {
    split_sec: f64,
    waveforms: StemWaveformViews,
    instrumental: WaveformView,
}

/// What the Stems tab shows for a track: `null` when it was not split.
#[tauri::command]
pub(crate) async fn stems_info(
    app: AppHandle,
    id: TrackId,
) -> Result<Option<StemsInfoDto>, AppError> {
    with_library(app, move |library| {
        Ok(library.stems(id)?.and_then(|stored| {
            let waveforms = stored.waveforms?;
            let view = |waveform: &cratekit_core::analysis::waveform::Waveform| {
                waveform.overview(OVERVIEW_COLUMNS)
            };
            Some(StemsInfoDto {
                split_sec: stored.split_sec.unwrap_or(0.0),
                waveforms: StemWaveformViews {
                    vocals: view(&waveforms.vocals),
                    drums: view(&waveforms.drums),
                    bass: view(&waveforms.bass),
                    other: view(&waveforms.other),
                },
                instrumental: view(&waveforms.instrumental),
            })
        }))
    })
    .await
}

/// Copies a split track's four stems into `<output folder>/Stems/<artist> -
/// <title>/` as `<artist> - <title> (drums).wav` and so on (the title alone
/// without an artist), replacing an earlier export; returns the paths. `not_found` when the track was not split.
#[tauri::command]
pub(crate) async fn stems_export(app: AppHandle, id: TrackId) -> Result<Vec<String>, AppError> {
    let (stored, artist, title, setting) = with_library(app.clone(), move |library| {
        let stored = library
            .stems(id)?
            .ok_or(CoreError::NotFound { what: "stems", id })?;
        let track = library.track(id)?.track;
        let setting = library.settings()?.output_folder;
        Ok((stored, track.artist, track.title, setting))
    })
    .await?;
    let dest = output_dir(&app, &setting)?.join("Stems");
    tauri::async_runtime::spawn_blocking(move || {
        export_folder(&stored.dir, &dest, &artist, &title)
            .map(|paths| {
                paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect()
            })
            .map_err(AppError::from)
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
    fn every_preference_splits_on_the_cpu_with_its_reason() {
        let chosen = backend_status(StemsBackend::Cpu);
        assert_eq!(chosen.active, StemsActiveBackend::Cpu);
        assert_eq!(chosen.reason, Some(StemsCpuReason::CpuChosen));

        let auto = backend_status(StemsBackend::Auto);
        assert_eq!(auto.active, StemsActiveBackend::Cpu);
        assert_eq!(auto.reason, Some(StemsCpuReason::GpuUnsupported));
    }

    #[test]
    fn the_status_serializes_as_the_ui_reads_it() {
        let json = serde_json::to_value(backend_status(StemsBackend::Auto)).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({ "active": "cpu", "reason": "gpu_unsupported" })
        );
    }
}
