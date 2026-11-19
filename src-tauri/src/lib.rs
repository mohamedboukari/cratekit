//! Cratekit desktop app: Tauri wiring only.
//!
//! Commands here stay thin: they validate input, call `cratekit-core` or
//! `cratekit-audio`, and map errors to [`AppError`]. No domain logic lives here.

mod jobs;
mod library;
mod playback;
mod settings;

use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
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

/// Text of every native menu item and of the Settings window title, in the
/// UI language. The UI owns the translations and sends them
/// (`app_set_menu_labels`); Rust keeps the latest in managed state so a
/// Settings window opened later gets the right title too.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MenuLabels {
    settings: String,
    settings_window_title: String,
    #[cfg_attr(
        all(target_os = "macos", not(test)),
        expect(
            dead_code,
            reason = "macOS puts Settings in the app menu, there is no File menu"
        )
    )]
    file: String,
    edit: String,
    window: String,
    undo: String,
    redo: String,
    cut: String,
    copy: String,
    paste: String,
    select_all: String,
    minimize: String,
    close_window: String,
    quit: String,
    #[cfg_attr(
        all(not(target_os = "macos"), not(test)),
        expect(dead_code, reason = "only the macOS app menu has this item")
    )]
    about: String,
    #[cfg_attr(
        all(not(target_os = "macos"), not(test)),
        expect(dead_code, reason = "only the macOS app menu has this item")
    )]
    services: String,
    #[cfg_attr(
        all(not(target_os = "macos"), not(test)),
        expect(dead_code, reason = "only the macOS app menu has this item")
    )]
    hide: String,
    #[cfg_attr(
        all(not(target_os = "macos"), not(test)),
        expect(dead_code, reason = "only the macOS app menu has this item")
    )]
    hide_others: String,
    #[cfg_attr(
        all(not(target_os = "macos"), not(test)),
        expect(dead_code, reason = "only the macOS app menu has this item")
    )]
    show_all: String,
}

impl Default for MenuLabels {
    /// English, the default UI language, used until the UI sends its labels.
    fn default() -> Self {
        Self {
            settings: "Settings...".into(),
            settings_window_title: "Cratekit Settings".into(),
            file: "File".into(),
            edit: "Edit".into(),
            window: "Window".into(),
            undo: "Undo".into(),
            redo: "Redo".into(),
            cut: "Cut".into(),
            copy: "Copy".into(),
            paste: "Paste".into(),
            select_all: "Select All".into(),
            minimize: "Minimize".into(),
            close_window: "Close Window".into(),
            quit: "Quit".into(),
            about: "About Cratekit".into(),
            services: "Services".into(),
            hide: "Hide Cratekit".into(),
            hide_others: "Hide Others".into(),
            show_all: "Show All".into(),
        }
    }
}

/// The labels in use: read when a window or menu is built, replaced by
/// `app_set_menu_labels`.
type MenuLabelsState = Mutex<MenuLabels>;

/// The current labels (a poisoned lock still holds valid text, so it is used).
fn current_labels<R: Runtime>(app: &AppHandle<R>) -> MenuLabels {
    app.try_state::<MenuLabelsState>()
        .map(|state| state.lock().unwrap_or_else(PoisonError::into_inner).clone())
        .unwrap_or_default()
}

/// The error every Tauri command returns, serialized to the UI as `{ code, message }`.
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    /// Stable machine-readable code the UI can switch on, e.g. `"io"`.
    pub code: &'static str,
    /// Human-readable message for logs and error toasts.
    pub message: String,
}

impl From<cratekit_core::CoreError> for AppError {
    fn from(err: cratekit_core::CoreError) -> Self {
        let message = err.to_string();
        Self::from_core(&err, message)
    }
}

