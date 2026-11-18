//! Finding the audio files in what the user adds: files and whole folders.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// File extensions Cratekit adds (SPEC, Module 1), lower case. The UI's file
/// picker uses the same list (`ui/src/lib/formats.ts`).
pub const AUDIO_EXTENSIONS: [&str; 8] = ["mp3", "wav", "flac", "aiff", "aif", "m4a", "aac", "ogg"];

/// Whether `path` has one of [`AUDIO_EXTENSIONS`], in any case.
#[must_use]
pub fn has_audio_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            AUDIO_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

/// Every audio file in `paths`: files are taken as they are (if they have an
/// audio extension), folders are searched with all their subfolders. Hidden
/// files and folders (names starting with a dot) are skipped, and symbolic
/// links to folders are not followed (no loops). Sorted, without repeats.
///
/// Folders that cannot be read are returned with the reason, next to the files.
pub(crate) fn audio_files(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<(PathBuf, io::Error)>) {
    let mut files = Vec::new();
    let mut unreadable = Vec::new();
    let mut folders: Vec<PathBuf> = Vec::new();
    for path in paths {
        match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => folders.push(path.clone()),
            Ok(_) if has_audio_extension(path) => files.push(path.clone()),
            Ok(_) => {}
            Err(err) => unreadable.push((path.clone(), err)),
        }
    }
    while let Some(folder) = folders.pop() {
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(err) => {
                unreadable.push((folder, err));
                continue;
            }
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            // `file_type` does not follow links: a link to a folder is skipped,
            // a link to a file is checked through its target.
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => folders.push(path),
                Ok(kind) if kind.is_file() && has_audio_extension(&path) => files.push(path),
                Ok(kind) if kind.is_symlink() && has_audio_extension(&path) && path.is_file() => {
                    files.push(path);
                }
                _ => {}
            }
        }
    }
    files.sort();
    files.dedup();
    (files, unreadable)
}

/// How many bytes from each end of a file go into its fingerprint.
const FINGERPRINT_BYTES: u64 = 64 * 1024;

/// A cheap fingerprint of a file's content, to notice a file that was moved
/// or changed: its length and its first and last 64 KiB, hashed with FNV-1a
/// (64-bit), as 16 hex digits. Reading a whole library of tracks to hash them
/// fully would take minutes; this reads 128 KiB per file.
///
/// # Errors
///
/// When the file cannot be opened or read.
pub(crate) fn fingerprint(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    let mut hash = Fnv1a::default();
    hash.write(&length.to_le_bytes());
    let mut chunk = Vec::new();
    (&mut file)
        .take(FINGERPRINT_BYTES)
        .read_to_end(&mut chunk)?;
    hash.write(&chunk);
    if length > FINGERPRINT_BYTES {
        chunk.clear();
        file.seek(SeekFrom::Start(
            length
                .saturating_sub(FINGERPRINT_BYTES)
                .max(FINGERPRINT_BYTES),
        ))?;
        file.take(FINGERPRINT_BYTES).read_to_end(&mut chunk)?;
        hash.write(&chunk);
    }
    Ok(format!("{:016x}", hash.0))
}

/// The 64-bit FNV-1a hash: simple, fast, and the same on every platform and
/// Rust version (unlike the standard library's hasher), so stored
/// fingerprints stay comparable.
struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv1a {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_its_published_test_vectors() {
        for (input, expected) in [
            (&b""[..], 0xcbf2_9ce4_8422_2325_u64),
            (b"a", 0xaf63_dc4c_8601_ec8c),
            (b"foobar", 0x8594_4171_f739_67e8),
        ] {
            let mut hash = Fnv1a::default();
            hash.write(input);
            assert_eq!(hash.0, expected, "{input:?}");
        }
    }

    #[test]
    fn recognizes_audio_extensions_in_any_case() {
        for name in ["a.mp3", "b.WAV", "c.Flac", "d.aif", "e.M4A", "f.ogg"] {
            assert!(has_audio_extension(Path::new(name)), "{name}");
        }
        for name in ["notes.txt", "cover.jpg", "noextension", "mp3"] {
            assert!(!has_audio_extension(Path::new(name)), "{name}");
        }
    }
}
