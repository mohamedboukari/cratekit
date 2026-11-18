//! Decoding audio files to an [`AudioBuffer`] with `symphonia`.

use std::fs::File;
use std::io::ErrorKind;
use std::path::Path;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::TimeBase;

use crate::CoreError;
use crate::audio::AudioBuffer;
use crate::progress::{CancellationToken, NoProgress, ProgressSink};

/// The longest stretch of missing audio (damaged packets) filled with
/// silence. A longer jump in the timestamps means the file is broken.
const MAX_GAP_SECONDS: u64 = 10;
/// Memory reserved up front from the length the header announces, at most
/// this long: a damaged header must not make us reserve a huge buffer.
const MAX_RESERVE_SECONDS: u64 = 3600;
/// A file fails when more than 1 packet in this many is damaged (5%), and at
/// least `MIN_DAMAGED_TO_FAIL` are: a few bad frames are survivable, a file
/// whose format changes halfway (half its packets rejected) is not.
const DAMAGED_SHARE_DIVISOR: usize = 20;
/// See `DAMAGED_SHARE_DIVISOR`.
const MIN_DAMAGED_TO_FAIL: usize = 8;
/// How often decoding checks for a cancel, in packets.
const CANCEL_CHECK_PACKETS: usize = 64;

/// Decodes a whole audio file to planar `f32` at its own sample rate and
/// channel count. The file is only read, never changed.
///
/// Reads MP3, WAV, FLAC, AIFF, M4A (AAC or Apple Lossless) and Ogg Vorbis;
/// the format is found
/// from the file's content, with the extension only as a hint. Encoder delay
/// and padding are trimmed when the file records them (MP3 with a LAME or
/// Xing header, Ogg), so sample 0 is the first sample of the music. M4A is
/// the exception for now: symphonia does not apply the MP4 edit list, so the
/// AAC encoder's priming samples (about 1024 frames) are kept; see
/// docs/audio.md.
///
/// A damaged packet is replaced by silence of the same length, from the
/// packet timestamps, so everything after it stays at its place in the
/// file's timeline (exactly for FLAC, WAV, AIFF and M4A, which record
/// positions; to within a frame, about 26 ms, for MP3, whose reader counts
/// frame lengths). A file cut short ends where its readable data ends.
///
/// # Errors
///
/// - [`CoreError::ReadFile`]: the file cannot be opened or read.
/// - [`CoreError::UnsupportedFormat`]: not an audio format or codec we read.
/// - [`CoreError::NoAudio`]: no audio track, or no samples in it.
/// - [`CoreError::Decode`]: damaged data that cannot be skipped (including a
///   file that ends before its audio starts), more than 5% of the packets
///   damaged, a gap over 10 s, or a sample rate or channel count that changes
///   mid-file.
pub fn decode_file(path: &Path) -> Result<AudioBuffer, CoreError> {
    decode_file_with(path, &NoProgress, &CancellationToken::new())
}

