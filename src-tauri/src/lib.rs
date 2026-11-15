//! Cratekit desktop app: Tauri wiring only.
//!
//! Commands here stay thin: they validate input, call `cratekit-core` or
//! `cratekit-audio`, and map errors to [`AppError`]. No domain logic lives here.

use serde::Serialize;

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

/// Returns the app version. The first command, proving UI to Rust IPC works.
#[tauri::command]
fn app_version() -> Result<String, AppError> {
    Ok(cratekit_core::version().to_owned())
}

/// Builds and runs the Tauri app. Returns when the last window closes.
///
/// # Errors
///
/// Returns an error if the Tauri runtime fails to start.
pub fn run() -> anyhow::Result<()> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_version])
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
