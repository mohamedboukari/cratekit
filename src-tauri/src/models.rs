//! Model commands (#30): the stems model's status, its download (a job, the
//! only network call in the app) and its removal. The models live in
//! `models/` in the app data folder.

use std::path::PathBuf;

use cratekit_core::stems::models::{HTDEMUCS, Model};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

use crate::AppError;
use crate::jobs::Jobs;

/// Every model the app can download.
const MODELS: [Model; 1] = [HTDEMUCS];

/// One model as the UI's `ModelStatus`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelStatus {
    name: String,
    size_bytes: u64,
    downloaded: bool,
}

/// The folder the models are downloaded into.
pub(crate) fn models_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, AppError> {
    Ok(app.path().app_data_dir()?.join("models"))
}

/// The model called `name`.
fn find(name: &str) -> Result<Model, AppError> {
    MODELS
        .iter()
        .find(|model| model.name == name)
        .copied()
        .ok_or_else(|| AppError {
            code: "not_found",
            message: format!("no model named {name}"),
        })
}

/// Every model, and whether it is downloaded.
#[tauri::command]
pub(crate) fn models_status(app: AppHandle) -> Result<Vec<ModelStatus>, AppError> {
    let dir = models_dir(&app)?;
    Ok(MODELS
        .iter()
        .map(|model| ModelStatus {
            name: model.name.to_owned(),
            size_bytes: model.size(),
            downloaded: model.is_downloaded(&dir),
        })
        .collect())
}

/// Starts downloading a model (or returns the running download's job id).
/// Progress, done and failure arrive as job events.
#[tauri::command]
pub(crate) fn models_download(
    app: AppHandle,
    jobs: tauri::State<'_, Jobs>,
    name: String,
) -> Result<String, AppError> {
    let model = find(&name)?;
    let dir = models_dir(&app)?;
    Ok(jobs.start_download(model, dir, app))
}

/// Deletes a downloaded model. Refused while a download runs (it would
/// write the files again).
#[tauri::command]
pub(crate) fn models_delete(
    app: AppHandle,
    jobs: tauri::State<'_, Jobs>,
    name: String,
) -> Result<(), AppError> {
    let model = find(&name)?;
    if jobs.is_downloading() {
        return Err(AppError {
            code: "busy",
            message: "a model download is running".to_owned(),
        });
    }
    model.delete(&models_dir(&app)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_serialize_as_the_ui_model_status() {
        let json = serde_json::to_value(ModelStatus {
            name: "htdemucs".into(),
            size_bytes: 170_747_491,
            downloaded: false,
        })
        .expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({ "name": "htdemucs", "sizeBytes": 170_747_491, "downloaded": false })
        );
    }

    #[test]
    fn only_known_models_are_found() {
        assert_eq!(
            find("htdemucs").map(|model| model.name).ok(),
            Some("htdemucs")
        );
        assert_eq!(find("other").err().map(|err| err.code), Some("not_found"));
    }
}
