//! Cratekit desktop app: Tauri wiring only.
//!
//! Commands here stay thin: they validate input, call `cratekit-core` or
//! `cratekit-audio`, and map errors to [`AppError`]. No domain logic lives here.

use serde::Serialize;
use tauri::menu::{Menu, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Window label of the main window (see `tauri.conf.json`).
const MAIN_LABEL: &str = "main";
/// Window label of the Settings window (the capability in `capabilities/settings.json` uses it).
const SETTINGS_LABEL: &str = "settings";
/// Menu item id for "Settings...".
const SETTINGS_MENU_ID: &str = "open-settings";
/// The UI renders the Settings view for this hash (see `ui/src/main.tsx`).
const SETTINGS_URL: &str = "index.html#settings";

/// The error every Tauri command returns, serialized to the UI as `{ code, message }`.
#[derive(Debug, Serialize)]
pub struct AppError {
    /// Stable machine-readable code the UI can switch on, e.g. `"io"`.
    pub code: &'static str,
    /// Human-readable message for logs and error toasts.
    pub message: String,
}

impl From<cratekit_core::CoreError> for AppError {
    fn from(err: cratekit_core::CoreError) -> Self {
        let code = match err {
            cratekit_core::CoreError::Io(_) => "io",
        };
        Self {
            code,
            message: err.to_string(),
        }
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        Self {
            code: "window",
            message: err.to_string(),
        }
    }
}

/// Returns the app version. The first command, proving UI to Rust IPC works.
#[tauri::command]
fn app_version() -> Result<String, AppError> {
    Ok(cratekit_core::version().to_owned())
}

/// Opens the Settings window, or brings it to the front when it is already open.
fn open_settings<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }
    WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App(SETTINGS_URL.into()))
        .title("Cratekit Settings")
        .inner_size(760.0, 560.0)
        .min_inner_size(640.0, 480.0)
        .resizable(true)
        .theme(Some(tauri::Theme::Dark))
        .build()?;
    Ok(())
}

/// Opens the Settings window from the UI (sidebar button, Cmd/Ctrl+,).
///
/// Async on purpose: creating a window from a sync command can deadlock on Windows.
#[tauri::command]
async fn open_settings_window(app: AppHandle) -> Result<(), AppError> {
    open_settings(&app)?;
    Ok(())
}

/// The native menu: "Settings..." with Cmd/Ctrl+, plus the standard Edit and
/// Window menus (copy and paste in text fields need the Edit menu on macOS).
fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let settings = MenuItemBuilder::with_id(SETTINGS_MENU_ID, "Settings...")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .separator()
        .close_window()
        .build()?;

    // macOS puts Settings in the app menu; Windows and Linux in File.
    #[cfg(target_os = "macos")]
    let first = SubmenuBuilder::new(app, "Cratekit")
        .about(None)
        .separator()
        .item(&settings)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    #[cfg(not(target_os = "macos"))]
    let first = SubmenuBuilder::new(app, "File")
        .item(&settings)
        .separator()
        .quit()
        .build()?;

    Menu::with_items(app, &[&first, &edit, &window])
}

/// Builds and runs the Tauri app. Returns when the last window closes.
///
/// # Errors
///
/// Returns an error if the Tauri runtime fails to start.
pub fn run() -> anyhow::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .menu(build_menu)
        .on_menu_event(|app, event| {
            if event.id() == SETTINGS_MENU_ID {
                // A failure here has no caller to report to; the menu item stays usable.
                open_settings(app).ok();
            }
        })
        .on_window_event(|window, event| {
            // Settings belongs to the main window: closing main closes it too, so the app quits.
            if window.label() == MAIN_LABEL
                && matches!(event, WindowEvent::Destroyed)
                && let Some(settings) = window.app_handle().get_webview_window(SETTINGS_LABEL)
            {
                settings.close().ok();
            }
        })
        .invoke_handler(tauri::generate_handler![app_version, open_settings_window])
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_version_returns_the_core_version() {
        assert_eq!(app_version().unwrap(), cratekit_core::version());
    }

    #[test]
    fn core_io_errors_map_to_the_io_code() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let app: AppError = cratekit_core::CoreError::from(io).into();
        assert_eq!(app.code, "io");
        assert!(app.message.contains("missing"));
    }
}
