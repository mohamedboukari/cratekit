# Cratekit: audio module

`crates/cratekit-core/src/audio/`: decoded audio in memory (SPEC, Module 1 formats). Pure Rust, no Tauri; tested with `cargo nextest run -p cratekit-core`. The real-time playback engine is a separate crate (`cratekit-audio`); this module is for offline work: analysis, stems, mastering, export.

## Layout

| File | Contents |
| --- | --- |
| `mod.rs` | Module root; re-exports `AudioBuffer`, `decode_file`, `resample` |
| `buffer.rs` | `AudioBuffer`: planar `f32` channels at one sample rate |
| `decode.rs` | `decode_file`: any supported file to an `AudioBuffer` (`symphonia` 0.6, MPL-2.0) |
| `resample.rs` | `resample`: change the sample rate (`rubato` 5, MIT OR Apache-2.0) |
| `probe.rs` | `probe_file`: tags and stream properties without decoding (`FileInfo`) |

## `AudioBuffer`

One `Vec<f32>` per channel (planar), full scale = 1.0. Private fields: `AudioBuffer::new(rate, channels)` checks there is at least one channel, all channels have the same length and the rate is above zero (`CoreError::InvalidAudio` otherwise), so every buffer keeps those rules.

| Method | Returns |
| --- | --- |
| `sample_rate()`, `channel_count()`, `frames()`, `duration_seconds()` | The shape |
| `channels()` | `&[Vec<f32>]`, in file order (left, right) |
| `channel_slices()` | `Vec<&[f32]>`, the shape `measure_loudness` takes |
| `to_mono()` | The average of all channels: what the tempo, key and energy detectors take |
| `into_channels()` | The channels, moved out without a copy |

## `decode_file(path) -> Result<AudioBuffer, CoreError>`

Decodes the whole file at its own sample rate and channel count. The file is opened read-only and never changed.

- **Formats:** MP3, WAV, FLAC, AIFF, M4A (AAC or Apple Lossless) and Ogg Vorbis (symphonia features `mp3`, `aac`, `alac`, `isomp4`, `aiff` plus its defaults). The format is found from the content; the extension is only a hint, so a mislabelled file still decodes. Not read yet, reported as `UnsupportedFormat`: Opus in Ogg (symphonia has no Opus decoder) and multichannel AAC.
- **Gapless:** encoder delay and padding are trimmed when the file records them. MP3 with a LAME or Xing header comes out at its exact length, and Ogg at the length its last page records, so sample 0 is the first sample of the music and cues line up.
- **Known limitation, M4A with AAC:** symphonia 0.6 does not apply the MP4 edit list, so AAC in M4A keeps the encoder's priming samples (1024 frames, about 23 ms, from ffmpeg's encoder; Apple's encoder uses 2112) and its end padding. Beat and cue positions in M4A tracks can be that much late. To check against VirtualDJ on real tracks (#22) before deciding whether to trim it ourselves.
- **Damaged files:** a damaged packet is replaced by silence of the same length, placed from the packet timestamps, so everything after it keeps its place in the file's timeline: exactly for FLAC, WAV, AIFF and M4A, which record positions, and to within one frame (about 26 ms) for MP3, whose reader counts frame lengths instead. If the reader stops early after damage, the end is padded with silence up to the length the header announces. A gap over 10 s, or more than 5% of the packets damaged (and at least 8), is a `Decode` error: that is a broken file, or one whose sample rate or channels change mid-stream.
- **Cut short:** WAV, FLAC and MP3 keep what was decoded before the cut. Ogg and AIFF, which read their whole layout first, report `Decode` ("the file ends before its audio starts"); an M4A whose index (`moov`) is at the end loses it and reports `UnsupportedFormat`.
- **Errors:** `ReadFile` (cannot open or read; carries the path), `UnsupportedFormat` (not audio, or a codec we do not read), `NoAudio` (no audio track or no samples), `Decode` (see above). The Tauri layer maps them to the codes `io`, `unsupported_format`, `no_audio` and `decode`, which the UI translates; `InvalidAudio` and `Resample` map to `audio_processing`.
- **Memory:** the whole track is held in memory as `f32`: about 21 MB per stereo minute at 44.1 kHz (about 127 MB for a 6 minute track). The length the header announces is reserved up front, at most one hour's worth, so a damaged header cannot make it reserve a huge buffer.

