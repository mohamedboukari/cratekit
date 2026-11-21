//! The stems cache (#33): a split track's four stems as 16-bit WAV files in
//! a folder of their own (the app keeps one per track in its data folder),
//! and the waveforms the Stems tab draws.
//!
//! The folder appears complete or not at all: the files are written to
//! `<folder>.part`; only when every file is in, the old folder is moved aside
//! to `<folder>.old`, the new one takes its place, and the old one is deleted.
//! If the new one cannot take its place, the old one is moved back, and a
//! `.old` folder left by a crash is restored or removed on the next split.

use std::fs;
use std::path::Path;
use std::time::Instant;

use super::{StemSeparator, Stems};
use crate::CoreError;
use crate::analysis::waveform::Waveform;
use crate::audio::{WavFormat, decode_file, decode_file_with, with_suffix, write_wav};
use crate::progress::{CancellationToken, ProgressSink};

/// The files in a track's stems folder, in the model's order.
pub const STEM_FILES: [&str; 4] = ["drums.wav", "bass.wav", "other.wav", "vocals.wav"];

/// The waveforms of the four stems and of the 2-stem instrumental.
#[derive(Debug, Clone, PartialEq)]
pub struct StemWaveforms {
    /// Drums.
    pub drums: Waveform,
    /// Bass.
    pub bass: Waveform,
    /// Other.
    pub other: Waveform,
    /// Vocals.
    pub vocals: Waveform,
    /// Drums, bass and other summed.
    pub instrumental: Waveform,
}

impl StemWaveforms {
    /// Computes the waveforms of split stems.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidAudio`] if the stems do not have the same shape.
    pub fn of(stems: &Stems) -> Result<Self, CoreError> {
        Ok(Self {
            drums: Waveform::of(&stems.drums),
            bass: Waveform::of(&stems.bass),
            other: Waveform::of(&stems.other),
            vocals: Waveform::of(&stems.vocals),
            instrumental: Waveform::of(&stems.instrumental()?),
        })
    }

    fn all(&self) -> [&Waveform; 5] {
        [
            &self.drums,
            &self.bass,
            &self.other,
            &self.vocals,
            &self.instrumental,
        ]
    }

    /// The waveforms as bytes for the library: each one's length (4 bytes,
    /// little-endian) and then its bytes, in the order of the fields.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for waveform in self.all() {
            let bytes = waveform.to_bytes();
            let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&bytes);
        }
        out
    }

    /// Reads [`Self::to_bytes`] back; `None` for bytes it did not write.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut rest = bytes;
        let mut next = || -> Option<Waveform> {
            let (len, tail) = rest.split_first_chunk::<4>()?;
            let len = usize::try_from(u32::from_le_bytes(*len)).ok()?;
            let (one, tail) = tail.split_at_checked(len)?;
            rest = tail;
            Waveform::from_bytes(one)
        };
        let waveforms = Self {
            drums: next()?,
            bass: next()?,
            other: next()?,
            vocals: next()?,
            instrumental: next()?,
        };
        rest.is_empty().then_some(waveforms)
    }
}

/// What splitting a track produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
    /// The stems' waveforms.
    pub waveforms: StemWaveforms,
    /// How long it took, decoding and writing included, in seconds.
    pub split_sec: f64,
}