impl AppError {
    /// The error for a core failure, with the code the UI translates and
    /// `message` (usually the error's own text).
    fn from_core(err: &cratekit_core::CoreError, message: String) -> Self {
        let code = match err {
            cratekit_core::CoreError::Io(_) | cratekit_core::CoreError::ReadFile { .. } => "io",
            cratekit_core::CoreError::UnsupportedFormat { .. } => "unsupported_format",
            cratekit_core::CoreError::Decode { .. } => "decode",
            cratekit_core::CoreError::NoAudio { .. } => "no_audio",
            cratekit_core::CoreError::InvalidAudio(_) | cratekit_core::CoreError::Resample(_) => {
                "audio_processing"
            }
            cratekit_core::CoreError::Database(_) => "database",
            cratekit_core::CoreError::DatabaseTooNew { .. } => "database_too_new",
            cratekit_core::CoreError::NotFound { .. } => "not_found",
            cratekit_core::CoreError::Cancelled => "cancelled",
            cratekit_core::CoreError::InvalidSetting { .. } => "invalid_setting",
        };
        Self { code, message }
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
        .title(current_labels(app).settings_window_title)
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

/// Rebuilds the native menu with labels in the UI language (sent by the UI at
/// startup and whenever the language changes in Settings).
#[tauri::command]
fn app_set_menu_labels(
    app: AppHandle,
    state: tauri::State<'_, MenuLabelsState>,
    labels: MenuLabels,
) -> Result<(), AppError> {
    let menu = build_menu(&app, &labels)?;
    app.set_menu(menu)?;
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        window.set_title(&labels.settings_window_title)?;
    }
    *state.lock().unwrap_or_else(PoisonError::into_inner) = labels;
    Ok(())
}

/// The native menu: "Settings..." with Cmd/Ctrl+, plus the standard Edit and
/// Window menus (copy and paste in text fields need the Edit menu on macOS).
fn build_menu<R: Runtime>(app: &AppHandle<R>, labels: &MenuLabels) -> tauri::Result<Menu<R>> {
    let settings = MenuItemBuilder::with_id(SETTINGS_MENU_ID, &labels.settings)
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let edit = SubmenuBuilder::new(app, &labels.edit)
        .undo_with_text(&labels.undo)
        .redo_with_text(&labels.redo)
        .separator()
        .cut_with_text(&labels.cut)
        .copy_with_text(&labels.copy)
        .paste_with_text(&labels.paste)
        .select_all_with_text(&labels.select_all)
        .build()?;
    let window = SubmenuBuilder::new(app, &labels.window)
        .minimize_with_text(&labels.minimize)
        .separator()
        .close_window_with_text(&labels.close_window)
        .build()?;

    // macOS puts Settings in the app menu; Windows and Linux in File.
    #[cfg(target_os = "macos")]
    let first = SubmenuBuilder::new(app, "Cratekit")
        .about_with_text(&labels.about, None)
        .separator()
        .item(&settings)
        .separator()
        .services_with_text(&labels.services)
        .separator()
        .hide_with_text(&labels.hide)
        .hide_others_with_text(&labels.hide_others)
        .show_all_with_text(&labels.show_all)
        .separator()
        .quit_with_text(&labels.quit)
        .build()?;
    #[cfg(not(target_os = "macos"))]
    let first = SubmenuBuilder::new(app, &labels.file)
        .item(&settings)
        .separator()
        .quit_with_text(&labels.quit)
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
        .manage(MenuLabelsState::default())
        .setup(|app| {
            let state = library::LibraryState::open(app.handle());
            app.manage(state);
            app.manage(playback::PlayerState::default());
            playback::start_poller(app.handle().clone());
            let jobs = jobs::Jobs::default();
            jobs.start(app.handle().clone());
            app.manage(jobs);
            Ok(())
        })
        .menu(|app| build_menu(app, &MenuLabels::default()))
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
        .invoke_handler(tauri::generate_handler![
            app_version,
            open_settings_window,
            app_set_menu_labels,
            library::library_add_paths,
            library::library_list,
            library::library_counts,
            library::track_detail,
            library::crates_list,
            library::setlist_transitions,
            jobs::analyze_tracks,
            jobs::job_cancel,
            jobs::jobs_set_paused,
            playback::playback_load,
            playback::playback_play,
            playback::playback_pause,
            playback::playback_seek,
            playback::playback_set_filter,
            playback::audio_list_devices,
            playback::audio_set_device,
            settings::settings_get,
            settings::settings_set
        ])
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
    fn menu_labels_default_to_english_and_are_all_set() {
        let MenuLabels {
            settings,
            settings_window_title,
            file,
            edit,
            window,
            undo,
            redo,
            cut,
            copy,
            paste,
            select_all,
            minimize,
            close_window,
            quit,
            about,
            services,
            hide,
            hide_others,
            show_all,
        } = MenuLabels::default();
        assert_eq!(settings, "Settings...");
        assert_eq!([file, edit, window], ["File", "Edit", "Window"]);
        let all = [
            settings_window_title,
            undo,
            redo,
            cut,
            copy,
            paste,
            select_all,
            minimize,
            close_window,
            quit,
            about,
            services,
            hide,
            hide_others,
            show_all,
        ];
        assert!(all.iter().all(|label| !label.is_empty()));
    }

    #[test]
    fn core_io_errors_map_to_the_io_code() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let app: AppError = cratekit_core::CoreError::from(io).into();
        assert_eq!(app.code, "io");
        assert!(app.message.contains("missing"));
    }
}
