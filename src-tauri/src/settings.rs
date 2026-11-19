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
/// and tells every window. A new buffer size or output device is applied to
/// the running engine at once (it restarts and keeps the track playing).
#[tauri::command]
pub(crate) async fn settings_set(
    app: AppHandle,
    key: String,
    value: serde_json::Value,
) -> Result<(), AppError> {
    let audio = matches!(key.as_str(), "bufferSize" | "audioDeviceId");
    let settings =
        with_library(app.clone(), move |library| library.set_setting(&key, value)).await?;
    let wanted = settings.audio_device_id.clone().unwrap_or_default();
    // A window that is closing cannot receive it; the others still do.
    app.emit("settings_changed", settings).ok();
    if audio {
        // Apply the new buffer size or device to a running engine. The value
        // is saved either way: a failure is a notice, not an error here.
        let applied = crate::playback::apply_audio_settings(app.clone()).await;
        crate::playback::report_applied(&app, applied, wanted);
    }
    Ok(())
}
