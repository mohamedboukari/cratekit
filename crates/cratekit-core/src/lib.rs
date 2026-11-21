//! Cratekit core: everything that is not UI and not real-time audio.
//!
//! This crate must build and test without Tauri (`cargo test -p cratekit-core`).
//! Modules from `docs/ARCHITECTURE.md` (audio, analysis, mastering, stems,
//! library, export) are added here task by task. So far: [`audio`] (decoding
//! files to planar `f32`, resampling, reading tags), [`library`] (the SQLite
//! track library), [`settings`] (the app settings), [`analysis::key`]
//! (musical keys and the Camelot wheel), [`analysis::tempo`] and
//! [`analysis::key_detect`] (tempo and key detection), [`analysis::loudness`]
//! (integrated LUFS and true peak), [`analysis::energy`] (the 1 to 10 energy
//! score), [`stems::runtime`] (loading ONNX Runtime for stems), and `synth` (synthetic test audio, compiled for tests or with the
//! `test-helpers` feature).

pub mod analysis;
pub mod audio;
mod error;
pub mod files;
pub mod library;
pub mod progress;
pub mod settings;
pub mod stems;
#[cfg(any(test, feature = "test-helpers"))]
pub mod synth;
#[cfg(test)]
mod testing;

pub use error::CoreError;

/// The Cratekit version, taken from `Cargo.toml` at compile time.
///
/// `env!` reads an environment variable while compiling, so the version is
/// baked into the binary and costs nothing at run time.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_major_minor_patch() {
        let parts: Vec<&str> = version().split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "expected MAJOR.MINOR.PATCH, got {}",
            version()
        );
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
    }
}
