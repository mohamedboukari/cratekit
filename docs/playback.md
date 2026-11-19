# Cratekit: playback

`crates/cratekit-audio/` plays the loaded track on the default output (SPEC, deck; ROADMAP #19). `src-tauri/src/playback.rs` exposes it to the UI. Tested with `cargo nextest run -p cratekit-audio` (no audio device needed) and `-p cratekit`.

## Layout

| File | Contents |
| --- | --- |
| `crates/cratekit-audio/src/renderer.rs` | `Renderer::render` (the audio callback), `Command`, `Track`, `Shared` |
| `crates/cratekit-audio/src/engine.rs` | `Engine`: the output device, track preparation, commands, `Status` |
| `src-tauri/src/playback.rs` | `PlayerState`, the playback commands and the 30 Hz poller |

## How it fits together

```
playback_load ── decode_file + resample (blocking worker) ── Command::Load(Box<Track>) ──► rtrb ──► Renderer
play / pause / seek ─────────────────────────────────────── Command ──────────────────────► rtrb ──► Renderer
Renderer ── replaced Box<Track> ──► rtrb ──► cleanup thread (frees it)
Renderer ── position, playing, ends, peak, rms ──► atomics ──► poller (33 ms) ──► playback_position, meter, playback_state
```

- **The audio callback** (`Renderer::render`) never allocates, frees, locks, blocks, does I/O or panics. Commands arrive over a lock-free `rtrb` queue; a replaced track leaves over a second queue and is freed on the cleanup thread (when that queue is full it waits in one of four slots; only if all are full is it leaked, counted in `Shared::leaked_tracks`, rather than freed on the audio thread). A test counts allocations on the rendering thread and asserts none.
- **Whole track in memory.** `Engine::prepare` takes the decoded track by value, makes it stereo (mono on both sides, the first two channels of anything wider), silences any non-finite sample from a damaged file, and resamples it to the device rate; `Engine::load` sends it ready to play. Seeking is then an index change. A loaded track takes about 21 MB per stereo minute; while preparing, the decoded input and the resampled output are both alive for a moment (about twice that).
- **Decks.** Commands and status carry a `DeckId` (only `A` in v0.1), so Performance mode (v0.2) can add a second deck without changing the engine's interface.
- **Latest load wins.** The app counts load requests: a load that finishes decoding after a newer one was asked for is dropped, and the counter is checked again under a lock right before the track is sent.
- **Click-free.** Play, pause, seek and load fade over 5 ms (`FADE_SECONDS`): pause fades out then holds the position; seek fades out, jumps, fades back in; a load fades the old track out and waits at the new one's start, stopped. A seek while paused moves at once. A seek or load during the last fade of a track is not counted as its end.
- **Any device.** Any callback size and channel count: stereo goes to the first two outputs (extra outputs silent), a mono device gets the average. A partial frame at the end of a buffer stays silent.
- **Sample format.** The renderer works in `f32`. The engine uses the device's default format when it is `f32`, else an `f32` format the device also offers, else converts to its integer format (`i16`, `i32`, `u16`) at the output, through a scratch buffer allocated before the stream starts.
- **The cpal stream lives on its own thread** (on some systems it cannot move between threads); dropping the `Engine` closes it. The device's error callback only sets a flag (`Status::device_error`), for the output-device task (#20).
- **The engine starts on the first load,** so the app opens the audio device only when something is played.
- **Events:** about every 33 ms the poller reads `Engine::status` and sends `playback_position {sec}` while playing or after a seek, `meter {peakDb, rmsDb}` while there is sound (peak since the last read, floored at -96 dBFS), and `playback_state {playing: false}` once when a track plays to its end. The UI writes position and meters into refs and draws them on canvas (no React renders at 30 Hz).
- **Errors:** `no_audio_device` (no output), `audio_device` (the device failed or the engine stopped responding), plus the decoding and library codes for the track.

## Not yet

Tempo (`playback_set_rate`), the DJ filter (#56), Original / Mastered / Stems modes, level matching, choosing the output device and recovering when it is unplugged (#20) come in their own tasks.

## Tests

- Renderer (synthetic tracks, no device): silence with nothing loaded; play fades in and then outputs the track exactly; pause fades out without a click and holds the position, play resumes there; seeking while playing fades out, jumps and fades in with no sample-to-sample step above a sine's own (no click), and lands where expected; seeking while paused moves at once, clamped to the end; the end stops playback and is counted once; a seek or load in the last 5 ms of a track keeps playing instead of ending; loading while playing fades out, waits at the new start and hands the old track back; any callback size (1 to 4096 frames) on mono, stereo and 6-channel devices; peak and RMS of a full-scale sine; **no allocation or free in `render`** through play, seek, pause and load.
- Engine: tracks are prepared as stereo at the device rate (mono doubled, extra channels dropped); NaN and infinite samples are silenced.
- App: position, meter and end events from status changes, dB conversion, and the JSON the UI expects.
- Checked by hand on macOS: the default output opens (48 kHz), the position advances in real time, a seek lands, the end is reported.
