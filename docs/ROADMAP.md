# Cratekit: Roadmap

v0.1 in about 8 weeks of part-time work. Each phase ends with a gate: the test DJ (VirtualDJ user, Pioneer DDJ-FLX4) must pass it before the next phase starts. Tick tasks as they are done.

## Phase 0a: Dev tooling

- [x] Rust toolchain (rustup, rustfmt, clippy, rust-analyzer), cargo-deny, TS language server
- [x] Git repo + private GitHub repo (`mohamedboukari/cratekit`), `.gitignore` (no test audio, no VirtualDJ files)
- [x] Docs revised: React 19 + Vite UI, bun, SQLite/rusqlite notes, platforms + GPU, 8 hot cues, Settings module, no em dashes
- [x] `CHANGELOG.md` and PR template
- [x] Custom Claude skills: `ship`, `realtime-audio`, `license-check`
- [x] `.claude/settings.json` (allowlist, em-dash hook, project plugins), `.mcp.json` (context7), LSP plugins (rust-analyzer, TypeScript), Vercel plugins disabled for this project
- [x] graphify: skill, CLAUDE.md section, hook-guard hooks, git hooks, first graph of the docs
- [x] Vendored skills: `rust-best-practices` (BeautifulRust + Apollo), `rust-review`, superpowers (debugging, TDD, verification, writing-plans), `interface-design`, React rules, interface guidelines, shadcn
- [x] Custom `desktop-pro-ui` skill
- [ ] User-level plugins (impeccable, playground, Trail of Bits)
- [x] Open source: fresh public repo (history kept in a private archive), SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md, issue templates, Dependabot, CodeQL, gitleaks, zizmor, dependency review, secret scanning, ruleset (PR + green CI)
- [ ] GitHub issues and milestones for every ROADMAP task
- [ ] Redesign `docs/ui-preview.html` and `docs/UI.md` (laptop-first panes, missing v0.1 screens, SPEC parity, accessibility, native titlebar, Tailwind v4 + shadcn tokens)

## Phase 0b: Validate (week 1)

- [ ] Send `docs/ui-preview.html` to the test DJ
- [ ] Get answers: DJ software, what he pays for, biggest annoyance, mastering habits, how he gets acapellas, laptop specs (OS, RAM, GPU)
- [ ] Get a copy of his VirtualDJ database file and 10 tracks he owns, for testing (keep them in `test-data/`, out of git)
- [ ] Decide the first module after analysis (mastering or stems)

**Gate:** the DJ picks the module he wants most.

## Phase 1: Core + analysis (weeks 2 to 3)

- [x] Cargo workspace: `cratekit-core`, `cratekit-audio`, `src-tauri` (edition 2024, pinned toolchain, workspace lints)
- [x] UI scaffold in `ui/`: React 19 + TypeScript 7 + Vite 8 with bun; Vitest; Tailwind v4 + shadcn (Base UI); oxlint, Prettier, knip; husky + lint-staged + commitlint
- [ ] Choose (license check, ask first): state library (if any), table virtualization
- [x] `deny.toml` (license allow-list, see the `license-check` skill)
- [x] CI: fmt, clippy, tests, `cargo deny check`, UI type-check + Vitest, run natively on macOS arm64 + x86_64, Windows x86_64 + ARM64, Linux x86_64 + arm64 (GitHub Actions; every PR on all six targets)
- [ ] Confirm minimum OS versions (Tauri 2, ONNX Runtime) and that every native dependency has builds for all six targets
- [ ] Decode any supported file to `AudioBuffer` (symphonia) + resample (rubato)
- [ ] Synthetic audio test helpers (kick at BPM, chords in a key, sine at level)
- [ ] `TempoDetector` + `KeyDetector` traits; stratum-dsp implementation; fallback implementation
- [ ] Camelot mapping + `matches()` with tests for every key
- [ ] Loudness (LUFS, true peak) + energy score
- [ ] Waveform columns
- [ ] SQLite (rusqlite, WAL, single writer) schema + migrations; add files/folders; tag reading
- [ ] Background analysis queue with progress events
- [ ] UI: typed IPC layer (`ui/src/lib/ipc.ts`)
- [ ] UI: window layout from `ui-preview.html` (sidebar, deck, tabs, track browser, status bar)
- [ ] UI: Analyze tab (key, chroma bars, mixes-well-with chips)
- [ ] Playback: load, play/pause, seek, waveform playhead (canvas, outside React renders)
- [ ] Audio output device selection + hot-plug fallback to the system default
- [ ] UI: Settings screen (audio output, watched folders, output folder)
- [ ] Test analysis on the DJ's 10 tracks; compare with VirtualDJ's BPM/key

**Gate:** runs on a laptop CPU; BPM and key agree with VirtualDJ on at least 8 of 10 tracks.

## Phase 2: Mastering + stems (weeks 4 to 6)

Mastering
- [ ] Biquad EQ, compressor, true-peak limiter with tests
- [ ] Presets + intensity
- [ ] 2-pass loudness targeting, export WAV 24/16-bit (dither)
- [ ] Real-time preview chain in `cratekit-audio`; level-matched A/B
- [ ] Reference-track matching
- [ ] UI: Master tab

Stems
- [ ] **Research:** ONNX Runtime GPU backends for htdemucs via `ort`: CoreML (macOS), DirectML vs Windows ML (Windows); prebuilt binaries per target; record findings in `docs/ARCHITECTURE.md`
- [ ] Model download with progress + checksum
- [ ] `StemSeparator` trait; htdemucs implementation (stem-splitter-core)
- [ ] Backend selection: GPU provider with CPU fallback, CPU reference check, "CPU only" setting, backend in the status bar; verified on real hardware (Apple Silicon, Windows GPU, a CPU-only machine)
- [ ] Cancel + progress; cache per track
- [ ] Stem mixer in the audio engine (gain, mute, solo)
- [ ] UI: Stems tab; export WAVs

**Gate:** the DJ uses at least one master or one acapella in a real set or edit.

## Phase 3: Controller + launch (weeks 7 to 8)

- [ ] MIDI device list + hot-plug
- [ ] MIDI learn + mapping storage
- [ ] Targets: play, cue, hot cues 1 to 8 (pad page 1 to 4 / 5 to 8), stem pads, filter, tempo, volume, A/B
- [ ] Pad LED feedback for the DDJ-FLX4
- [ ] Keyboard shortcuts
- [ ] Hot cue editing on the waveform (set, move, colour, name)
- [ ] **Research:** document the VirtualDJ database format from the DJ's real file (song entries, BPM, key, cue points); write findings in `docs/virtualdj-format.md`
- [ ] VirtualDJ export with backup + dry-run
- [ ] Installers via Tauri bundler: macOS universal dmg, Windows msi (x86_64 + ARM64), Linux AppImage (x86_64 + arm64); installer size checked against the 60 MB budget per target
- [ ] `release` Claude skill (version bump, CHANGELOG cut, tag, signed installers)
- [ ] README in BunMail's style: pitch, "Why Cratekit?" comparison table, quick start, honest "How good is the mastering?" section, screenshots
- [ ] Launch: Show HN, r/DJs, r/WeAreTheMusicMakers, VirtualDJ forum

**Gate:** the DJ uses Cratekit every week to prepare his sets → tag **v0.1** and publish.

## Later

- Sample library (index folders, search by BPM / key / type, preview)
- Auto cue points (drop, breakdown, outro)
- 6 stems, lead vs backing vocals, NI STEM export
- Rekordbox XML and Serato export
- Ready-made mappings for more controllers (Numark, Hercules, Pioneer DDJ-400/SB3)
- Batch mastering of a crate