/// Decodes `input`, splits it with `separator`, and writes the four stems as
/// 16-bit WAV files ([`STEM_FILES`]) into `dir`, replacing what was there.
/// Progress: decoding is the first 10%, splitting up to 90%, writing the rest.
///
/// # Errors
///
/// The decoder's errors, the separator's ([`CoreError::Stems`]),
/// [`CoreError::Cancelled`], or [`CoreError::Io`]. On any error `dir` keeps
/// the stems it had (moved back if they had been moved aside).
pub fn split_to_folder(
    separator: &mut dyn StemSeparator,
    input: &Path,
    dir: &Path,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<Split, CoreError> {
    let start = Instant::now();
    let audio = decode_file_with(input, &|f: f32| progress.progress(f * 0.1), cancel)?;
    let stems = separator.separate(audio, &|f: f32| progress.progress(0.1 + f * 0.8), cancel)?;
    cancel.check()?;
    // Everything that can fail without touching the disk comes first.
    let waveforms = StemWaveforms::of(&stems)?;
    let part = with_suffix(dir, ".part");
    let old = with_suffix(dir, ".old");
    recover(dir, &part, &old)?;
    fs::create_dir_all(&part)?;
    if let Err(err) = write_all(&part, &stems, progress, cancel) {
        fs::remove_dir_all(&part).ok();
        return Err(err);
    }
    if let Err(err) = replace(dir, &part, &old) {
        fs::remove_dir_all(&part).ok();
        return Err(err);
    }
    progress.progress(1.0);
    Ok(Split {
        waveforms,
        split_sec: start.elapsed().as_secs_f64(),
    })
}

/// Reads the four stems [`split_to_folder`] wrote into `dir` (for playback).
///
/// # Errors
///
/// The decoder's errors, for example when a file is missing or damaged.
pub fn read_folder(dir: &Path) -> Result<Stems, CoreError> {
    let [drums, bass, other, vocals] = STEM_FILES;
    Ok(Stems {
        drums: decode_file(&dir.join(drums))?,
        bass: decode_file(&dir.join(bass))?,
        other: decode_file(&dir.join(other))?,
        vocals: decode_file(&dir.join(vocals))?,
    })
}

/// Cleans up after an earlier split that stopped halfway: a `.part` folder
/// is removed, and a `.old` folder is moved back when `dir` is missing (the
/// crash came between moving it aside and the new one taking its place), or
/// removed otherwise.
fn recover(dir: &Path, part: &Path, old: &Path) -> Result<(), CoreError> {
    if part.exists() {
        fs::remove_dir_all(part)?;
    }
    if old.exists() {
        if dir.exists() {
            fs::remove_dir_all(old)?;
        } else {
            fs::rename(old, dir)?;
        }
    }
    Ok(())
}

/// Puts the complete `part` folder in place of `dir`: the old folder moves
/// aside first, and moves back if the new one cannot take its place.
fn replace(dir: &Path, part: &Path, old: &Path) -> Result<(), CoreError> {
    let had_old = dir.exists();
    if had_old {
        fs::rename(dir, old)?;
    }
    if let Err(err) = fs::rename(part, dir) {
        if had_old {
            fs::rename(old, dir).ok();
        }
        return Err(err.into());
    }
    if had_old {
        // The new stems are in place; a leftover here is removed next time.
        fs::remove_dir_all(old).ok();
    }
    Ok(())
}

/// Writes the four stems into `part`.
fn write_all(
    part: &Path,
    stems: &Stems,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<(), CoreError> {
    let audio = [&stems.drums, &stems.bass, &stems.other, &stems.vocals];
    for (index, (file, stem)) in STEM_FILES.iter().zip(audio).enumerate() {
        cancel.check()?;
        write_wav(&part.join(file), stem, WavFormat::Pcm16)?;
        #[expect(clippy::cast_precision_loss, reason = "four files")]
        progress.progress(0.9 + 0.1 * (index + 1) as f32 / STEM_FILES.len() as f32);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;
    use crate::audio::{AudioBuffer, decode_file};
    use crate::progress::NoProgress;

    /// A stand-in for the model: drums get half the mix, bass a quarter,
    /// vocals nothing, other the rest.
    struct Halves;

    impl StemSeparator for Halves {
        fn separate(
            &mut self,
            audio: AudioBuffer,
            progress: &dyn ProgressSink,
            cancel: &CancellationToken,
        ) -> Result<Stems, CoreError> {
            cancel.check()?;
            progress.progress(1.0);
            let scaled = |k: f32| {
                AudioBuffer::new(
                    audio.sample_rate(),
                    audio
                        .channels()
                        .iter()
                        .map(|c| c.iter().map(|s| s * k).collect())
                        .collect(),
                )
            };
            Ok(Stems {
                drums: scaled(0.5)?,
                bass: scaled(0.25)?,
                other: scaled(0.25)?,
                vocals: scaled(0.0)?,
            })
        }
    }

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.wav")
    }

    fn temp(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cratekit-cache-{label}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn writes_the_four_stems_and_their_waveforms() {
        let dir = temp("ok").join("42");
        let seen = Mutex::new(Vec::new());
        let progress = |f: f32| seen.lock().expect("seen").push(f);

        let split = split_to_folder(
            &mut Halves,
            &fixture(),
            &dir,
            &progress,
            &CancellationToken::new(),
        )
        .expect("split");

        let original = decode_file(&fixture()).expect("decode");
        let drums = decode_file(&dir.join("drums.wav")).expect("drums");
        assert_eq!(drums.sample_rate(), original.sample_rate());
        assert_eq!(drums.frames(), original.frames());
        let error = drums.channels()[0]
            .iter()
            .zip(&original.channels()[0])
            .map(|(d, o)| (d - o * 0.5).abs())
            .fold(0.0_f32, f32::max);
        assert!(error < 1e-4, "16-bit drums error {error}");
        for file in STEM_FILES {
            assert!(dir.join(file).is_file(), "{file}");
        }
        assert!(!with_suffix(&dir, ".part").exists());
        assert!(!with_suffix(&dir, ".old").exists());
        assert!(split.split_sec >= 0.0);
        let seen = seen.into_inner().expect("seen");
        assert!(
            seen.windows(2).all(|pair| pair[0] <= pair[1]),
            "progress goes forward"
        );
        assert_eq!(seen.last(), Some(&1.0));
        fs::remove_dir_all(dir.parent().expect("parent")).ok();
    }

    #[test]
    fn the_folder_reads_back_as_the_four_stems() {
        let dir = temp("read").join("3");
        split_to_folder(
            &mut Halves,
            &fixture(),
            &dir,
            &NoProgress,
            &CancellationToken::new(),
        )
        .expect("split");

        let stems = read_folder(&dir).expect("read");

        let original = decode_file(&fixture()).expect("decode");
        let level = |stem: &crate::audio::AudioBuffer| -> f32 {
            stem.channels()[0]
                .iter()
                .map(|s| s.abs())
                .fold(0.0, f32::max)
        };
        let peak = level(&original);
        assert!((level(&stems.drums) - peak * 0.5).abs() < 1e-3);
        assert!((level(&stems.bass) - peak * 0.25).abs() < 1e-3);
        assert!(level(&stems.vocals) < 1e-3);
        assert_eq!(stems.other.frames(), original.frames());
        fs::remove_dir(&dir).ok();
        assert!(read_folder(&dir.join("missing")).is_err());
        fs::remove_dir_all(dir.parent().expect("parent")).ok();
    }

    #[test]
    fn a_new_split_replaces_the_old_folder() {
        let dir = temp("replace").join("7");
        fs::create_dir_all(&dir).expect("dir");
        fs::write(dir.join("stale.txt"), b"old").expect("stale");

        split_to_folder(
            &mut Halves,
            &fixture(),
            &dir,
            &NoProgress,
            &CancellationToken::new(),
        )
        .expect("split");

        assert!(!dir.join("stale.txt").exists());
        assert!(dir.join("vocals.wav").is_file());
        fs::remove_dir_all(dir.parent().expect("parent")).ok();
    }

    #[test]
    fn a_cancelled_split_leaves_the_folder_as_it_was() {
        let dir = temp("cancel").join("9");
        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = split_to_folder(&mut Halves, &fixture(), &dir, &NoProgress, &cancel);

        assert!(matches!(result, Err(CoreError::Cancelled)), "{result:?}");
        assert!(!dir.exists());
        assert!(!with_suffix(&dir, ".part").exists());
    }

    #[test]
    fn folders_with_dots_in_their_names_do_not_collide() {
        let root = temp("dots");
        let (whole, dotted) = (root.join("42"), root.join("42.1"));
        split_to_folder(
            &mut Halves,
            &fixture(),
            &whole,
            &NoProgress,
            &CancellationToken::new(),
        )
        .expect("42");
        split_to_folder(
            &mut Halves,
            &fixture(),
            &dotted,
            &NoProgress,
            &CancellationToken::new(),
        )
        .expect("42.1");
        assert!(whole.join("drums.wav").is_file());
        assert!(dotted.join("drums.wav").is_file());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn leftovers_of_a_crash_are_recovered() {
        let root = temp("crash");
        // A crash after the old folder moved aside: no folder, only `.old`.
        let dir = root.join("5");
        let old = with_suffix(&dir, ".old");
        fs::create_dir_all(&old).expect("old");
        fs::write(old.join("drums.wav"), b"old stems").expect("old file");
        fs::create_dir_all(with_suffix(&dir, ".part")).expect("part");
        recover(&dir, &with_suffix(&dir, ".part"), &old).expect("recover");
        assert_eq!(
            fs::read(dir.join("drums.wav")).expect("restored"),
            b"old stems"
        );
        assert!(!old.exists() && !with_suffix(&dir, ".part").exists());

        // The next split replaces the restored folder and leaves nothing else.
        split_to_folder(
            &mut Halves,
            &fixture(),
            &dir,
            &NoProgress,
            &CancellationToken::new(),
        )
        .expect("split");
        assert_ne!(fs::read(dir.join("drums.wav")).expect("new"), b"old stems");
        assert!(!old.exists());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn waveforms_read_back_from_their_bytes() {
        let audio = decode_file(&fixture()).expect("decode");
        let stems = Halves
            .separate(audio, &NoProgress, &CancellationToken::new())
            .expect("split");
        let waveforms = StemWaveforms::of(&stems).expect("waveforms");
        let bytes = waveforms.to_bytes();
        assert_eq!(StemWaveforms::from_bytes(&bytes), Some(waveforms));
        assert_eq!(StemWaveforms::from_bytes(&bytes[..bytes.len() - 1]), None);
        assert_eq!(StemWaveforms::from_bytes(&[]), None);
    }
}
