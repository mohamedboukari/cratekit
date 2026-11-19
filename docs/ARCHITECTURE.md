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
    mod.rs             trait StemSeparator; split() -> StemSet
    htdemucs.rs        impl using stem-splitter-core (or ort directly)
    models.rs          download + checksum + cache path
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
  filter.rs      DJ filter (one knob: LP below centre, HP above) (planned)
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
      types.ts         TS types mirroring the Rust command/event payloads
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
- **State.** Zustand stores per domain (`ui/src/stores/`). Rust events update stores through `hooks/useIpcEvents.ts`; components subscribe with selectors so they re-render only for what they show. The payload types in `lib/types.ts` mirror the Rust structs (later we can generate them from Rust with a crate such as `ts-rs` or `specta`, after a license check).
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
| `crate_create / crate_add_tracks / crate_remove_tracks` | Crates |
| `setlist_transitions(ids)` | Set builder: for each neighbour pair, `keyCompatible` (null when either key is unknown), `bpmChangePct` and `bpmJump` (Rust decides the thresholds) (#55) |
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
| `models_status / models_download(name) / models_delete(name)` | ML models |
| `audio_list_devices / audio_set_device(id)` | Audio output: every output device (`id`, `name`, `isDefault`, `sampleRate`); choosing one saves it as the `audioDeviceId` setting and moves playback there; following the system default again is `settings_set("audioDeviceId", null)`. The buffer size is the `bufferSize` setting (`settings_set`); a change to either restarts a running engine on the new device or buffer, keeping the track, position and playing state |
| `settings_get / settings_set(key, value)` | Settings (UI `language`, `"en"` by default for every user; watched folders, output folder, processing backend, pane layouts, the set list order, ...). Rust owns the defaults and checks every value (`invalid_setting` for an unknown key, a wrong type or a buffer size other than 128, 256, 512 or 1024); stored in the library database, so they survive restarts |
| `stems_backend_status` | Active stems backend (CoreML / DirectML / CPU) and why, as a code (`no_gpu`, `cpu_chosen`, `gpu_check_failed`) the UI words |

**Events (Rust → UI)**: `job_progress {jobId, kind, label, stage, trackIds, done, total, queued}` (`done` and `total` are progress steps, not tracks: analysis counts 100 per track; `queued`: sent once when the job is queued, before it starts, so a waiting job can be seen and cancelled; `trackIds`: the tracks the job works on, empty for a model download; `stage`: `download` or `verify` for the model download, else null; `label` is an English fallback, the UI names jobs from `kind`, `stage` and the track count), `job_done {jobId, kind}`, `job_failed {jobId, kind, error}` (an analysis job skips tracks that fail and ends with `job_failed` saying how many; a cancelled job sends nothing more), `playback_position {sec}` (about 30 Hz, while playing or after a seek), `meter {peakDb, rmsDb}` (about 30 Hz, while there is sound), `playback_state {playing}` (today only `playing: false` when a track plays to its end: the UI sets the state itself after `playback_play` and `playback_pause`; play and pause that Rust starts itself, such as from MIDI, will send it too), `midi_device_changed`, `midi_learned {target, binding, replaced}` (`replaced`: the target that lost this control, or null), `control_event {target, value}` (Rust runs engine actions such as play, cue, hot cues, filter and tempo itself; the UI runs the UI-owned targets `mute-*`, `ab-toggle` and `pad-page` from this event), `audio_device_changed {id, reason}` (`selected`: the user picked it, or the picked device came back; `unplugged`: the device in use failed or disappeared and playback moved to the system default; `default-changed`: the system default changed while the app follows it; `lost`: the output failed and no other could open, nothing plays; `list-changed`: devices were plugged or unplugged, reload the list. The device list is checked about every 2 s), `settings_changed` (the full settings, after any change, so both windows stay in sync). Files dropped on a window arrive through Tauri's drag and drop event (`onFilesDropped` in `CratekitApi`).

**Naming convention:** command and event names are snake_case; payload fields are camelCase, so Rust payload structs use `#[serde(rename_all = "camelCase")]`. The TypeScript side of this contract is `ui/src/lib/types.ts` and `CratekitApi` in `ui/src/lib/ipc.ts`; keep the three in sync.

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

- `stem-splitter-core` (Rust + ONNX Runtime, htdemucs) is the first choice: it handles model download and caching.
- If it does not fit, use the `ort` crate with an htdemucs ONNX export (Apache-2.0 conversion of Demucs), chunking ~7.8 s windows (343,980 samples at 44.1 kHz) with overlap-add.
- Backup option: the Burn-based Rust reimplementation of HTDemucs.
- **Execution providers (GPU with CPU fallback).** At startup, `stems/models.rs` builds a priority list and keeps the first provider that initializes:
  1. macOS: **CoreML** (GPU and Neural Engine on Apple Silicon).
  2. Windows: **DirectML** (any DirectX 12 GPU: NVIDIA, AMD, Intel, Qualcomm). Microsoft has moved DirectML to maintenance in favour of Windows ML; re-check which one `ort` supports best in the Phase 2 research.
  3. **CPU** always last, always available.
- The first split on a GPU backend is checked against a short CPU reference chunk. If the residual exceeds the SPEC threshold, the app switches to CPU and records why. Settings can force "CPU only"; the status bar shows the active backend.
- CUDA is not bundled (its libraries are hundreds of MB, far over the installer budget). It could become an optional download later.
- Verify each provider on real hardware before enabling it by default.
- Only stems use the GPU. Analysis, mastering and playback run on the CPU.

## Tooling

| Area | Tools | Where configured |
| --- | --- | --- |
| Rust toolchain | rustc 1.100 (edition 2024), rustfmt, clippy, rust-analyzer, llvm-tools | `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `[workspace.lints]` in `Cargo.toml` |
| Rust checks | cargo-nextest, cargo-deny, cargo-shear, cargo-llvm-cov, taplo, typos, bacon | `.config/nextest.toml`, `deny.toml`, `.taplo.toml`, `_typos.toml`, `bacon.toml` |
| UI checks | TypeScript 7 (`tsc`), oxlint + tsgolint, Prettier, knip, Vitest | `ui/tsconfig.json`, `.oxlintrc.json`, `.prettierrc`, `knip.json`, `ui/vite.config.ts` |
| Git hooks | husky, lint-staged, commitlint | `.husky/`, `package.json` (`lint-staged`), `commitlint.config.ts` |
| CI | GitHub Actions, actions pinned by SHA | `.github/workflows/ci.yml` |

`bun run check` runs every gate in the same order as the pre-push hook and CI.

## Platforms and portability

| OS | Rust targets | Package |
| --- | --- | --- |
| macOS | `aarch64-apple-darwin` + `x86_64-apple-darwin`, combined as `universal-apple-darwin` | dmg |
| Windows | `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` | msi |
| Linux | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | AppImage |

Rules:

- **Portable code only.** No architecture-specific intrinsics (`std::arch::x86_64::*`, NEON) unless behind runtime feature detection with a scalar fallback, and with a test for both paths. Prefer plain loops over slices (`chunks_exact`, iterators) that the compiler auto-vectorizes on both x86_64 and arm64.
- **Native dependencies must exist for every target**: check ONNX Runtime prebuilt binaries per target before adopting a version. SQLite (rusqlite `bundled`) compiles from source, so it works everywhere.
- **Floating point:** do not rely on CPU-specific flush-to-zero modes; flush denormals in code inside the DSP and audio callback paths.
- **CI builds and tests natively on each architecture** (GitHub-hosted macOS arm64, Windows ARM64 and Linux arm64 runners exist), not only cross-compiles.

## MIDI notes

- `midir` input per device; parse status byte: `0x90` Note On, `0x80` Note Off, `0xB0` CC.
- Binding = `{ kind: note|cc, channel, number }`. Button trigger: Note On velocity > 0, or CC rising through 64. Continuous: CC value / 127.
- Some jog wheels send relative CC (64 ± n). Detect and treat as relative for the seek/nudge target.
- MIDI output for pad LEDs: send Note On with velocity = colour/brightness value per controller mapping.
