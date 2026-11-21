# Cratekit: Architecture

## Overview

```
┌──────────────┐   IPC (commands + events)   ┌───────────────────────────┐   jobs   ┌──────────────────────┐
│   React UI   │ ◄─────────────────────────► │  Tauri app (src-tauri)    │ ───────► │  cratekit-core       │
│   (ui/)      │                             │  commands, MIDI, wiring   │          │  analysis, mastering,│
└──────────────┘                             └───────────┬───────────────┘          │  stems, library,     │
                                                         │                          │  export              │
┌──────────────┐   USB MIDI (midir)                      │                          └──────────┬───────────┘
│ DJ controller│ ───────────────────────────────────────►│                                     │
└──────────────┘                                         ▼                                     ▼
                                              ┌─────────────────────┐            ┌──────────────────────┐
                                              │ cratekit-audio      │            │ SQLite library +     │
                                              │ cpal playback,      │            │ app data folder      │
                                              │ stem mixer, A/B     │            │ (stems, masters,     │
                                              └─────────────────────┘            │  waveforms, models)  │
                                                                                 └──────────────────────┘
```

- **One Rust process.** No Python, no sidecars.
- `cratekit-core` is a normal Rust library crate: testable with `cargo test`, no Tauri dependency.
- `cratekit-audio` owns the real-time audio thread.
- `src-tauri` is glue: it exposes commands, emits events, owns the MIDI connection and the job queue.
- `ui/` renders state and sends commands. It holds no domain logic.

## Threads

| Thread | Owns | Rules |
| --- | --- | --- |
| Main / Tauri | Window, IPC | Never does heavy work |
| Audio (cpal callback) | Output buffer | No alloc, no locks, no I/O. Reads commands from an `rtrb` ring buffer; publishes position through an `AtomicU64` |
| Decoder | Decodes the track to load, off the audio thread | Today the whole track is decoded and resampled before it reaches the callback (about 21 MB per stereo minute); streaming 2 s ahead can replace it if memory matters |
| Audio cleanup | Frees tracks the callback replaced (they arrive over a second rtrb queue) | Never on the audio thread |
| Audio stream owner | Holds the cpal stream (on some systems it cannot move between threads) | Dropping the `Engine` closes it |
| Playback poller | Reads `Engine::status` every 33 ms and sends `playback_position`, `meter`, `playback_state`; restarts the engine when the output fails; about every 2 s checks the device list, the chosen and the default device (ids only), re-decoding the track off the audio thread when it moves | Never touches the audio thread except through atomics |
| MIDI (midir callback) | Incoming messages | Translates to `ControlEvent`, pushes to a channel; never touches audio directly |
| Job workers (tokio + rayon) | Analysis, stems, mastering, export | Report progress via `ProgressSink`, support cancellation |
| DB writer | The single write connection to SQLite | All writes go through it; reads use their own connections (WAL mode) |

## Workspace crates

### `crates/cratekit-core`

```
src/
  lib.rs
  error.rs             CoreError (thiserror)
  synth.rs             synthetic test audio (kick, chords, bass, sine, SPEC loop); tests or `test-helpers` feature only
  audio/               AudioBuffer, decode_file (symphonia), resample (rubato), probe_file (tags and stream properties); write WAV (hound) later; see docs/audio.md
  analysis/
    mod.rs             ANALYZER_VERSION
    track.rs           analyze_file() -> TrackAnalysis (progress, cancellation)
    tempo.rs           trait TempoDetector; OnsetTempo (ours, the default)
    key.rs             Key, PitchClass, Camelot (mapping, matches(), mixes_well_with(), parsing), scale flags
    key_detect.rs      trait KeyDetector; ProfileKey (ours, the default: chroma + Krumhansl-Kessler)
    stratum.rs         StratumTempo, StratumKey: stratum-dsp behind the same traits (`stratum` feature, off)
    loudness.rs        integrated LUFS, true peak (ebur128 crate)
    energy.rs          1 to 10 score (loudness, onset density, brightness)
    waveform.rs        min/max/RMS columns (20 per second, one byte each), stored with the analysis; a 1200-column overview for the deck
  mastering/
    mod.rs             master() -> MasterResult
    presets.rs         Preset structs (see table below)
    eq.rs              biquad shelves and peaks (RBJ cookbook)
    compressor.rs      RMS glue compressor
    limiter.rs         look-ahead true-peak limiter (4x oversampled detection)
    reference.rs       reference-track spectral matching
  stems/
    mod.rs             trait StemSeparator; separate() -> Stems (drums, bass, other, vocals)
    dsp.rs             STFT and inverse STFT, normalizations, segments and their blending, as the reference
    runtime.rs         load ONNX Runtime at run time; CPU check; why stems are unavailable (docs/stems.md)
    htdemucs.rs        HtDemucs: impl StemSeparator on `ort`; "other" is the mix minus the other three
    models.rs          the pinned model; download (resume, SHA-256, rename when verified), status, delete; the only network call
  library/
    mod.rs             Library: open (WAL, foreign keys), add_paths, list, counts, track, remove_tracks; see docs/library.md
    schema.rs          the schema as numbered migrations (PRAGMA user_version)
    scan.rs            folder scan, audio extensions, file fingerprint (tags come from audio::probe_file)
  export/
    virtualdj.rs       read/write VirtualDJ database (format TBD, see ROADMAP research task)
  progress.rs          ProgressSink trait (any Fn(f32)), NoProgress, CancellationToken
  settings.rs          Settings (the UI's settings, with defaults and rules), stored per key in the library's settings table
```

