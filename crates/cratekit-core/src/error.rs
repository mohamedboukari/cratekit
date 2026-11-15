//! The error type for everything in `cratekit-core`.

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
}
