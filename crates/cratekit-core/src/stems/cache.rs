//! The stems cache (#33): a split track's four stems as 16-bit WAV files in
//! a folder of their own (the app keeps one per track in its data folder),
//! and the waveforms the Stems tab draws.
//!
//! The folder appears complete or not at all: the files are written to
//! `<folder>.part`, which replaces the folder only when every file is in.

use std::fs;
use std::path::Path;
use std::time::Instant;

use super::{StemSeparator, Stems};
use crate::CoreError;
use crate::analysis::waveform::Waveform;
use crate::audio::{WavFormat, decode_file_with, write_wav};
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
/// [`CoreError::Cancelled`], or [`CoreError::Io`]. On any error `dir` is left
/// as it was.
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
    let part = dir.with_extension("part");
    if part.exists() {
        fs::remove_dir_all(&part)?;
    }
    fs::create_dir_all(&part)?;
    let written = write_all(&part, &stems, progress, cancel);
    if let Err(err) = written {
        fs::remove_dir_all(&part).ok();
        return Err(err);
    }
    let waveforms = StemWaveforms::of(&stems)?;
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::rename(&part, dir)?;
    progress.progress(1.0);
    Ok(Split {
        waveforms,
        split_sec: start.elapsed().as_secs_f64(),
    })
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
        assert!(!dir.with_extension("part").exists());
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
        assert!(!dir.with_extension("part").exists());
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