### `crates/cratekit-audio`

```
src/
  engine.rs      Engine: opens the chosen output from EngineOptions {device_id, buffer_size}, falling back to the default
                 (cpal stream on its own thread), prepare_track (stereo, resampled), load / play / pause / seek, status, close;
                 output_devices, output_device_ids, default_device_id, is_fatal; a cleanup thread frees replaced tracks.
                 Done (#19, #20); see docs/playback.md
  renderer.rs    Renderer::render (the audio callback), Command (sent over rtrb), Track, Shared (atomics).
                 Done (#19)
  mixer.rs       modes: Original | Mastered(preview chain) | Stems(4 gains + mute/solo) (planned)
  filter.rs      DJ filter (one knob: LP below centre, HP above)
```

A/B and mode switches use a 10 ms crossfade to stay click-free.

**Built to grow into Performance mode (v0.2+, SPEC Module 8):** engine commands and published state carry a deck id from the start (v0.1 uses deck A only), the mixer keeps per-deck FX slots (empty in v0.1), and the UI deck component can be instantiated twice. Time-stretching for sync and key lock must be GPL-free (Rubber Band is GPL): see the research issue.

### `src-tauri`

```
src/
  main.rs        builder, state, plugins
  state.rs       AppState { db, engine, jobs, midi }
  library.rs     library commands and LibraryState (done: library_add_paths, library_list, library_counts, track_detail, crates_list)
  settings.rs    settings_get / settings_set (sends settings_changed to every window). Done (#21)
  playback.rs    PlayerState (engine started on first load, on the device and buffer from the settings), playback_load / play / pause / seek,
                 audio_list_devices / audio_set_device, engine restart that keeps the track, the 30 Hz poller (playback_position, meter,
                 playback_state at the end of a track) and the device watcher (unplug, default change). Done (#19, #20)
  commands/      mastering.rs, stems.rs, midi.rs, export.rs (planned)
  midi/          device watch, MIDI learn, mapping store, LED feedback
  jobs.rs        job queue (one worker thread; analysis jobs), job_progress / job_done / job_failed events; done: analyze_tracks, job_cancel, jobs_set_paused
```

## UI (`ui/`, React 19 + Vite + TypeScript, bun)

```
ui/
  index.html
  vite.config.ts
  src/
    main.tsx           React root
    App.tsx            window layout (titlebar, sidebar, deck, tool tabs, browser, status bar)
    lib/
      ipc.ts           CratekitApi contract (one function per command, one onX() per event) + initApi()/api()
      ipc.tauri.ts     real implementation: the ONLY place that calls invoke() and listen()
      types.ts         the UI's payload types (ahead of Rust where features are not built yet)
      bindings/        GENERATED by ts-rs from the Rust payload structs; never edit by hand
      bindings.test.ts type-level contract: Rust payloads fit types.ts, UI arguments fit Rust
      shortcuts.ts     the single keyboard shortcut map
      notify.ts        command errors as toasts
      format.ts        display formatting only (times, durations, filter text, backend and job labels)
      dnd.ts           drag and drop payload type and validation
      focus.ts         focus helpers (buttons give focus back after a mouse click)
    i18n/              i18next instance (English for every user by default), typed keys, en/ (source) and one folder per language; see docs/UI.md, Languages
    mock/              fake-data CratekitApi + fixtures (browser dev server, Pages demo, tests);
                       only loaded when DEV or VITE_MOCK=1, so production Tauri builds never contain it
    stores/            Zustand stores per domain: library, player, jobs, settings, ui, devices, setlist, master, stems, controller
    components/        one component per file, PascalCase .tsx (see docs/UI.md); controls/ holds shared widgets (Knob)
    hooks/             useIpcEvents (Rust events into stores), usePlaybackRefs (30 Hz refs), useShortcuts, usePaneLayout (saved pane sizes), useHearing (what you hear follows the tool tab)
    canvas/            waveform, overview bar, meters: imperative drawing code
    components/ui/     shadcn components (style base-mira, Base UI primitives), added with `bunx --bun shadcn@latest add`
    index.css          Tailwind v4 + design tokens (`ck-*`) mapped onto shadcn's semantic colours; fonts via fontsource
  test/                Vitest setup and helpers
```

Rules:

