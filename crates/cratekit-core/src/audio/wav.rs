//! Writing WAV files (#33): the stems cache, and later exports and masters.
//!
//! A WAV file is a RIFF header, a `fmt ` chunk describing the samples and a
//! `data` chunk with the samples interleaved, little-endian. PCM samples get
//! TPDF dither (a triangular random value of up to one step) before
//! rounding, which turns quantization distortion into a steady, very low
//! noise. The noise is deterministic (seeded from the file's name, so
//! files summed together, like stems, get independent noise), and the same
//! audio always gives the same file. A file is written to `<file name>.part`
//! next to it and renamed when complete.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use super::AudioBuffer;
use crate::CoreError;

/// How the samples are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavFormat {
    /// 16-bit integers, dithered (CD depth; the stems cache).
    Pcm16,
    /// 24-bit integers, dithered.
    Pcm24,
    /// 32-bit floats, exactly as computed.
    Float32,
}

impl WavFormat {
    fn bits(self) -> u16 {
        match self {
            Self::Pcm16 => 16,
            Self::Pcm24 => 24,
            Self::Float32 => 32,
        }
    }

    /// The `fmt ` chunk's format tag: 1 for integers, 3 for floats.
    fn tag(self) -> u16 {
        match self {
            Self::Pcm16 | Self::Pcm24 => 1,
            Self::Float32 => 3,
        }
    }
}

/// A small, fast random source for the dither (xorshift; not for anything
/// that must be unpredictable).
struct Dither(u32);

impl Dither {
    /// A uniform value in 0 to 1.
    fn uniform(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        #[expect(clippy::cast_precision_loss, reason = "24 bits fit f32 exactly")]
        let value = (self.0 >> 8) as f32 / 16_777_216.0;
        value
    }

    /// A triangular value in -1 to 1 (the difference of two uniform ones).
    fn triangular(&mut self) -> f32 {
        self.uniform() - self.uniform()
    }
}

/// `path` with `suffix` appended to its whole file name (`drums.wav` gives
/// `drums.wav.part`), so names with dots never collide.
pub(crate) fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

/// The dither seed for a file: a hash (FNV-1a) of its name, never zero.
fn seed_for(path: &Path) -> u32 {
    let name = path
        .file_name()
        .map(std::ffi::OsStr::as_encoded_bytes)
        .unwrap_or_default();
    let hash = name.iter().fold(0x811C_9DC5_u32, |hash, &byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    hash | 1
}

fn too_long() -> CoreError {
    CoreError::InvalidAudio("the audio is too long for a WAV file")
}

/// `sample` (-1 to 1) as an integer: scaled by `scale` (2^15 for 16 bits,
/// 2^23 for 24, the convention our decoder reads back), dithered, rounded and
/// clamped to `-scale` to `scale - 1`.
fn quantize(sample: f32, scale: f32, dither: &mut Dither) -> i32 {
    let value = (sample * scale + dither.triangular()).round();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "clamped to the integer range first"
    )]
    let value = value.clamp(-scale, scale - 1.0) as i32;
    value
}

