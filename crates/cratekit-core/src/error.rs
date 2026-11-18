//! The error type for everything in `cratekit-core`.

use std::path::PathBuf;

/// Errors returned by `cratekit-core`.
///
/// `thiserror` writes the `std::error::Error` and `Display` impls from the
/// `#[error("...")]` attributes. New variants are added as modules arrive
/// (decode, analysis, database, ...).
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// A file could not be read or written.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A file could not be opened or read (the path says which).
    #[error("could not read {}: {source}", path.display())]
    ReadFile {
        /// The file.
        path: PathBuf,
        /// Why, from the operating system.
        #[source]
        source: std::io::Error,
    },

    /// The file is not in an audio format Cratekit can read.
    #[error("unsupported audio format: {}", path.display())]
    UnsupportedFormat {
        /// The file that was opened.
        path: PathBuf,
    },

    /// The file is an audio format we read, but its data is damaged or uses a
    /// feature the decoder does not support.
    #[error("could not decode {}: {reason}", path.display())]
    Decode {
        /// The file that was decoded.
        path: PathBuf,
        /// What went wrong, from the decoder.
        reason: String,
    },

    /// The file has no audio track, or its audio track has no samples.
    #[error("no audio in {}", path.display())]
    NoAudio {
        /// The file that was opened.
        path: PathBuf,
    },

    /// Audio data that breaks the `AudioBuffer` rules (no channels, channels
    /// of different lengths, a zero sample rate).
    #[error("invalid audio: {0}")]
    InvalidAudio(&'static str),

    /// The resampler could not be set up or failed while running.
    #[error("resampling failed: {0}")]
    Resample(String),

    /// The library database failed (it could not be opened, migrated, read
    /// or written).
    #[error("library database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// The library database was made by a newer Cratekit than this one.
    #[error(
        "the library database is from a newer Cratekit (schema {found}, this app knows {known})"
    )]
    DatabaseTooNew {
        /// The schema version in the file.
        found: i64,
        /// The newest schema version this build knows.
        known: i64,
    },

    /// The task was cancelled (see `progress::CancellationToken`).
    #[error("cancelled")]
    Cancelled,

    /// A track, crate or other item asked for by id does not exist.
    #[error("no {what} with id {id}")]
    NotFound {
        /// What was looked up, e.g. "track".
        what: &'static str,
        /// The id that was asked for.
        id: i64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_errors_convert_with_question_mark() {
        fn read_missing() -> Result<String, CoreError> {
            // `?` converts std::io::Error into CoreError through `#[from]`.
            Ok(std::fs::read_to_string("/definitely/not/a/real/path")?)
        }

        let err = read_missing().unwrap_err();
        assert!(matches!(err, CoreError::Io(_)));
        assert!(err.to_string().starts_with("I/O error:"));
    }

    #[test]
    fn decode_errors_name_the_file() {
        let err = CoreError::Decode {
            path: PathBuf::from("set/track.mp3"),
            reason: "malformed stream".into(),
        };
        assert_eq!(
            err.to_string(),
            "could not decode set/track.mp3: malformed stream"
        );
    }
}
