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
- [x] GitHub issues and milestones for every ROADMAP task (#1 to #52)
- [x] UI redesign as the real React app with a mock API and a GitHub Pages demo; `docs/UI.md` rewritten, the HTML mockup retired (#1)

## Phase 0b: Validate (week 1)

- [ ] Send the UI to the test DJ (first the HTML mockup, now the Pages demo) (#2)
- [ ] Get answers: DJ software, what he pays for, biggest annoyance, mastering habits, how he gets acapellas, laptop specs (OS, RAM, GPU) (#3)
- [ ] Get a copy of his VirtualDJ database file and 10 tracks he owns, for testing (keep them in `test-data/`, out of git) (#4)
- [x] Decide the first module after analysis (mastering or stems): stems first (#5)

**Gate:** the DJ picks the module he wants most.

## Phase 1: Core + analysis (weeks 2 to 3)

- [x] Cargo workspace: `cratekit-core`, `cratekit-audio`, `src-tauri` (edition 2024, pinned toolchain, workspace lints)
- [x] UI scaffold in `ui/`: React 19 + TypeScript 7 + Vite 8 with bun; Vitest; Tailwind v4 + shadcn (Base UI); oxlint, Prettier, knip; husky + lint-staged + commitlint
- [x] Choose (license check, ask first): state library and table virtualization: Zustand, TanStack Table + Virtual (#6)
- [x] `deny.toml` (license allow-list, see the `license-check` skill)
- [x] CI: fmt, clippy, tests, `cargo deny check`, UI type-check + Vitest, run natively on macOS arm64 + x86_64, Windows x86_64 + ARM64, Linux x86_64 + arm64 (GitHub Actions; every PR on all six targets)
- [x] Confirm minimum OS versions (Tauri 2, ONNX Runtime) and that every native dependency has builds for all six targets (#7)
- [x] Decode any supported file to `AudioBuffer` (symphonia) + resample (rubato) (#8)
- [x] Synthetic audio test helpers (kick at BPM, chords in a key, sine at level) (#9)
- [x] `TempoDetector` + `KeyDetector` traits; stratum-dsp implementation; fallback implementation (#10). Our own detectors are the default; stratum-dsp is behind the `stratum` feature
- [x] Camelot mapping + `matches()` with tests for every key (#11)
- [x] Loudness (LUFS, true peak) + energy score (#12)
- [x] Waveform columns (#13)
- [x] SQLite (rusqlite, WAL, single writer) schema + migrations; add files/folders; tag reading (#14)
- [x] Background analysis queue with progress events (#15)
- [x] UI: typed IPC layer (`ui/src/lib/ipc.ts`) (#16)
- [x] UI: window layout (sidebar, deck, tabs, track browser, status bar) (#17)
- [x] UI: Analyze tab (key, chroma bars, mixes-well-with chips) (#18)
- [x] Set builder: side list to stage tracks and build a set (#55)
- [x] DJ filter knob on the deck (#56)
- [x] Playback: load, play/pause, seek, waveform playhead (canvas, outside React renders) (#19)
- [x] Audio output device selection + hot-plug fallback to the system default (#20)
- [x] UI: Settings screen (audio output, watched folders, output folder) (#21)
- [ ] Test analysis on the DJ's 10 tracks; compare with VirtualDJ's BPM/key (#22)
- [x] Multi-language UI: i18n foundation (i18next), English by default for every user, French translation (#80)

**Gate:** runs on a laptop CPU; BPM and key agree with VirtualDJ on at least 8 of 10 tracks.

## Phase 2: Mastering + stems (weeks 4 to 6)

Order (decided 2026-11-19, #5): **stems first**, then mastering. Stems are everyday prep work (in the 2026 Digital DJ Tips census, 73% of DJs use stems and 43% prepare them ahead) and carry the most platform risk (an ML runtime per target), so they start first. Mastering mainly serves the DJ's own edits and recorded mixes.

Stems
- [x] **Research:** ONNX Runtime GPU backends for htdemucs via `ort`: CoreML (macOS), DirectML vs Windows ML (Windows); prebuilt binaries per target; record findings in `docs/ARCHITECTURE.md` (#29)
- [x] **Research:** stems on Intel Macs: ONNX Runtime 1.23.2, the last Intel build, loaded at run time (#114)
- [ ] ONNX Runtime linking (run-time load or static) and the CPU (AVX2), glibc and Windows 10 checks for stems (#115)
- [x] Model download with progress + checksum (#30)
- [x] `StemSeparator` trait; htdemucs implementation on `ort` with our own chunking (#31)
- [ ] Backend selection: GPU provider with CPU fallback, CPU reference check, "CPU only" setting, backend in the status bar; verified on real hardware (Apple Silicon, Windows GPU, a CPU-only machine) (#32)
- [x] Cancel + progress; cache per track (#33)
- [ ] Stem mixer in the audio engine (gain, mute, solo) (#34)
- [x] UI: Stems tab; export WAVs (#35)

Mastering
- [ ] Biquad EQ, compressor, true-peak limiter with tests (#23)
- [ ] Presets + intensity (#24)
- [ ] 2-pass loudness targeting, export WAV 24/16-bit (dither) (#25)
- [ ] Real-time preview chain in `cratekit-audio`; level-matched A/B (#26)
- [ ] Reference-track matching (#27)
- [ ] UI: Master tab (#28)

Infra
- [x] CI: unsigned installer builds on every target that check the minimum OS versions and installer sizes (#113)

**Gate:** the DJ uses at least one master or one acapella in a real set or edit.

## Phase 3: Controller + launch (weeks 7 to 8)

- [ ] MIDI device list + hot-plug (#36)
- [ ] MIDI learn + mapping storage (#37)
- [ ] Targets: play, cue, hot cues 1 to 8 (pad page 1 to 4 / 5 to 8), stem pads, filter, tempo, volume, A/B (#38)
- [ ] Pad LED feedback for the DDJ-FLX4 (#39)
- [ ] Keyboard shortcuts (#40)
- [ ] Hot cue editing on the waveform (set, move, colour, name) (#41)
- [ ] **Research:** document the VirtualDJ database format from the DJ's real file (song entries, BPM, key, cue points); write findings in `docs/virtualdj-format.md` (#42)
- [ ] VirtualDJ export with backup + dry-run (#43)
- [ ] Installers via Tauri bundler: macOS universal dmg, Windows msi (x86_64 + ARM64), Linux .deb and AppImage (x86_64 + arm64); installer size checked against the 60 MB budget per target (#44)
- [ ] Launch smoke tests on the minimum OS versions (VMs: macOS 14.2 and 13, Windows 10 22H2, Ubuntu 22.04, Debian 12) (#116)
- [ ] `release` Claude skill (version bump, CHANGELOG cut, tag, signed installers)
- [ ] README in BunMail's style: pitch, "Why Cratekit?" comparison table, quick start, honest "How good is the mastering?" section, screenshots (#45)
- [ ] Launch: Show HN, r/DJs, r/WeAreTheMusicMakers, VirtualDJ forum (#46)
- [ ] Arabic translation with a right-to-left layout (#81)

**Gate:** the DJ uses Cratekit every week to prepare his sets → tag **v0.1** and publish.

## v0.2: Performance mode

Toward replacing VirtualDJ (SPEC Module 8). Starts after v0.1 ships.

- [ ] **Research:** GPL-free time-stretching for beat sync and key lock (#57)
- [ ] Deck-aware engine and a second deck (#58)
- [ ] Mixer: gain, 3-band EQ, channel faders, crossfader (#59)
- [ ] Headphone cue (pre-listen) on a second output (#60)
- [ ] Beat sync and key lock (#61)
- [ ] Effects: echo, reverb, flanger and beat-synced FX (#62)
- [ ] Loops: auto loop, loop in/out, loop roll (#63)
- [ ] Record the mix to WAV (#64)
- [ ] Full DDJ-FLX4 mapping for performance (#65)
- [ ] Performance mode UI: two decks, mixer, FX (#66)
- [ ] Sampler: one-shot sample pads (#75)
- [ ] Slip mode and reverse (#76)
- [ ] Beat jump (#77)
- [ ] Up to 16 hot cues per track (#78)
- [ ] Pitch range setting: ±8%, ±16%, ±100% (#79)

**Gate:** the test DJ plays a full set with Cratekit instead of VirtualDJ.

## Later

- Sample library (index folders, search by BPM / key / type, preview) (#47)
- Auto cue points (drop, breakdown, outro) (#48)
- 6 stems, lead vs backing vocals, NI STEM export (#49)
- Rekordbox XML and Serato export (#50)
- Ready-made mappings for more controllers (Numark, Hercules, Pioneer DDJ-400/SB3) (#51)
- Batch mastering of a crate (#52)
- More UI languages: Spanish, German and others (#82)
- Four decks (#83)
- **Research:** DVS, control with timecode vinyl and CDs (#84)
- **Research:** CDJ and HID controller support (#85)
- Auto-mix and next-track suggestions, computed locally (#86)
- Effect chaining: several effects per deck (#87)
- Crossfader curve setting (#88)
- Automatic set recording (#89)
- FLAC export for recordings and stems (#90)
- Cover art in the library and on the deck (#91)
- Themes: light theme and accent colours (#92)
- Multi-screen: detach panels into their own windows (#93)
- Karaoke mode (#94)
- Playlist export and import (M3U), the offline alternative to cloud sharing (#95)
- Plan for the retiring Intel macOS CI runner (fall 2027): cross-compile and test on owned hardware (#117)
- **Research:** lower the macOS minimum if cpal weak-links its process-tap functions (#118)
