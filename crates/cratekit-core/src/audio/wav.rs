//! Writing WAV files (#33): the stems cache, and later exports and masters.
//!
//! A WAV file is a RIFF header, a `fmt ` chunk describing the samples and a
//! `data` chunk with the samples interleaved, little-endian. PCM samples get
//! TPDF dither (a triangular random value of up to one step) before
//! rounding, which turns quantization distortion into a steady, very low
//! noise. The noise is deterministic, so the same audio always gives the same
//! file. Files are written to `<name>.part` and renamed when complete.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

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
/// [`CoreError::InvalidAudio`] when the audio is too long for a WAV file
/// (more than 4 GB of samples), or [`CoreError::Io`].
pub fn write_wav(path: &Path, audio: &AudioBuffer, format: WavFormat) -> Result<(), CoreError> {
    let channels = audio.channels();
    let frames = audio.frames();
    let bytes_per_sample = usize::from(format.bits() / 8);
    let channel_count = u16::try_from(channels.len())
        .map_err(|_| CoreError::InvalidAudio("too many channels for a WAV file"))?;
    let data_len = u32::try_from(frames * channels.len() * bytes_per_sample)
        .map_err(|_| CoreError::InvalidAudio("the audio is too long for a WAV file"))?;
    // Float files carry the optional extension size (0) in `fmt ` and a
    // `fact` chunk with the frame count, as the format asks.
    let float = format == WavFormat::Float32;
    let fmt_len: u32 = if float { 18 } else { 16 };
    let fact_len: u32 = if float { 12 } else { 0 };
    let riff_len = 4 + (8 + fmt_len) + fact_len + 8 + data_len;
    let block_align = channel_count * format.bits() / 8;
    let byte_rate = audio.sample_rate() * u32::from(block_align);

    let part = path.with_extension("wav.part");
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
        let frame_count = u32::try_from(frames)
            .map_err(|_| CoreError::InvalidAudio("the audio is too long for a WAV file"))?;
        out.write_all(&frame_count.to_le_bytes())?;
    }
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;
    let mut dither = Dither(0x9E37_79B9);
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
        assert!(!path.with_extension("wav.part").exists());
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