/// [`decode_file`] that reports progress (0 to 1, when the file announces its
/// length) and stops with [`CoreError::Cancelled`] soon after `cancel` is
/// cancelled. Both happen every 64 packets, a second or so of audio.
///
/// # Errors
///
/// The same as [`decode_file`], plus [`CoreError::Cancelled`].
pub fn decode_file_with(
    path: &Path,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<AudioBuffer, CoreError> {
    let read_error = |source| CoreError::ReadFile {
        path: path.to_path_buf(),
        source,
    };
    let decode_error = |reason: String| CoreError::Decode {
        path: path.to_path_buf(),
        reason,
    };
    let unsupported = || CoreError::UnsupportedFormat {
        path: path.to_path_buf(),
    };
    let no_audio = || CoreError::NoAudio {
        path: path.to_path_buf(),
    };

    let mut format = open_format(path)?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(no_audio)?;
    let track_id = track.id;
    let time_base = track.time_base;
    let announced_frames = track.num_frames;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(no_audio)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default().gapless(true))
        .map_err(|_| unsupported())?;

    let mut timeline = Timeline::default();
    let (mut packets, mut damaged, mut unsupported_packets) = (0_usize, 0_usize, 0_usize);
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            // The end of the file, or of its first stream (a chained Ogg file
            // asks for a reset to start the next one).
            Ok(None) | Err(SymphoniaError::ResetRequired) => break,
            // A file cut short: keep what was decoded.
            Err(SymphoniaError::IoError(io)) if io.kind() == ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::IoError(io)) => return Err(read_error(io)),
            Err(other) => return Err(decode_error(other.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        packets += 1;
        if packets % CANCEL_CHECK_PACKETS == 0 {
            cancel.check()?;
            if let Some(total) = announced_frames.filter(|&total| total > 0) {
                let filled = timeline.channels.first().map_or(0, Vec::len) as f64;
                progress.progress((filled / total as f64).min(1.0) as f32);
            }
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // Damaged data. The AAC decoder reports some damage as an
            // unsupported feature, so that is skipped too once the decoder
            // exists; a file where every packet is "unsupported" really is.
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => {
                damaged += 1;
                continue;
            }
            Err(SymphoniaError::Unsupported(_)) => {
                damaged += 1;
                unsupported_packets += 1;
                continue;
            }
            Err(other) => return Err(decode_error(other.to_string())),
        };
        let rate = decoded.spec().rate();
        let start = frame_at(packet.pts.get(), time_base, rate);
        timeline
            .append(&decoded, start, announced_frames)
            .map_err(|reason| decode_error(reason.into()))?;
    }

    if damaged >= MIN_DAMAGED_TO_FAIL && damaged * DAMAGED_SHARE_DIVISOR > packets {
        return Err(decode_error(format!(
            "{damaged} of {packets} audio packets are damaged or change format mid-file"
        )));
    }
    if damaged > 0 {
        // Readers can stop early after damage (the MP3 reader counts bad
        // packets against the frame count in its header): end with silence
        // up to the announced length, so the track keeps its full duration.
        timeline
            .pad_to(announced_frames)
            .map_err(|reason| decode_error(reason.into()))?;
    }
    if timeline.channels.is_empty() {
        if damaged > 0 && unsupported_packets == damaged {
            return Err(unsupported());
        }
        if damaged > 0 {
            return Err(decode_error(format!(
                "all {damaged} audio packets are damaged"
            )));
        }
        return Err(no_audio());
    }
    AudioBuffer::new(timeline.sample_rate, timeline.channels)
}

/// Opens `path` and finds its container format from the content (the
/// extension is only a hint). Reads the headers and tags, not the audio.
pub(super) fn open_format(path: &Path) -> Result<Box<dyn FormatReader>, CoreError> {
    let file = File::open(path).map_err(|source| CoreError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }
    symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|err| match err {
            SymphoniaError::IoError(io) if io.kind() == ErrorKind::UnexpectedEof => {
                CoreError::Decode {
                    path: path.to_path_buf(),
                    reason: "the file ends before its audio starts".into(),
                }
            }
            SymphoniaError::IoError(source) => CoreError::ReadFile {
                path: path.to_path_buf(),
                source,
            },
            SymphoniaError::Unsupported(_) => CoreError::UnsupportedFormat {
                path: path.to_path_buf(),
            },
            other => CoreError::Decode {
                path: path.to_path_buf(),
                reason: other.to_string(),
            },
        })
}

/// The decoded audio so far, laid out on the file's timeline.
#[derive(Default)]
struct Timeline {
    sample_rate: u32,
    channels: Vec<Vec<f32>>,
    /// Reused for each packet's samples.
    packet_samples: Vec<Vec<f32>>,
}

impl Timeline {
    /// Adds a decoded packet that starts at frame `start` of the file
    /// (`None` when the file has no timestamps: right after the previous
    /// one). Missing audio before it, from skipped packets, becomes silence.
    fn append(
        &mut self,
        decoded: &GenericAudioBufferRef<'_>,
        start: Option<u64>,
        announced_frames: Option<u64>,
    ) -> Result<(), &'static str> {
        if decoded.frames() == 0 {
            return Ok(());
        }
        let rate = decoded.spec().rate();
        if self.channels.is_empty() {
            self.sample_rate = rate;
            // Reserve from the announced length, bounded: it is only a hint.
            let reserve = announced_frames
                .unwrap_or(0)
                .min(MAX_RESERVE_SECONDS * u64::from(rate));
            let reserve = usize::try_from(reserve).unwrap_or(0);
            self.channels = (0..decoded.num_planes())
                .map(|_| {
                    let mut channel = Vec::new();
                    // A failed reservation is fine: the channel grows as it fills.
                    let _ = channel.try_reserve(reserve);
                    channel
                })
                .collect();
        } else if rate != self.sample_rate || decoded.num_planes() != self.channels.len() {
            return Err("the sample rate or channel count changes mid-file");
        }

