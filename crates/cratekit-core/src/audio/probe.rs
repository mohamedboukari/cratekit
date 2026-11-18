//! Reading an audio file's tags and properties without decoding its audio.

use std::path::Path;

use symphonia::core::formats::TrackType;
use symphonia::core::meta::{RawValue, StandardTag, Tag};

use crate::CoreError;
use crate::audio::decode::open_format;

/// What a file says about itself: its tags and stream properties, read from
/// the headers only (fast enough for a whole folder).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileInfo {
    /// The title tag, if any.
    pub title: Option<String>,
    /// The artist tag, if any.
    pub artist: Option<String>,
    /// The album tag, if any.
    pub album: Option<String>,
    /// Length in seconds from the header. An MP3 without a Xing, Info or
    /// VBRI header gets a length estimated from the bitrate of its first
    /// frames, which can be off for a VBR file (the analysis in #15 will
    /// replace it with the decoded length).
    pub duration_sec: Option<f64>,
    /// Samples per second.
    pub sample_rate: Option<u32>,
    /// Number of channels.
    pub channels: Option<u32>,
}

/// Reads the tags (ID3, MP4, Vorbis comments, RIFF INFO) and stream
/// properties of `path` without decoding the audio. Later tags win over
/// earlier ones (an ID3v2 tag over the ID3v1 tag at the end of the file).
///
/// # Errors
///
/// The same as [`decode_file`](crate::audio::decode_file) for opening:
/// [`CoreError::ReadFile`], [`CoreError::UnsupportedFormat`],
/// [`CoreError::Decode`] (a file that ends before its audio starts), and
/// [`CoreError::NoAudio`] when there is no audio track.
pub fn probe_file(path: &Path) -> Result<FileInfo, CoreError> {
    let mut format = open_format(path)?;
    let mut info = FileInfo::default();
    {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| CoreError::NoAudio {
                path: path.to_path_buf(),
            })?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio());
        info.sample_rate = params.and_then(|params| params.sample_rate);
        info.channels = params
            .and_then(|params| params.channels.as_ref())
            .and_then(|channels| u32::try_from(channels.count()).ok());
        if let (Some(frames), Some(rate)) = (track.num_frames, info.sample_rate)
            && rate > 0
        {
            info.duration_sec = Some(frames as f64 / f64::from(rate));
        }
    }

    // Every metadata revision, oldest first: a later revision replaces a
    // field only when it has a value for it (ID3v2 at the start of a file
    // wins over ID3v1 at its end).
    let mut metadata = format.metadata();
    loop {
        if let Some(revision) = metadata.current() {
            // File-level tags, then stream-level ones (Ogg keeps its tags on
            // the stream).
            let stream_tags = revision
                .per_track
                .iter()
                .flat_map(|track| &track.metadata.tags);
            let found = TagValues::collect(revision.media.tags.iter().chain(stream_tags));
            info.title = found.title.joined().or(info.title);
            info.artist = found.artist.joined().or(info.artist);
            info.album = found.album.joined().or(info.album);
        }
        if metadata.pop().is_none() {
            break;
        }
    }
    Ok(info)
}

/// The title, artist and album values in one metadata revision.
#[derive(Debug, Default)]
struct TagValues {
    title: Values,
    artist: Values,
    album: Values,
}