## `resample(buffer, target_rate) -> Result<AudioBuffer, CoreError>`

rubato's FFT resampler (fixed ratio, Blackman-Harris anti-aliasing filter), run over the whole clip with its startup delay trimmed, so output sample positions line up with the input. The output length is exact: input frames times the rate ratio, rounded up, computed in integers. Takes the buffer by value: a buffer already at the target rate (or empty) comes back without a copy. Errors: `InvalidAudio` for a zero target rate, `Resample` if rubato rejects the rates.

## `probe_file(path) -> Result<FileInfo, CoreError>`

Reads a file's tags and headers without decoding the audio, for adding tracks to the library. `FileInfo` has the title, artist and album tags (ID3v1 and v2, MP4, Vorbis comments, RIFF INFO; file-level and stream-level tags), and the duration, sample rate and channel count from the header. A field with several values (two ARTIST comments, or an ID3v2.4 frame holding a list) keeps them all, joined with ", "; a later tag block replaces a field only when it has a value for it, so ID3v2 wins over ID3v1. An MP3 without a Xing, Info or VBRI header gets a duration estimated from the bitrate of its first frames, which can be off for VBR files (the analysis in #15 replaces it with the decoded length). Same errors as `decode_file` for opening.

## Tests

Test files are synthetic tones in `crates/cratekit-core/tests/fixtures/` (0.5 s, 44.1 kHz stereo: left 440 Hz at 0.5, right 1 kHz at 0.25, in WAV, AIFF, FLAC, MP3, AAC and Apple Lossless M4A, and Ogg; a 48 kHz mono 24-bit WAV; an MP3 whose rate changes from 44.1 to 48 kHz halfway; 0.2 s tagged MP3, FLAC, M4A and Ogg files), made by `make-fixtures.sh` there with ffmpeg on a developer machine. No recorded or copyrighted audio. The root `.gitignore` ignores audio files everywhere and lets through only these named files, so a real track dropped in that folder stays out of git.

- Every format decodes to 44.1 kHz stereo with each channel on its own tone, at the right RMS level (lossless within 0.01 dB, lossy within 0.5 dB), and its exact expected length: 22 050 frames for WAV, AIFF, FLAC and MP3, 22 080 for Ogg (the length the file records), 23 552 for M4A (the edit-list limitation).
- WAV is sample-exact against the formula; FLAC, AIFF and Apple Lossless (M4A) give exactly the WAV's samples.
- The mono 24-bit file keeps its rate and channel count.
- An MP3 named `.wav` still decodes; typed errors for a missing file (with its path), a text file, an empty file, a WAV header with no samples.
- Damage: bytes scrambled in the middle of an MP3, FLAC and M4A keep the full length, and FLAC and M4A audio after the damage is at exactly the same positions; AAC damage that the decoder reports as an unsupported feature is skipped too; the rate-change MP3 is a `Decode` error.
- Cut short: a WAV keeps exactly the frames before the cut, an MP3 about its first half; a cut Ogg or AIFF is a `Decode` error.
- Resampling 44.1 to 48, 48 to 44.1, 96 to 44.1 and 22.05 to 48 kHz keeps the exact length (also for a minute of audio), the pitch and the level (within 0.1 dB); a click stays at the same time; a 20 kHz tone is filtered out at 32 kHz instead of folding down; the same rate is a no-op.
- `AudioBuffer`: the construction rules, the shape, mono averaging.
- `probe_file`: the tags of an MP3, FLAC, M4A and Ogg file (`tagged.*` fixtures), repeated and list values all kept, the properties of the WAV fixtures, an error for a text file.
