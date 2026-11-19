//! Settings commands: thin wrappers over the settings stored in the library
//! (`cratekit_core::settings`). Every change is sent to every window as
//! `settings_changed`, so the main window and the Settings window agree.

use cratekit_core::settings::Settings;
use tauri::{AppHandle, Emitter};

use crate::AppError;
use crate::library::with_library;

/// All settings: stored values over the defaults.
#[tauri::command]
pub(crate) async fn settings_get(app: AppHandle) -> Result<Settings, AppError> {
    with_library(app, |library| library.settings()).await
}

/// Changes one setting (`key` as the UI names it), after Rust checks it,
/// and tells every window.
#[tauri::command]
pub(crate) async fn settings_set(
    app: AppHandle,
    key: String,
    value: serde_json::Value,
) -> Result<(), AppError> {
    let settings =
        with_library(app.clone(), move |library| library.set_setting(&key, value)).await?;
    // A window that is closing cannot receive it; the others still do.
    app.emit("settings_changed", settings).ok();
    Ok(())
}