impl TagValues {
    /// Every value of the three fields. A field can repeat (two ARTIST
    /// comments in FLAC or Ogg) or hold a list (an ID3v2.4 frame with
    /// several null-separated values, which symphonia leaves as a raw list).
    fn collect<'a>(tags: impl Iterator<Item = &'a Tag>) -> Self {
        let mut found = Self::default();
        for tag in tags {
            let field = match (&tag.std, tag.raw.key.as_str()) {
                (Some(StandardTag::TrackTitle(_)), _) | (None, "TIT2" | "TT2") => &mut found.title,
                (Some(StandardTag::Artist(_)), _) | (None, "TPE1" | "TP1") => &mut found.artist,
                (Some(StandardTag::Album(_)), _) | (None, "TALB" | "TAL") => &mut found.album,
                _ => continue,
            };
            match (&tag.std, &tag.raw.value) {
                (
                    Some(
                        StandardTag::TrackTitle(value)
                        | StandardTag::Artist(value)
                        | StandardTag::Album(value),
                    ),
                    _,
                ) => field.push(value),
                (None, RawValue::String(value)) => field.push(value),
                (None, RawValue::StringList(values)) => {
                    for value in values.iter() {
                        field.push(value);
                    }
                }
                _ => {}
            }
        }
        found
    }
}

/// The distinct, non-empty values of one field, in order.
#[derive(Debug, Default)]
struct Values(Vec<String>);

impl Values {
    fn push(&mut self, value: &str) {
        let value = value.trim();
        if !value.is_empty() && !self.0.iter().any(|known| known == value) {
            self.0.push(value.to_owned());
        }
    }

    /// The values joined with ", ", or `None` when there are none.
    fn joined(self) -> Option<String> {
        (!self.0.is_empty()).then(|| self.0.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture;

    #[test]
    fn reads_the_tags_of_each_container() {
        // tagged.*: title "Test Tone", artist "Cratekit", album "Fixtures".
        for name in ["tagged.mp3", "tagged.flac", "tagged.m4a", "tagged.ogg"] {
            let info = probe_file(&fixture(name)).expect("the fixture probes");
            assert_eq!(info.title.as_deref(), Some("Test Tone"), "{name}");
            assert_eq!(info.artist.as_deref(), Some("Cratekit"), "{name}");
            assert_eq!(info.album.as_deref(), Some("Fixtures"), "{name}");
        }
    }

    #[test]
    fn reads_the_stream_properties() {
        let info = probe_file(&fixture("tone.wav")).expect("the fixture probes");
        assert_eq!(info.sample_rate, Some(44_100));
        assert_eq!(info.channels, Some(2));
        assert!(
            info.duration_sec
                .is_some_and(|sec| (sec - 0.5).abs() < 1e-6)
        );
        assert_eq!(info.title, None, "the tone fixtures carry no tags");
        let mono = probe_file(&fixture("mono-48k.wav")).expect("the fixture probes");
        assert_eq!((mono.sample_rate, mono.channels), (Some(48_000), Some(1)));
    }

    #[test]
    fn repeated_and_listed_values_are_all_kept() {
        use std::sync::Arc;

        use symphonia::core::meta::RawTag;

        let artist = |name: &str| {
            Tag::new_std(
                RawTag::new("ARTIST", name.to_owned()),
                StandardTag::Artist(Arc::new(name.to_owned())),
            )
        };
        let tags = [
            artist("Alpha"),
            artist(" Beta "),
            artist("Alpha"),
            // An ID3v2.4 multi-value artist frame, left raw by symphonia.
            Tag::new(RawTag::new(
                "TPE1",
                RawValue::StringList(Arc::new(vec!["Gamma".into(), "Delta".into()])),
            )),
            Tag::new_std(
                RawTag::new("TITLE", "Song".to_owned()),
                StandardTag::TrackTitle(Arc::new("Song".into())),
            ),
            Tag::new(RawTag::new("COMMENT", "ignored".to_owned())),
        ];
        let found = TagValues::collect(tags.iter());
        assert_eq!(
            found.artist.joined().as_deref(),
            Some("Alpha, Beta, Gamma, Delta")
        );
        assert_eq!(found.title.joined().as_deref(), Some("Song"));
        assert_eq!(found.album.joined(), None);
    }

    #[test]
    fn a_file_that_is_not_audio_is_an_error() {
        let result = probe_file(&fixture("make-fixtures.sh"));
        assert!(
            matches!(result, Err(CoreError::UnsupportedFormat { .. })),
            "{result:?}"
        );
    }
}