- **Typed IPC.** `lib/ipc.ts` defines `CratekitApi`: one function per Tauri command and one subscribe function per event. Stores and hooks call `api()`; only `lib/ipc.tauri.ts` calls `invoke()` / `listen()`. In a browser (`bun run --cwd ui dev`) and in the GitHub Pages demo, `api()` is the mock in `ui/src/mock/`, so the whole UI runs on fake data.
- **State.** Zustand stores per domain (`ui/src/stores/`). Rust events update stores through `hooks/useIpcEvents.ts`; components subscribe with selectors so they re-render only for what they show. The payload types in `lib/types.ts` are checked against the Rust structs (see IPC, "Generated types").
- **High-frequency data bypasses React state.** `playback_position` and `meter` arrive at 30 Hz. Their listeners write into refs, and canvas components redraw in a `requestAnimationFrame` loop. React state is for things that change at human speed (selected track, active tab, job list).
- **Event subscriptions live in hooks** with cleanup on unmount, so listeners never leak.

## IPC

**Commands (UI → Rust)**

| Command | Purpose |
| --- | --- |
| `app_version` | App version (first command; proves IPC works) |
| `open_settings_window` | Opens or focuses the native Settings window (also the app menu item "Settings...", Cmd/Ctrl+,) |
| `app_set_menu_labels(labels)` | Relabels every native menu item (Settings..., File, Edit, Window, undo to select all, minimize, close window, quit, and the macOS app menu items) and the Settings window title (`MenuLabels`) in the UI language; the UI owns the translations, Rust keeps the latest for windows opened later (English until the first call) |
| file and folder pickers | Through `tauri-plugin-dialog` (`dialog:allow-open` only), wrapped as `dialogOpenFiles` / `dialogOpenFolder` in `CratekitApi` |
| `library_add_paths(paths)` | Add files/folders, returns new track ids |
| `library_list(query)` | Tracks with filters (crate, search, BPM range, near a BPM with the tolerance decided in Rust, key match, has stems) |
| `library_counts` / `track_detail(id)` / `crates_list` | Sidebar counts; one track with analysis, cues and waveform; all crates |
| `setlist_transitions(ids)` | Per neighbour pair: `keyCompatible` (same key or a Camelot partner; null when a key is unknown), `bpmChangePct` (one decimal), `bpmJump` (over 6%) |
| `crate_create(name, color, ids)` / `crate_add_tracks(crateId, ids)` / `crate_remove_tracks(crateId, ids)` | Crates: create one holding `ids` in that order in one step (the set builder's "Save as crate"; an empty name is `invalid_name`), add tracks at the end in the given order (a crate lists in that order), take tracks out |
| `analyze_tracks(ids)` | Queue analysis |
| `job_cancel(job_id)` / `jobs_set_paused(paused)` | Cancel one job; pause or resume the queue |
| `playback_load(id) / playback_play / playback_pause / playback_seek(sec) / playback_set_rate(r)` | Deck |
| `playback_set_mode(mode)` | Original / Mastered / Stems |
| `playback_set_filter(value)` | DJ filter knob: -1 full low-pass, 0 off, +1 full high-pass (#56) |
| `playback_set_level_match(enabled)` | Master A/B compare at equal loudness (default on) |
| `stems_split(id)` / `stems_set_gain(stem, gain)` / `stems_set_mute(stem, bool)` / `stems_set_solo(stem, bool)` | Stems; `stems_split` rejects with `model_missing` until the model is downloaded |
| `stems_info(id)` / `stems_export(id)` | Split time, per-stem waveforms and the summed instrumental waveform (null when not split); one WAV per stem into the output folder, returns the paths |
| `master_presets` / `master_preview(id, settings)` / `master_export(id, settings)` | Mastering: presets with label, default target and a one-line description; B-side render for A/B; export into the output folder, returns path, LUFS and true peak |
| `hotcue_set(id, slot, sec)` / `hotcue_clear(id, slot)` / `hotcue_update(id, slot, name, color)` | Cues (set or move, clear, rename and recolour) |
| `midi_list_devices / midi_learn_start(target) / midi_learn_cancel / midi_mappings(device_id)` | Controller; `midi_mappings` returns one row per control target (`binding` null when unmapped) |
| `export_virtualdj(ids, dry_run)` | Export; each track's `changes` are data (`{ field: bpm \| key \| cues, value }`), worded by the UI |
| `models_status / models_download(name) / models_delete(name)` | ML models: status (`{name, sizeBytes, downloaded}`), download as a job (`kind: model-download`; asking again while it runs returns its job id; the only network call), delete (refused with `busy` while a download runs). Error codes `download` and `checksum` |
| `audio_list_devices / audio_set_device(id)` | Audio output: every output device (`id`, `name`, `isDefault`, `sampleRate`); choosing one saves it as the `audioDeviceId` setting and moves playback there; following the system default again is `settings_set("audioDeviceId", null)`. The buffer size is the `bufferSize` setting (`settings_set`); a change to either restarts a running engine on the new device or buffer, keeping the track, position and playing state |
| `settings_get / settings_set(key, value)` | Settings (UI `language`, `"en"` by default for every user; watched folders, output folder, processing backend, pane layouts, the set list order, ...). Rust owns the defaults and checks every value (`invalid_setting` for an unknown key, a wrong type or a buffer size other than 128, 256, 512 or 1024); stored in the library database, so they survive restarts |
| `stems_backend_status` | Active stems backend (CoreML / DirectML / CPU) and why, as a code (`no_gpu`, `cpu_chosen`, `gpu_check_failed`) the UI words |

**Events (Rust → UI)**: `job_progress {jobId, kind, label, stage, trackIds, done, total, queued}` (`done` and `total` are progress steps, not tracks: analysis counts 100 per track, the model download 1000 in all; `queued`: sent once when the job is queued, before it starts, so a waiting job can be seen and cancelled; `trackIds`: the tracks the job works on, empty for a model download; `stage`: `download` or `verify` for the model download, else null; `label` is an English fallback, the UI names jobs from `kind`, `stage` and the track count), `job_done {jobId, kind}`, `job_failed {jobId, kind, error}` (an analysis job skips tracks that fail and ends with `job_failed` saying how many; a cancelled job sends nothing more), `playback_position {sec}` (about 30 Hz, while playing or after a seek), `meter {peakDb, rmsDb}` (about 30 Hz, while there is sound), `playback_state {playing}` (today only `playing: false` when a track plays to its end: the UI sets the state itself after `playback_play` and `playback_pause`; play and pause that Rust starts itself, such as from MIDI, will send it too), `midi_device_changed`, `midi_learned {target, binding, replaced}` (`replaced`: the target that lost this control, or null), `control_event {target, value}` (Rust runs engine actions such as play, cue, hot cues, filter and tempo itself; the UI runs the UI-owned targets `mute-*`, `ab-toggle` and `pad-page` from this event), `audio_device_changed {id, reason}` (`selected`: the user picked it, or the picked device came back; `unplugged`: the device in use failed or disappeared and playback moved to the system default; `default-changed`: the system default changed while the app follows it; `lost`: the output failed and no other could open, nothing plays; `list-changed`: devices were plugged or unplugged, reload the list. The device list is checked about every 2 s), `settings_changed` (the full settings, after any change, so both windows stay in sync). Files dropped on a window arrive through Tauri's drag and drop event (`onFilesDropped` in `CratekitApi`).

**Naming convention:** command and event names are snake_case; payload fields are camelCase, so Rust payload structs use `#[serde(rename_all = "camelCase")]`. The TypeScript side of this contract is `ui/src/lib/types.ts` and `CratekitApi` in `ui/src/lib/ipc.ts`.

**Generated types (#16).** Every Rust payload struct and enum carries `#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]`. With the `ts` feature (on in `--all-features` test runs, never in the app), the tests write one TypeScript file per type to `ui/src/lib/bindings/` (settings in `.cargo/config.toml`: the folder, and `i64` as `number`). Doc comments carry over. `ui/src/lib/bindings.test.ts` then makes `tsc` prove the contract in the direction the data flows: each Rust payload (command result or event) fits its UI type, and each UI argument fits its Rust type. So a renamed, removed or retyped Rust field fails `bun run typecheck`, while the UI can stay ahead of Rust for features not built yet. CI fails when the committed bindings differ from what the tests write.

- After changing a payload type: `bun run test:rust`, then commit `ui/src/lib/bindings/` with the change. A new payload type also gets a line in `bindings.test.ts`.
- Use Rust enums, not strings, for values the UI matches on (`JobKind`), so the generated type is a union. Where a plain Rust type is narrower in practice, say so with `#[cfg_attr(feature = "ts", ts(type = "..."))]` and the reason (`Cue.slot` is 1 to 8).
- ts-rs never deletes files: when a payload type is removed or renamed, delete its old file. CI clears the folder before its tests, so a leftover file fails the check.
- Commands themselves (`CratekitApi`) stay hand-written: the UI and its mock cover commands Rust does not have yet.

## Database (SQLite via rusqlite)

- **One file**, `cratekit.db`, in the app data folder. `rusqlite` with the `bundled` feature compiles SQLite into the binary, so there is nothing to install.
- **WAL mode** (`PRAGMA journal_mode=WAL`) and `PRAGMA foreign_keys=ON` on every connection. WAL lets the UI read the library while a job writes results.
- **One writer.** All writes go through a single connection: the app keeps one `Library` behind a lock, and commands use it on a blocking worker thread (the analysis job worker takes the same lock briefly to read a track's path and to store its result, never while decoding). Reads share that connection for now; adding reads the files without holding it. Separate read connections can come later if reads ever wait too long.
- **Migrations** are numbered SQL steps in `library/schema.rs`, applied at startup in a transaction each and tracked with `PRAGMA user_version`; a database from a newer Cratekit is refused (`database_too_new`). Our own small runner, no migrations crate.
- `rusqlite` is synchronous on purpose: jobs already run on worker threads, and sync code is simpler to write and test in `cratekit-core`.

```sql
CREATE TABLE tracks (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  file_hash TEXT NOT NULL,          -- detect moved/changed files
  title TEXT, artist TEXT, album TEXT,
  duration_sec REAL, sample_rate INTEGER, channels INTEGER,
  added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE analysis (
  track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
  bpm REAL, bpm_confidence REAL,
  key_tonic INTEGER, key_mode TEXT, camelot TEXT, key_confidence REAL,
  energy INTEGER, lufs REAL, true_peak_db REAL,
  first_beat_sec REAL,
  waveform BLOB,                     -- min/max/RMS columns, one byte each (analysis/waveform.rs)
  analyzer_version INTEGER NOT NULL,
  analyzed_at TEXT NOT NULL,
  chroma BLOB                        -- migration 2: 12 little-endian f32, C to B
);
CREATE TABLE cues (
  track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
  slot INTEGER,                      -- 1..8
  position_sec REAL NOT NULL, color TEXT, name TEXT,
  PRIMARY KEY (track_id, slot)
);
CREATE TABLE stems (
  track_id INTEGER PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
  model TEXT NOT NULL, dir TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE masters (
  id INTEGER PRIMARY KEY, track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE,
  preset TEXT, settings_json TEXT, path TEXT, lufs REAL, created_at TEXT
);
CREATE TABLE crates (id INTEGER PRIMARY KEY, name TEXT NOT NULL, color TEXT, sort INTEGER);
CREATE TABLE crate_tracks (crate_id INTEGER REFERENCES crates(id) ON DELETE CASCADE,
  track_id INTEGER REFERENCES tracks(id) ON DELETE CASCADE, sort INTEGER,
  PRIMARY KEY (crate_id, track_id));
CREATE TABLE midi_mappings (device TEXT, target TEXT, kind TEXT, channel INTEGER, number INTEGER,
  PRIMARY KEY (device, target));
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);
CREATE INDEX masters_track ON masters(track_id);
CREATE INDEX crate_tracks_track ON crate_tracks(track_id);
```

Bump `analyzer_version` when analysis code changes, so old results are recomputed lazily.

## Files on disk

App data folder (Tauri `app_data_dir`):

```
cratekit.db
models/htdemucs/...          downloaded once, checksum verified
stems/<track_id>/{vocals,drums,bass,other}.wav
masters/<track_id>/<timestamp>-<preset>.wav
backups/virtualdj/<timestamp>/  copies made before any export
```

## Mastering chain

```
decode → resample to 48 kHz (or keep source rate) → [reference match EQ] → preset EQ
       → glue compressor → makeup gain (2-pass loudness targeting) → true-peak limiter (−1 dBTP)
       → [TPDF dither if 16-bit] → WAV
```

Loudness targeting: render once, measure integrated LUFS, adjust makeup gain, render again (max 3 passes) until within ±0.3 LU.

**Presets (starting values, to tune by ear with the test DJ)**

| Preset | Low shelf | Mid peak | High shelf | Comp threshold / ratio / attack / release | Target |
| --- | --- | --- | --- | --- | --- |
| Club | 90 Hz +3 dB | 400 Hz −2 dB, Q 0.9 | 9 kHz +2.5 dB | −18 dB / 3:1 / 10 ms / 200 ms | −8 LUFS |
| Hip-hop | 70 Hz +4 dB | 300 Hz −1.5 dB, Q 0.9 | 10 kHz +1.5 dB | −20 dB / 3.5:1 / 15 ms / 250 ms | −9 LUFS |
| Pop | 100 Hz +1.5 dB | 2.5 kHz +1.5 dB, Q 0.8 | 12 kHz +3 dB | −16 dB / 2.5:1 / 10 ms / 150 ms | −9 LUFS |
| Lo-fi | 120 Hz +2 dB | 1 kHz −1 dB, Q 0.7 | 6 kHz −6 dB (+ LP 5.5 kHz) | −22 dB / 4:1 / 20 ms / 300 ms | −12 LUFS |
| Chill | 80 Hz +1 dB | 3 kHz −1 dB, Q 0.8 | 10 kHz +1 dB | −14 dB / 2:1 / 20 ms / 300 ms | −12 LUFS |

Intensity `k` (0 to 1): EQ gain × k, ratio = 1 + (ratio − 1) × k, makeup interpolated.

**Reference matching**: compute the long-term average spectrum (1/3-octave bands, 31 bands) of the track and of the reference; EQ curve = difference, smoothed across neighbours, clamped to ±6 dB, applied with a linear-phase FIR or a bank of peaking biquads.

## Analysis notes

- `stratum-dsp` is wrapped behind the `TempoDetector` / `KeyDetector` traits (`StratumTempo`, `StratumKey`, `stratum` cargo feature, off by default). Measured on 16 synthetic loops of 30 s (release): ours 16/16 tempo and 16/16 key in 1.9 s, stratum-dsp 12/16 and 11/16 in 10.4 s; it also doubled a 75 BPM groove and reported a tempo for a steady drone. So our detectors are the default; re-measure on the test DJ's real tracks (#22) before deciding again. Its beat grid is not used yet.
- Our detectors are the default: `OnsetTempo` (onset-energy autocorrelation, refined by a beat-grid fit) and `ProfileKey` (chroma + Krumhansl-Kessler profiles, using power not magnitude so bass overtones do not bias the mode). On the synthetic loops at 92/124/128/140 BPM they meet the SPEC targets, and in release they analyze 90 s of audio in about 0.35 s. See `docs/analysis.md`.
- `analyze_file` measures tempo and key on the middle 90 s of the track (faster, and intros and outros are often beatless); loudness and energy use the whole track. Analysis jobs run one at a time on the job worker; a 6 minute track takes about 1 s in release.

## Stems notes

Decided in #29 (research: ONNX Runtime per target, CoreML, DirectML and Windows ML, the htdemucs model; 2026-11-19). Linking and checks are #115, Intel Macs #114, the model download #30, the separator #31, backend selection #32.

**Runtime.** The `ort` crate used directly (not `stem-splitter-core`), with `default-features = false`, `load-dynamic` and `api-23`, plus the `coreml` feature on macOS only. Telemetry off (`with_telemetry(false)`).
- `load-dynamic`: the app starts even when ONNX Runtime is missing or cannot load on this OS or CPU, and stems say why. `ort::util::probe` checks the library's version before loading it.
- `api-23`: one feature set that works with every library below (the ONNX Runtime C API is backward compatible, so a 1.32 library serves API 23). htdemucs needs nothing newer.
- Not pyke's prebuilt static binaries: they need an x86-64-v3 CPU and glibc 2.39, and have no Intel macOS build.
- Not `stem-splitter-core`: it skips the weighted overlap-add and the whole-track normalization (measured 0.2 to 0.3 dB SDR below the reference), pins an old `ort` exactly, and pulls `reqwest` and `clap`.

**ONNX Runtime library per target** (Microsoft's official builds, MIT, **bundled in the installer**, so nothing executable is downloaded):

| Target | Library | Execution providers, in order | Notes |
| --- | --- | --- | --- |
| macOS arm64 | ONNX Runtime 1.32 (`libonnxruntime.dylib`, about 13 MB compressed) | CoreML, then CPU | `minos` 14.0, under the app's 14.2 |
| macOS x86_64 | ONNX Runtime 1.23.2, the last Intel macOS build (about 11 MB) | CPU | `minos` 13.4; frozen since Oct 2025. Loads and runs a model on `macos-15-intel` in CI (#114). A source build of a newer version is a possible follow-up |
| Windows x64, ARM64 | ONNX Runtime 1.32 CPU (`onnxruntime.dll`, about 7 MB) | CPU | No GPU in v0.1: every DirectML route (also Windows ML) ships Microsoft's proprietary `DirectML.dll`, whose licence includes data collection |
| Linux x64, arm64 | ONNX Runtime 1.32 (`libonnxruntime.so`, about 11 to 13 MB) | CPU | Needs glibc 2.28, fine on Ubuntu 22.04. ROCm was removed from ONNX Runtime, CUDA is far over budget; WebGPU (Vulkan) is the only GPU path worth a later test |

- Bundling adds about 25 MB to the universal dmg, 7 MB to the msi and 11 to 13 MB to the Linux packages. The AppImage then reaches about 90 to 94 MB: its budget is revisited when the library lands (#115).
- **CPU baseline:** Microsoft's builds pick SIMD at run time, so they should also run on CPUs without AVX2 (slowly); not verified yet. Until it is, check `is_x86_feature_detected!("avx2")` and `"fma"` before loading and report "stems need a newer CPU" (#115).

**CoreML (Apple Silicon).** MLProgram format, compute units `CPUAndGPU` (the Neural Engine was slower than the GPU), static input shapes, and a model cache folder in Cratekit's data folder (a cold CoreML compile takes about 27 s, about 7 s with the cache). It works only with a "split" export: the STFT and inverse STFT run in Rust, outside the graph, and the time-branch convolutions are tiled to at most 16,384 samples so the whole graph is one CoreML partition. Measured (Charon, M4 Pro, 193 s track): 16.6 s on CPU, 5.6 s on CoreML, CoreML within 1.3e-5 of PyTorch.

**Model.** htdemucs, 4 stems (vocals, drums, bass, other; 2-stem mode sums the other three), as a split ONNX export of about 170 to 185 MB. Not htdemucs_ft (4 models, 4 times slower) and not htdemucs_6s (its weights are likely non-commercial).
- **Licence (decided 2026-11-19).** The Demucs code is MIT, but its author states that "the license of the weights is not clear" (part of the training data was for research use only). So Cratekit **never bundles or hosts the weights**. Stems stay off until the user chooses to download the model; the dialog says where it comes from and that its licence is unclear (personal use), and the download is verified against a pinned SHA-256 (#30). The source, settled in #30: **Intel's split export** on Hugging Face (Intel/demucs-openvino, `htdemucs_v4/htdemucs_fwd.onnx` plus `htdemucs_fwd.onnx.data`, 171 MB, labelled MIT by its uploader), pinned to one revision. In a spike on an M4 Pro it was the only export fast on both backends: 0.25 s per 7.8 s segment on CoreML (ONNX Runtime 1.32) and 0.85 s on CPU (1.23.2); the single-file StemSplitio export took 53 s per segment on CoreML.
- **Input layout our chunking implements** (from adefossez/demucs `apply.py`, `htdemucs.py`, `spec.py`):
  - 44.1 kHz stereo (resample with rubato; mono is duplicated).
  - Whole-track normalization: subtract the mean of the mono mix and divide by its standard deviation (plus 1e-8); undone on the output.
  - Segments of 7.8 s (343,980 samples), overlap 25% (stride 257,985), triangular weights; outputs summed with the weights and divided by the summed weight; the last segment is zero-padded and cropped. No random shifts (deterministic).
  - STFT: n_fft 4096, hop 1024, Hann, normalized, centred with reflect padding, plus the model's own padding; the last bin is dropped (2048 bins, 336 frames), complex values as channels, giving `[1, 4, 2048, 336]`. The inverse STFT output is added to the time-branch output `[1, 4, 2, 343980]`. Stem order: drums, bass, other, vocals.
- **Memory:** the session runs with ONNX Runtime's memory pattern off: it otherwise keeps every planned buffer for the whole run. Measured on an M4 Pro, splitting a 236 s track: 3.3 GB peak for the whole process (5.5 GB with the pattern on), at the same speed (34 s), within the SPEC's 4 GB. Still to check on a real 8 GB machine.

**Speed per 4-minute track** (measured, or scaled from measurements): Apple Silicon about 21 s on CPU and 7 s on CoreML; a modern x86 desktop about 60 s on CPU (inference); older laptops and Intel Macs 2 to 4 minutes (inference). Stems run as a background job with progress, which the DJ research found acceptable at 30 to 60 s per track. Not measured anywhere yet: ONNX Runtime on an x86 laptop and on Linux.

**Backend selection (#32).** At startup, the app tries the providers in the order above and keeps the first that initializes. The first split on CoreML is checked against a short CPU reference chunk; if the difference exceeds the SPEC threshold, the app switches to CPU and records why. Settings can force "CPU only"; the status bar shows the active backend. Only stems use the GPU: analysis, mastering and playback run on the CPU.

**Rust-only alternatives** (to re-check later, mainly for Intel Macs or a GPU on Windows without proprietary files): the Burn port demucs-rs (Apache-2.0, wgpu) still produces NaN on some tracks; candle, tract and burn-onnx have no htdemucs port or import yet.

## Tooling

| Area | Tools | Where configured |
| --- | --- | --- |
| Rust toolchain | rustc 1.100 (edition 2024), rustfmt, clippy, rust-analyzer, llvm-tools | `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `[workspace.lints]` in `Cargo.toml` |
| Rust checks | cargo-nextest, cargo-deny, cargo-shear, cargo-llvm-cov, taplo, typos, bacon | `.config/nextest.toml`, `deny.toml`, `.taplo.toml`, `_typos.toml`, `bacon.toml` |
| UI checks | TypeScript 7 (`tsc`), oxlint + tsgolint, Prettier, knip, Vitest | `ui/tsconfig.json`, `.oxlintrc.json`, `.prettierrc`, `knip.json`, `ui/vite.config.ts` |
| Git hooks | husky, lint-staged, commitlint | `.husky/`, `package.json` (`lint-staged`), `commitlint.config.ts` |
| CI | GitHub Actions, actions pinned by SHA | `.github/workflows/ci.yml` |
| Stems runtime | Loads Microsoft's ONNX Runtime on every target and runs a tiny model; manual, weekly and on stems runtime changes (#115) | `.github/workflows/stems-runtime.yml`, `.github/scripts/fetch-onnxruntime.sh` |
| Installers | Unsigned `tauri build` per target, checked against the minimum OS versions and the size budget; manual, weekly and on bundling changes (#113) | `.github/workflows/installers.yml`, `.github/scripts/check-installer.sh` |

`bun run check` runs every gate in the same order as the pre-push hook and CI.

## Platforms and portability

| OS | Rust targets (tier) | Minimum OS | Package |
| --- | --- | --- | --- |
| macOS | `aarch64-apple-darwin` (1) + `x86_64-apple-darwin` (2), combined as `universal-apple-darwin` | 14.2 | dmg |
| Windows | `x86_64-pc-windows-msvc` (1), `aarch64-pc-windows-msvc` (1) | 10 22H2 (x86_64), 11 (ARM64) | msi |
| Linux | `x86_64-unknown-linux-gnu` (1), `aarch64-unknown-linux-gnu` (1) | Ubuntu 22.04, Debian 12 | deb and AppImage |

Why these minimums:

- **macOS 14.2:** cpal 0.18 links Core Audio process-tap functions that exist only from 14.2. With a deployment target of 12 or later they bind at launch, so an older macOS would refuse to start the app with "Symbol not found". ONNX Runtime also needs 14.0. `bundle.macOS.minimumSystemVersion` in `tauri.conf.json` is `14.2`; Tauri passes it to release builds as `MACOSX_DEPLOYMENT_TARGET`, and macOS shows a clear "needs a newer macOS" message instead of crashing.
- **Windows:** Rust's MSVC targets need Windows 10. WebView2 is part of Windows 10 (1803+) and 11, and Microsoft updates it on 10 22H2 until at least October 2028. The installer keeps Tauri's default WebView2 mode (download only if missing); the offline WebView2 installer would add about 127 MB and break the 60 MB budget. Windows on ARM devices run Windows 11 in practice.
- **Linux:** Tauri 2 needs WebKitGTK 4.1 (Ubuntu 22.04+, Debian 12+; not Debian 11). An AppImage needs a glibc at least as new as the build machine's, so **installers are built on `ubuntu-22.04` and `ubuntu-22.04-arm`**, the oldest base we support (CI tests run on 24.04).

Native dependencies per target:

| Dependency | Kind | All six targets? |
| --- | --- | --- |
| Tauri 2 (WKWebView, WebView2, WebKitGTK 4.1) | OS webview | Yes |
| tauri-plugin-dialog (AppKit, common controls, GTK3) | OS dialogs | Yes |
| cpal (CoreAudio, WASAPI, ALSA) | OS audio API | Yes (ALSA reaches PipeWire and PulseAudio through their ALSA bridge) |
| midir (CoreMIDI, WinMM, ALSA), planned | OS MIDI API | Yes |
| SQLite (rusqlite `bundled`) | C, compiled by `cc` | Yes |
| symphonia, rubato, rustfft, ebur128 | Pure Rust | Yes (rustfft picks SIMD at run time) |
| ONNX Runtime (stems), planned | Prebuilt C++ library, loaded at run time | Yes, with Microsoft's builds: 1.32 everywhere except Intel Macs (1.23.2, the last Intel build). See Stems notes |

Stems and ONNX Runtime: decided in #29, see **Stems notes** (Microsoft's builds loaded at run time, bundled per target). The `Stems runtime` workflow proves every library on its target, including 1.23.2 on the Intel Mac (#114), Ubuntu 22.04 (glibc) and Windows Server 2022 (#115). Still to check on real hardware: a Windows 10 22H2 machine and a CPU without AVX2.

Rules:

- **Portable code only.** No architecture-specific intrinsics (`std::arch::x86_64::*`, NEON) unless behind runtime feature detection with a scalar fallback, and with a test for both paths. Prefer plain loops over slices (`chunks_exact`, iterators) that the compiler auto-vectorizes on both x86_64 and arm64.
- **Native dependencies must exist for every target.** Before adopting a crate with prebuilt binaries, check the binaries per target, their minimum OS and glibc, and their CPU baseline (for example x86-64-v3), and record the result in the table above. SQLite (rusqlite `bundled`) compiles from source, so it works everywhere.
- **Floating point:** do not rely on CPU-specific flush-to-zero modes; flush denormals in code inside the DSP and audio callback paths.
- **CI builds and tests natively on each architecture** (GitHub-hosted macOS arm64 and Intel, Windows x64 and ARM64, Linux x64 and arm64 runners), not only cross-compiles. The Intel macOS runner (`macos-15-intel`) is GitHub's last one and retires around fall 2027; after that, Intel builds are cross-compiled.
- **Installer checks (`installers.yml`, #113):** every target builds its installer (macOS universal dmg, msi on x64 and ARM64, .deb and AppImage on x64 and arm64), and `check-installer.sh` fails the job when:
  - macOS: any architecture of the app binary, or `Info.plist`, has a minimum other than `bundle.macOS.minimumSystemVersion`;
  - Linux: any binary in the AppImage or the .deb needs a glibc newer than 2.35 (Ubuntu 22.04), or the .deb does not depend on ALSA (`libasound2`, or `libasound2t64` on Ubuntu 24.04 and Debian 13), which audio output loads at run time;
  - Windows: the app imports a Visual C++ runtime DLL (Tauri links it statically, so users never install a redistributable).

  Each job reports each installer's size against its budget (90 MB for the AppImage, which bundles WebKitGTK; 60 MB for the others; a warning above it) and keeps the unsigned installer as a workflow artifact for 14 days. Whether a macOS library uses an API newer than the minimum cannot be checked from the binary; the launch tests cover that.
- **Not tested in CI yet:** launching on the minimum OS versions (manual, #116), real audio output (runners have no audio device) and GPU providers.

## MIDI notes

- `midir` input per device; parse status byte: `0x90` Note On, `0x80` Note Off, `0xB0` CC.
- Binding = `{ kind: note|cc, channel, number }`. Button trigger: Note On velocity > 0, or CC rising through 64. Continuous: CC value / 127.
- Some jog wheels send relative CC (64 ± n). Detect and treat as relative for the seek/nudge target.
- MIDI output for pad LEDs: send Note On with velocity = colour/brightness value per controller mapping.