        if let Some(start) = start {
            self.pad_to(Some(start))?;
        }
        decoded.copy_to_vecs_planar::<f32>(&mut self.packet_samples);
        for (channel, samples) in self.channels.iter_mut().zip(&self.packet_samples) {
            channel.extend_from_slice(samples);
        }
        Ok(())
    }
}

impl Timeline {
    /// Fills the channels with silence up to `frames` (nothing when they are
    /// already that long, there are no channels yet, or `frames` is `None`).
    fn pad_to(&mut self, frames: Option<u64>) -> Result<(), &'static str> {
        let filled = self.channels.first().map_or(0, Vec::len) as u64;
        let Some(gap) = frames.and_then(|frames| frames.checked_sub(filled)) else {
            return Ok(());
        };
        if gap > MAX_GAP_SECONDS * u64::from(self.sample_rate) {
            return Err("the timestamps jump forward by more than 10 seconds");
        }
        let gap = usize::try_from(gap).map_err(|_| "the timestamps are out of range")?;
        for channel in &mut self.channels {
            channel.resize(channel.len() + gap, 0.0);
        }
        Ok(())
    }
}

/// A timestamp in the track's timebase as a frame index at `rate`, or `None`
/// without a timebase. Timestamps before the start (MP3 encoder delay) are
/// frame 0.
fn frame_at(ts: i64, time_base: Option<TimeBase>, rate: u32) -> Option<u64> {
    let time_base = time_base?;
    let frames = i128::from(ts.max(0)) * i128::from(time_base.numer.get()) * i128::from(rate)
        / i128::from(time_base.denom.get());
    u64::try_from(frames).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::{rms_db, tone_power};
    use crate::testing::{Scratch, fixture, read_fixture};

    /// RMS level of a channel in dBFS, ignoring the first and last 50 ms
    /// (lossy codecs fade in and out a little). Lossy codecs keep a tone's
    /// power but not its exact peaks, so levels are compared as RMS.
    fn steady_rms(samples: &[f32], sample_rate: u32) -> f32 {
        let margin = sample_rate as usize / 20;
        rms_db(&samples[margin..samples.len() - margin])
    }

    /// Which of `candidates` is the strongest frequency in `samples`.
    fn strongest(samples: &[f32], sample_rate: u32, candidates: &[f32]) -> f32 {
        candidates
            .iter()
            .copied()
            .max_by(|a, b| {
                tone_power(samples, *a, sample_rate).total_cmp(&tone_power(
                    samples,
                    *b,
                    sample_rate,
                ))
            })
            .unwrap_or(0.0)
    }

    #[test]
    fn decodes_every_supported_format_to_the_same_tones() {
        // Each file: 0.5 s at 44.1 kHz stereo, left 440 Hz at 0.5, right 1 kHz
        // at 0.25. Expected lengths: lossless and MP3 (gapless trimming from
        // its LAME header) are exact; Ogg is the length the file records
        // (ffmpeg's Vorbis encoder ends its last page at 22 080); M4A keeps
        // the AAC encoder's 1024 priming frames and its end padding (see
        // docs/audio.md).
        for (name, lossy, frames) in [
            ("tone.wav", false, 22_050),
            ("tone.aiff", false, 22_050),
            ("tone.flac", false, 22_050),
            ("tone.mp3", true, 22_050),
            ("tone.ogg", true, 22_080),
            ("tone.m4a", true, 23_552),
        ] {
            let audio = decode_file(&fixture(name)).expect("the fixture decodes");
            assert_eq!(audio.sample_rate(), 44_100, "{name}");
            assert_eq!(audio.channel_count(), 2, "{name}");
            assert_eq!(audio.frames(), frames, "{name}");
            let [left, right] = audio.channels() else {
                panic!("{name}: expected two channels");
            };
            assert_eq!(
                strongest(left, 44_100, &[440.0, 1000.0]),
                440.0,
                "{name} left"
            );
            assert_eq!(
                strongest(right, 44_100, &[440.0, 1000.0]),
                1000.0,
                "{name} right"
            );
            // A sine peaking at 0.5 is -9.03 dBFS RMS; at 0.25, -15.05.
            let level_tolerance = if lossy { 0.5 } else { 0.01 };
            let (left_db, right_db) = (steady_rms(left, 44_100), steady_rms(right, 44_100));
            assert!(
                (left_db - -9.03).abs() < level_tolerance,
                "{name} left {left_db}"
            );
            assert!(
                (right_db - -15.05).abs() < level_tolerance,
                "{name} right {right_db}"
            );
        }
    }

    #[test]
    fn lossless_files_decode_sample_exact() {
        // 16-bit PCM holds each sample to within half a step of 1/32768.
        let wav = decode_file(&fixture("tone.wav")).expect("the fixture decodes");
        for (n, sample) in wav.channels()[0].iter().enumerate() {
            let expected = 0.5 * (std::f64::consts::TAU * 440.0 * n as f64 / 44_100.0).sin();
            assert!(
                (f64::from(*sample) - expected).abs() < 1.0 / 32_768.0,
                "sample {n}"
            );
        }
        let flac = decode_file(&fixture("tone.flac")).expect("the fixture decodes");
        assert!(flac == wav, "FLAC is lossless: the same samples as the WAV");
        let aiff = decode_file(&fixture("tone.aiff")).expect("the fixture decodes");
        assert!(aiff == wav, "AIFF holds the same 16-bit samples as the WAV");
        let alac = decode_file(&fixture("tone-alac.m4a")).expect("the fixture decodes");
        assert!(alac == wav, "Apple Lossless: the same samples as the WAV");
    }

    #[test]
    fn keeps_the_rate_and_channels_of_a_mono_24_bit_file() {
        let audio = decode_file(&fixture("mono-48k.wav")).expect("the fixture decodes");
        assert_eq!(audio.sample_rate(), 48_000);
        assert_eq!(audio.channel_count(), 1);
        assert_eq!(audio.frames(), 24_000);
        assert!((steady_rms(&audio.channels()[0], 48_000) - -9.03).abs() < 0.01);
    }

    #[test]
    fn the_extension_is_only_a_hint() {
        // An MP3 saved with the wrong extension still decodes as MP3.
        let scratch = Scratch::new("extension-hint");
        let renamed = scratch.write("mislabelled.wav", &read_fixture("tone.mp3"));
        let audio = decode_file(&renamed).expect("decodes by content");
        assert_eq!((audio.channel_count(), audio.frames()), (2, 22_050));
    }

    #[test]
    fn a_damaged_packet_becomes_silence_and_later_audio_keeps_its_place() {
        let scratch = Scratch::new("damaged-packet");
        for name in ["tone.mp3", "tone.flac", "tone.m4a"] {
            let clean = decode_file(&fixture(name)).expect("the fixture decodes");
            // Scramble a few bytes in the middle of the audio data.
            let mut bytes = read_fixture(name);
            let middle = bytes.len() / 2;
            for byte in &mut bytes[middle..middle + 40] {
                *byte ^= 0xA5;
            }
            let damaged = decode_file(&scratch.write(name, &bytes))
                .unwrap_or_else(|err| panic!("{name}: a damaged packet is skipped: {err}"));
            assert_eq!(damaged.frames(), clean.frames(), "{name}: same length");
            // MP3 frames carry no timestamps (the reader counts frame
            // lengths), so after damage MP3 positions can be off by a frame;
            // FLAC and M4A record real positions.
            if name == "tone.mp3" {
                continue;
            }
            // 75% to 95% of the way in comes from undamaged packets well after
            // the damage (a decoder needs a packet or two to settle): the
            // same samples, at the same positions.
            let tail = clean.frames() * 3 / 4..clean.frames() * 19 / 20;
            let difference = clean.channels()[0][tail.clone()]
                .iter()
                .zip(&damaged.channels()[0][tail])
                .fold(0.0_f32, |max, (a, b)| max.max((a - b).abs()));
            assert!(difference < 1e-3, "{name}: the tail moved ({difference})");
        }
    }

    #[test]
    fn aac_damage_reported_as_an_unsupported_feature_is_skipped_too() {
        // These bytes land in an AAC packet and turn it into an element the
        // decoder calls unsupported (a program config or coupling channel).
        let scratch = Scratch::new("aac-unsupported");
        let clean = decode_file(&fixture("tone.m4a")).expect("the fixture decodes");
        for offset in [942, 977] {
            let mut bytes = read_fixture("tone.m4a");
            for byte in &mut bytes[offset..offset + 4] {
                *byte ^= 0xA5;
            }
            let damaged = decode_file(&scratch.write("damaged.m4a", &bytes))
                .unwrap_or_else(|err| panic!("offset {offset}: {err}"));
            assert_eq!(damaged.frames(), clean.frames(), "offset {offset}");
        }
    }

    #[test]
    fn a_format_change_mid_file_is_an_error() {
        // 1 s at 44.1 kHz then 1 s at 48 kHz: the MP3 decoder rejects every
        // packet of the second part.
        let result = decode_file(&fixture("rate-change.mp3"));
        assert!(
            matches!(&result, Err(CoreError::Decode { reason, .. }) if reason.contains("change format")),
            "{result:?}"
        );
    }

    #[test]
    fn reports_typed_errors() {
        let missing = decode_file(&fixture("no-such-file.mp3"));
        assert!(
            matches!(&missing, Err(CoreError::ReadFile { path, .. }) if path.ends_with("no-such-file.mp3")),
            "{missing:?}"
        );
        assert!(
            missing
                .err()
                .is_some_and(|err| err.to_string().contains("no-such-file.mp3"))
        );

        // The generator script is a text file, not audio.
        let text = decode_file(&fixture("make-fixtures.sh"));
        assert!(
            matches!(text, Err(CoreError::UnsupportedFormat { .. })),
            "{text:?}"
        );

        let scratch = Scratch::new("typed-errors");
        let empty = decode_file(&scratch.write("empty.mp3", &[]));
        assert!(
            matches!(empty, Err(CoreError::UnsupportedFormat { .. })),
            "{empty:?}"
        );

        // A WAV header with no samples after it.
        let wav = read_fixture("tone.wav");
        let silent = decode_file(&scratch.write("header-only.wav", &wav[..44]));
        assert!(
            matches!(silent, Err(CoreError::NoAudio { .. })),
            "{silent:?}"
        );
    }

    #[test]
    fn a_file_cut_short_keeps_what_is_there_or_says_it_is_damaged() {
        let scratch = Scratch::new("cut-short");
        // A WAV cut after 10 000 frames (4 bytes each): keeps exactly those.
        let wav = read_fixture("tone.wav");
        let cut = decode_file(&scratch.write("cut.wav", &wav[..44 + 4 * 10_000]))
            .expect("a cut WAV keeps its first part");
        assert_eq!(cut.frames(), 10_000);
        // An MP3 cut in half keeps about its first half.
        let mp3 = read_fixture("tone.mp3");
        let cut = decode_file(&scratch.write("cut.mp3", &mp3[..mp3.len() / 2]))
            .expect("a cut MP3 keeps its first part");
        assert!(
            cut.frames() > 5_000 && cut.frames() < 22_050,
            "{} frames",
            cut.frames()
        );
        // Ogg and AIFF read their whole layout first: cut in half, the file
        // ends before its audio starts, which is damage, not a wrong format.
        for name in ["tone.ogg", "tone.aiff"] {
            let bytes = read_fixture(name);
            let cut = decode_file(&scratch.write(name, &bytes[..bytes.len() / 2]));
            assert!(
                matches!(&cut, Err(CoreError::Decode { .. })),
                "{name}: {cut:?}"
            );
        }
    }
}