/// Writes `audio` to `path` as a WAV file in `format`. The file appears only
/// once complete (written to `<path>.part`, then renamed).
///
/// # Errors
///
/// [`CoreError::InvalidAudio`] when the audio does not fit a WAV file (more
/// than 4 GB of samples, too many channels, or a sample rate too high for its
/// header), or [`CoreError::Io`].
pub fn write_wav(path: &Path, audio: &AudioBuffer, format: WavFormat) -> Result<(), CoreError> {
    let channels = audio.channels();
    let frames = audio.frames();
    let bytes_per_sample = format.bits() / 8;
    let channel_count = u16::try_from(channels.len())
        .map_err(|_| CoreError::InvalidAudio("too many channels for a WAV file"))?;
    let data_len = frames
        .checked_mul(channels.len())
        .and_then(|samples| samples.checked_mul(usize::from(bytes_per_sample)))
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(too_long)?;
    // RIFF chunks are word-aligned: an odd-sized data chunk gets a pad byte.
    let pad = data_len % 2;
    // Float files carry the optional extension size (0) in `fmt ` and a
    // `fact` chunk with the frame count, as the format asks.
    let float = format == WavFormat::Float32;
    let fmt_len: u32 = if float { 18 } else { 16 };
    let fact_len: u32 = if float { 12 } else { 0 };
    let riff_len = (4 + (8 + fmt_len) + fact_len + 8)
        .checked_add(data_len)
        .and_then(|len| len.checked_add(pad))
        .ok_or_else(too_long)?;
    let block_align = channel_count
        .checked_mul(bytes_per_sample)
        .ok_or(CoreError::InvalidAudio("too many channels for a WAV file"))?;
    let byte_rate = audio
        .sample_rate()
        .checked_mul(u32::from(block_align))
        .ok_or(CoreError::InvalidAudio(
            "the sample rate is too high for a WAV file",
        ))?;

    let part = with_suffix(path, ".part");
    let mut out = BufWriter::new(File::create(&part)?);
    out.write_all(b"RIFF")?;
    out.write_all(&riff_len.to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&fmt_len.to_le_bytes())?;
    out.write_all(&format.tag().to_le_bytes())?;
    out.write_all(&channel_count.to_le_bytes())?;
    out.write_all(&audio.sample_rate().to_le_bytes())?;
    out.write_all(&byte_rate.to_le_bytes())?;
    out.write_all(&block_align.to_le_bytes())?;
    out.write_all(&format.bits().to_le_bytes())?;
    if float {
        out.write_all(&0_u16.to_le_bytes())?;
        out.write_all(b"fact")?;
        out.write_all(&4_u32.to_le_bytes())?;
        let frame_count = u32::try_from(frames).map_err(|_| too_long())?;
        out.write_all(&frame_count.to_le_bytes())?;
    }
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;
    let mut dither = Dither(seed_for(path));
    for frame in 0..frames {
        for channel in channels {
            let sample = channel.get(frame).copied().unwrap_or(0.0);
            match format {
                WavFormat::Pcm16 => {
                    let value = quantize(sample, 32_768.0, &mut dither);
                    let bytes = value.to_le_bytes();
                    out.write_all(bytes.get(..2).unwrap_or_default())?;
                }
                WavFormat::Pcm24 => {
                    let value = quantize(sample, 8_388_608.0, &mut dither);
                    let bytes = value.to_le_bytes();
                    out.write_all(bytes.get(..3).unwrap_or_default())?;
                }
                WavFormat::Float32 => out.write_all(&sample.to_le_bytes())?,
            }
        }
    }
    if pad == 1 {
        out.write_all(&[0])?;
    }
    out.flush()?;
    out.get_ref().sync_all()?;
    drop(out);
    fs::rename(&part, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decode_file;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cratekit-wav-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        dir.join(name)
    }

    fn sine(len: usize, amplitude: f32) -> Vec<f32> {
        (0..len)
            .map(|i| amplitude * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 44_100.0).sin())
            .collect()
    }

    #[test]
    fn the_header_describes_16_bit_stereo() {
        let path = temp("header.wav");
        let audio = AudioBuffer::new(44_100, vec![vec![0.0; 10], vec![0.0; 10]]).expect("audio");
        write_wav(&path, &audio, WavFormat::Pcm16).expect("write");
        let bytes = fs::read(&path).expect("read");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 36 + 40);
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1, "PCM");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2, "channels");
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            44_100
        );
        assert_eq!(
            u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            44_100 * 4
        );
        assert_eq!(u16::from_le_bytes([bytes[32], bytes[33]]), 4, "block align");
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16, "bits");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(bytes.len(), 44 + 40);
        assert!(!with_suffix(&path, ".part").exists());
    }

    #[test]
    fn every_format_reads_back_through_our_decoder() {
        let original =
            AudioBuffer::new(44_100, vec![sine(4410, 0.5), sine(4410, -0.25)]).expect("audio");
        for (format, step) in [
            (WavFormat::Pcm16, 1.0 / 32_768.0),
            (WavFormat::Pcm24, 1.0 / 8_388_608.0),
            (WavFormat::Float32, 0.0),
        ] {
            let path = temp(&format!("{format:?}.wav"));
            write_wav(&path, &original, format).expect("write");
            let decoded = decode_file(&path).expect("decode");
            assert_eq!(decoded.sample_rate(), 44_100);
            assert_eq!(decoded.channel_count(), 2);
            assert_eq!(decoded.frames(), original.frames());
            let error = decoded
                .channels()
                .iter()
                .flatten()
                .zip(original.channels().iter().flatten())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            // Dither and rounding: at most about a step and a half.
            assert!(error <= 1.6 * step + 1e-9, "{format:?}: error {error}");
        }
    }

    #[test]
    fn samples_past_full_scale_are_clamped() {
        let path = temp("clip.wav");
        let audio = AudioBuffer::new(44_100, vec![vec![2.0, -2.0, 1.0, -1.0]]).expect("audio");
        write_wav(&path, &audio, WavFormat::Pcm16).expect("write");
        let bytes = fs::read(&path).expect("read");
        let samples: Vec<i16> = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| i16::from_le_bytes(pair))
            .collect();
        assert_eq!(samples[0], i16::MAX);
        assert_eq!(samples[1], i16::MIN);
        assert!(samples[2] >= i16::MAX - 1);
        assert!(samples[3] <= i16::MIN + 2);
    }

    #[test]
    fn an_odd_sized_data_chunk_gets_its_pad_byte() {
        // 24-bit mono, 3 frames: 9 bytes of samples, then one pad byte.
        let path = temp("odd.wav");
        let audio = AudioBuffer::new(44_100, vec![vec![0.1, 0.2, 0.3]]).expect("audio");
        write_wav(&path, &audio, WavFormat::Pcm24).expect("write");
        let bytes = fs::read(&path).expect("read");
        assert_eq!(bytes.len(), 44 + 9 + 1);
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
            bytes.len() - 8
        );
        assert_eq!(
            u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
            9,
            "data size stays odd"
        );
        assert_eq!(decode_file(&path).expect("decode").frames(), 3);
    }

    #[test]
    fn names_with_dots_keep_their_own_temporary_file() {
        let audio = AudioBuffer::new(44_100, vec![vec![0.25; 100]]).expect("audio");
        let (first, second) = (temp("Mix v1.2"), temp("Mix v1.3"));
        write_wav(&first, &audio, WavFormat::Pcm16).expect("first");
        write_wav(&second, &audio, WavFormat::Pcm16).expect("second");
        assert!(first.is_file() && second.is_file());
        assert_eq!(
            with_suffix(&first, ".part").file_name().unwrap(),
            "Mix v1.2.part"
        );
    }

    #[test]
    fn each_file_gets_its_own_dither() {
        // The same audio under two names: different noise (independent when
        // summed), but the same name always gives the same bytes.
        let audio = AudioBuffer::new(44_100, vec![vec![0.1; 1000]]).expect("audio");
        let (drums, bass) = (temp("drums.wav"), temp("bass.wav"));
        write_wav(&drums, &audio, WavFormat::Pcm16).expect("drums");
        write_wav(&bass, &audio, WavFormat::Pcm16).expect("bass");
        assert_ne!(
            fs::read(&drums).expect("read"),
            fs::read(&bass).expect("read")
        );
        let again = fs::read(&drums).expect("read");
        write_wav(&drums, &audio, WavFormat::Pcm16).expect("drums again");
        assert_eq!(fs::read(&drums).expect("read"), again);
    }

    #[test]
    fn dither_keeps_levels_below_one_step() {
        // A level of 0.3 steps would round to silence without dither; with
        // it, the average of the written samples stays near 0.3 steps.
        let level = 0.3 / 32_768.0;
        let path = temp("dither.wav");
        let audio = AudioBuffer::new(44_100, vec![vec![level; 100_000]]).expect("audio");
        write_wav(&path, &audio, WavFormat::Pcm16).expect("write");
        let bytes = fs::read(&path).expect("read");
        let sum: i64 = bytes[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| i64::from(i16::from_le_bytes(pair)))
            .sum();
        let mean = sum as f64 / 100_000.0;
        assert!((mean - 0.3).abs() < 0.02, "mean {mean}");
    }
}
