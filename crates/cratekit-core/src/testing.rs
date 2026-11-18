//! Helpers shared by this crate's tests: the fixture files and scratch
//! folders.

use std::path::{Path, PathBuf};

/// A synthetic test file (see `tests/fixtures/make-fixtures.sh`).
pub(crate) fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The bytes of a fixture.
pub(crate) fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read the fixture")
}

/// A scratch folder for one test, removed when dropped (even if the test
/// fails), named after the test so parallel tests never share one.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cratekit-{test}-{}", std::process::id()));
        // A folder left by an earlier crashed run would leak its files into
        // this one.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create a scratch folder");
        Self(dir)
    }

    /// The folder.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `bytes` to `name` (subfolders allowed) and returns its path.
    pub(crate) fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create a scratch subfolder");
        }
        std::fs::write(&path, bytes).expect("write a scratch file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
