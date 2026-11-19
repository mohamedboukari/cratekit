# Cratekit: playback

`crates/cratekit-audio/` plays the loaded track on the chosen output, the system default as fallback (SPEC, deck; ROADMAP #19, #20). `src-tauri/src/playback.rs` exposes it to the UI. Tested with `cargo nextest run -p cratekit-audio` (no audio device needed) and `-p cratekit`.

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
- **The cpal stream lives on its own thread** (on some systems it cannot move between threads). `Engine::close` stops it and waits for its threads even while other handles to the engine exist; dropping the last handle closes it too. The device's error callback sets `Status::device_error` only for errors that mean the output is gone (`is_fatal`: not an xrun, a refused real-time priority, a busy device, or the system rerouting the default output, which cpal follows by itself); see Unplugging below.
- **The engine starts on the first load,** on the output device and buffer size from the settings, so the app opens the audio device only when something is played. A chosen device that is not there falls back to the system default; a buffer size the device does not allow falls back to the device's own.
- **Changing device or buffer size while playing** (`audio_set_device`, or `settings_set` of `audioDeviceId` or `bufferSize`) restarts the engine, unless it already uses that device and buffer. The track is decoded first; then, holding the gate that play, pause, seek and loads also take, the old engine is closed (some systems give a device to one stream at a time), the new one opens, and the track is put back. Position and playing state carry over only for the track the engine really held; a newer track still loading starts at 0, stopped, as a normal load would. Play, pause and seek sent meanwhile wait and then act on the new engine. A track that can no longer be decoded leaves the deck empty and is not reloaded again. The setting is saved even when applying it fails; the UI then gets a notice instead of an error.
- **Unplugging:** when the output fails for real, the poller notices within 33 ms, reopens the system default, and playback resumes there once the track is prepared for it (about half a second); the UI gets `audio_device_changed {reason: "unplugged"}` and shows a notice. If no output can be opened, playback stops (`playback_state {playing: false}`), the UI gets `reason: "lost"` with an error notice, and the device check keeps trying to bring the deck back. The stored choice stays, so when the chosen device comes back the engine moves back to it (`reason: "selected"`).
- **Device check (about every 2 s, even with nothing playing):** lists the device ids only (no device is opened or probed; a failed listing is skipped, never read as "no devices"), sends `reason: "list-changed"` when devices were plugged or unplugged so the UI reloads its list, follows a new system default when the app follows it (`reason: "default-changed"`), and moves back to the chosen device when it returns. Device ids that name an ALSA card by its number (`hw:CARD=0`) are left out: the number can change after a reboot, and the card's name-based id is kept.
- **Following the system default:** the setting `audioDeviceId` is null (the Settings picker's "System default" entry). Picking a device by name pins it.
- **Events:** about every 33 ms the poller reads `Engine::status` and sends `playback_position {sec}` while playing or after a seek, `meter {peakDb, rmsDb}` while there is sound (peak since the last read, floored at -96 dBFS), and `playback_state {playing: false}` once when a track plays to its end. The UI writes position and meters into refs and draws them on canvas (no React renders at 30 Hz).
- **Errors:** `no_audio_device` (no output), `audio_device` (the device failed or the engine stopped responding), plus the decoding and library codes for the track.

## Not yet

Tempo (`playback_set_rate`), the DJ filter (#56), Original / Mastered / Stems modes and level matching come in their own tasks.

## Tests

- Renderer (synthetic tracks, no device): silence with nothing loaded; play fades in and then outputs the track exactly; pause fades out without a click and holds the position, play resumes there; seeking while playing fades out, jumps and fades in with no sample-to-sample step above a sine's own (no click), and lands where expected; seeking while paused moves at once, clamped to the end; the end stops playback and is counted once; a seek or load in the last 5 ms of a track keeps playing instead of ending; loading while playing fades out, waits at the new start and hands the old track back; any callback size (1 to 4096 frames) on mono, stereo and 6-channel devices; peak and RMS of a full-scale sine; **no allocation or free in `render`** through play, seek, pause and load.
- Engine: tracks are prepared as stereo at the device rate (mono doubled, extra channels dropped); NaN and infinite samples are silenced.
- App: position, meter and end events from status changes, dB conversion, which device to move to (the chosen one when available, else the default; none when already right), what a restart puts back (position and playing only for the track really on the deck), and the JSON the UI expects (`AudioDevice`, `AudioDeviceChanged` with every reason).
- Engine: which stream errors are fatal; which device ids are unstable.
- Checked by hand on macOS: the default output opens (48 kHz), the position advances in real time, a seek lands, the end is reported; the devices are listed with the default marked; a missing device id falls back to the default; a 256-frame buffer plays.
