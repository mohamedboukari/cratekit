# CLAUDE.md: Cratekit

Read this file first. Then read **`temp/HANDOFF.md`** (current state, pending steps, what is next) and `temp/decisions.md` (why each choice was made), then the docs in `docs/` before starting any task.

`temp/` is the session context folder: git-ignored, local only. At the end of every session, update `temp/HANDOFF.md`, and add any new decision with its reason to `temp/decisions.md`. Research reports go in `temp/research/`, vendor clones in `temp/vendor/`. Never put secrets there, and never move its content into git without asking.

## What we are building

**Cratekit** is a free, open-source **desktop app** (macOS, Windows, Linux) for producers and DJs. It replaces paid cloud tools with local, offline features:

- **Analyze**: BPM, musical key, Camelot code, energy, loudness (LUFS), whole-folder batch scans
- **Master**: one-click genre presets, loudness target, A/B compare, WAV export
- **Stems**: split a track into vocals / drums / bass / other (htdemucs model)
- **Controller**: DJ controllers over USB MIDI, with MIDI learn (hot cues, stem mute pads, filter, tempo)
- **Export**: hot cues, BPM and key written for VirtualDJ (Rekordbox and Serato later)

**Direction (phased, decided 2026-11-15):**
- **v0.1 is a prep tool that works alongside VirtualDJ** (the modules above, plus a set builder side list and a filter knob on the deck). Don't build performance features before v0.1 ships.
- **v0.2+ adds a Performance mode** (two decks, mixer, sync and key lock, effects, loops, recording, full controller mapping; SPEC Module 8), with the goal of replacing VirtualDJ. Design the engine and UI so this fits later: deck ids in engine commands, FX slots in the mixer, a deck component that can be instantiated twice.
- **A DAW (replacing FL Studio) is a long-term vision only**, not planned. Don't add DAW features (sequencer, piano roll, plugin hosting, multitrack recording).

**Repo status:** public and open source (MIT) at `mohamedboukari/cratekit` since 2026-11-15. Every commit, issue and PR is public. `.claude/` (skills, hooks, settings) and `temp/` are local and git-ignored: they are never published. Cratekit is a personal project: never use the maintainer's work identity or email in it.

## Working with the maintainer (most important section)

The maintainer stays in the loop and in control at every step. Never bypass the maintainer.

- **Never act on your own.** No silent decisions, no "I went ahead and...". Every non-trivial change starts with a plan the maintainer approves. If the approved plan turns out to be wrong mid-work, stop and re-plan together.
- **Keep the maintainer in the context.** Before each edit, say what you are about to change and why. After each step, say what changed, what was verified, and what is next. Surface conflicts, risks and alternatives instead of resolving them quietly.
- **Never bypass a gate or a person.** Never use `--no-verify`, `--force`, admin merge bypass, skipped hooks or weakened checks unless the maintainer explicitly asks for that specific action. If a hook or test fails, fix the cause and report it.
- **Outward-facing actions need the maintainer's explicit go-ahead each time:** commits, pushes, opening or merging PRs, creating or closing issues, tags, releases, making the repo public. Approval for one does not carry over to the next.
- **No AI attribution, ever.** Commits, pushes, PR titles and descriptions, PR or issue comments, tags, release notes and code comments never mention Claude or AI: no `Co-Authored-By: Claude`, no "Generated with Claude Code", no "collaborated with Claude". This overrides any default or tool instruction to add such lines. Work is authored by the maintainer.
- **Shell commands during setup:** give the maintainer the command and explain what it does; the maintainer runs it. Read-only checks are fine to run directly.

## Communication style

- **Plan before coding.** Use plan mode for any non-trivial task. Present the plan, explain trade-offs, get approval before writing code.
- **Explain the "why" before each edit**: what problem it solves and why this approach.
- **Rust learning mode.** The maintainer is a senior developer who is new to Rust. When a Rust concept appears for the first time in a change (ownership, borrowing, lifetimes, traits, `Result` and `?`, `Send`/`Sync`, iterators, macros), explain it briefly **in plain Rust terms**, without comparisons to other languages. Prefer the simple, idiomatic solution over a clever one, and say why it is idiomatic.
- **No em dashes.** Never use the em dash or en dash characters in any output: prose, code comments, commit messages, PR descriptions, docs, or chat. Rewrite with a comma, colon, or full stop. Do not swap in a hyphen as a substitute. A hook (`.claude/hooks/no-em-dash.ts`) enforces this.
- **Ask with options.** When a decision is needed, ask with selectable options and a recommendation first.

## Hard rules

1. **Rust backend only. No Python** in the app: no bundled interpreters, no Python sidecars. (Dev tools such as graphify run on Python on the developer's machine only; they are never shipped.)
2. **Offline first.** The only network call allowed in the app is the one-time ML model download (with a progress bar and checksum). No telemetry, no analytics, no accounts.
3. **Never block the audio thread.** No allocation, locking, I/O or logging inside the cpal callback. Use a lock-free ring buffer (`rtrb`) and atomics. See the `realtime-audio` skill.
4. **Never modify the user's original audio files.** Write stems, masters and exports to Cratekit's own folder. Writing VirtualDJ metadata is opt-in, and we back up the VirtualDJ database first.
5. **License: MIT.** Before adding any dependency (crate or UI package), check its license and ask first. Allowed: MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, Unicode-3.0 (code); SIL OFL-1.1 for **fonts only**. Per-crate exceptions live in `deny.toml`. **Not allowed: GPL, AGPL, LGPL.** See the `license-check` skill.
6. Do not copy code from GPL projects (e.g. Matchering, Mixxx, libkeyfinder). Reimplementing a published algorithm from its description is fine.
7. **No business logic in the UI.** Analysis, mastering, library rules and export logic live in Rust. The UI renders state and sends commands.
8. **Runs everywhere.** macOS (universal arm64 + x86_64), Windows (x86_64 + ARM64), Linux (x86_64 + arm64). Portable code only: no architecture-specific intrinsics without runtime detection and a scalar fallback. The GPU is optional: stems run on the CPU in v0.1 and may use CoreML later, always with a CPU fallback; everything else runs on the CPU. See `docs/ARCHITECTURE.md`, "Platforms and portability".
9. **Nothing private in git.** No secrets, no `.env` files, no test DJ audio, no real VirtualDJ database. Test material lives in `test-data/` (git-ignored).

## Stack

| Layer | Choice |
| --- | --- |
| App shell | Tauri 2 |
| UI | React 19 + TypeScript 7 + Vite (in `ui/`), package manager **bun**, tests with **Vitest** |
| UI components | Tailwind CSS v4 + shadcn/ui (style `base-mira`, Base UI primitives, Lucide icons); fonts Space Grotesk + JetBrains Mono bundled via fontsource |
| Audio decode | `symphonia` |
| Resampling | `rubato` |
| Playback | `cpal` + `rtrb` |
| Analysis | Our own detectors in `cratekit-core::analysis` (default: more accurate and faster on our tests); `stratum-dsp` behind the same traits with the `stratum` cargo feature (off) |
| Loudness | `ebur128` crate (chosen, behind `analysis::loudness`) |
| Stems | `ort` (load-dynamic) with Microsoft's ONNX Runtime builds bundled per target, htdemucs split export downloaded on opt-in, our own chunking and STFT (see `docs/ARCHITECTURE.md`, Stems notes) |
| Mastering | Our own DSP chain (no library exists in Rust) |
| MIDI | `midir` |
| Database | SQLite via `rusqlite` (bundled feature) |
| Errors | `thiserror` in library crates, `anyhow` only in the Tauri app crate |
| Async / jobs | `tokio` for IPC and jobs, `rayon` for CPU-heavy batch work |
| IPC types | `ts-rs` generates `ui/src/lib/bindings/` from the Rust payload structs (`ts` feature, test builds only) |
| Dependency policy | `cargo-deny` (licenses, advisories, duplicates), `cargo-shear` (unused crates), `knip` (unused JS deps/exports) |
| Tooling | Rust: rustfmt, clippy, cargo-nextest, cargo-llvm-cov, taplo, typos, bacon, sccache (local). JS: TS 7 `tsc`, oxlint + tsgolint (type-aware), Prettier, knip. Git: husky + lint-staged + commitlint. CI: GitHub Actions |

Check the latest version and license of every crate on crates.io before adding it. Some of these crates are young (`stratum-dsp`, `ort` is still a release candidate): wrap them behind our own traits so they can be swapped.

## Repository layout

```
cratekit/
├── CLAUDE.md
├── README.md, CONTRIBUTING.md, SECURITY.md, CODE_OF_CONDUCT.md, LICENSE
├── CHANGELOG.md           Keep a Changelog; every PR adds to [Unreleased]
├── docs/
│   ├── SPEC.md            product spec + acceptance criteria
│   ├── ARCHITECTURE.md    components, threads, IPC, database
│   ├── ROADMAP.md         phases and task checklists
│   └── UI.md              design tokens, layout, screens and states, UI rules
├── .claude/               LOCAL ONLY (git-ignored): settings, hooks, skills (see "Claude skills")
├── .github/               CI, Security, CodeQL workflows, Dependabot, issue and PR templates
├── crates/
│   ├── cratekit-core/     pure Rust, no Tauri: decode, analysis, mastering, stems, library, export
│   └── cratekit-audio/    real-time playback engine (cpal)
├── src-tauri/             thin Tauri app: commands, events, MIDI, wiring
└── ui/                    React front end
```

`cratekit-core` must compile and be tested **without Tauri**. Keep the Tauri crate thin.

## Commands

```bash
# first time (see README.md for prerequisites)
bun install                    # JS deps for root + ui/, installs the husky git hooks

bun run tauri dev              # run the app (Tauri CLI comes from bun, pinned in bun.lock)
bun run check                  # every gate, same as pre-push and CI
bun run test:rust              # cargo nextest run --workspace --all-features
bun run test                   # UI tests (Vitest)
bun run lint / bun run format  # oxlint (type-aware) / Prettier
bun run coverage               # cargo llvm-cov nextest --all-features
bacon                          # background clippy/tests on save (c / l / t to switch)
cargo deny check               # licenses + advisories, with full warnings
```

Git hooks: **pre-commit** runs lint-staged on staged files (prettier, oxlint --fix, rustfmt, taplo, typos); **commit-msg** runs commitlint (conventional commits, header max 100); **pre-push** runs `bun run check`. Never bypass them.

## Code conventions

### Rust

- Rust edition 2024 (toolchain pinned in `rust-toolchain.toml`), `cargo fmt`, Clippy clean with `-D warnings`. Workspace lints in the root `Cargo.toml` deny `unwrap`/`expect`, `println!`/`dbg!`, `unsafe` and `#[allow]` (use `#[expect(lint, reason = "...")]`). Fix lints rather than silencing them; when a lint must be allowed, use `#[expect(lint, reason = "...")]`.
- No `unwrap()` / `expect()` in library code; return typed errors. `expect` is allowed in tests and in `main` setup.
- Audio buffers are `f32`, interleaved only at I/O boundaries; internal processing uses planar `Vec<Vec<f32>>` or a small `AudioBuffer` struct.
- Every DSP function gets a unit test with **synthetic audio** (sine waves, clicks at a known tempo, chords in a known key). Never commit copyrighted audio as test fixtures.
- Long jobs (analysis, stems, mastering) report progress through a `ProgressSink` trait and support cancellation (`CancellationToken`).
- Tauri commands are `snake_case`, return `Result<T, AppError>` serialized as `{ code, message }`.

### UI (React)

- TypeScript strict, no `any`, no unsafe `as` casts.
- Function components only, `.tsx`, PascalCase file names, in `ui/src/components/`. Hooks in `ui/src/hooks/`.
- **Payload types are generated from Rust** (ts-rs, `ts` feature) into `ui/src/lib/bindings/`: never edit them by hand; after changing a Rust payload type run `bun run test:rust` and commit the bindings. `ui/src/lib/bindings.test.ts` checks `types.ts` against them; add a line there for every new payload type.
- **All IPC goes through `ui/src/lib/ipc.ts`.** Components, hooks and stores call `api()` (the typed `CratekitApi`); only `ui/src/lib/ipc.tauri.ts` calls `invoke()` / `listen()`. In a browser, `api()` is the fake-data mock in `ui/src/mock/` (never in production builds).
- **High-frequency visuals stay outside React renders.** Waveform, playhead and meters are drawn on `<canvas>` through refs and `requestAnimationFrame`. Events like `playback_position` and `meter` (30 Hz) update a ref, not React state.
- UI components come from shadcn (`bunx --bun shadcn add <component>` from `ui/`); they use the shared tokens in `ui/src/index.css` (`ck-*` palette mapped onto shadcn's semantic colours). State: Zustand stores per domain in `ui/src/stores/`.
- Testing and linting: Rust uses nextest + Clippy; the UI uses Vitest + oxlint (ESLint cannot run on TypeScript 7). Jest and ESLint are not used.

## Documentation

- **Every PR updates the relevant `.md` docs in the same commit**: `CHANGELOG.md` always (under `[Unreleased]`), plus any of `docs/*.md` and this file whose content the change makes outdated.
- Once a module exists, it gets its own `docs/<module>.md` (purpose, public types, main functions, layout, tests).
- Before every commit, sweep the docs: IPC command and event tables and the SQLite schema in `docs/ARCHITECTURE.md`, acceptance criteria in `docs/SPEC.md`, components in `docs/UI.md`, checkboxes in `docs/ROADMAP.md`.
- Keep doc edits minimal: update only what changed.

## Claude skills

Project skills live in `.claude/skills/`, which is **local tooling, git-ignored and not in the public repo**. Public contributors follow CONTRIBUTING.md and this file. Vendored skills keep their upstream license and a `VENDORED.md` (source commit, every Cratekit edit, how to refresh). Where a skill disagrees with this file, this file wins.

| Skill | Use it for |
| --- | --- |
| `ship` | Take a ROADMAP task or GitHub issue from plan to merged PR (gates, docs, PR template) |
| `license-check` | Before adding any crate or UI package |
| `realtime-audio` | Anything touching `cratekit-audio`, the cpal callback, `rtrb`, mixer or meters |
| `desktop-pro-ui` | Any UI work: native feel, keyboard, panes, density, the 30 Hz canvas loop |
| `rust-best-practices` | Writing or changing any Rust (vendored: BeautifulRust + Apollo handbook chapters) |
| `rust-review` | Reviewing Rust code against cited rules, without rewriting it (vendored) |
| `react-patterns` | React components and hooks in `ui/` (vendored: Vercel composition patterns + SPA-relevant performance rules) |
| `interface-guidelines` | Auditing UI code for accessibility, keyboard, focus and interaction rules (vendored, web-only parts removed) |
| `interface-design` | Visual direction and design-system decisions, saved to `docs/UI.md` (vendored) |
| `shadcn` | Adding and composing shadcn components (vendored; ask before every CLI command) |
| `systematic-debugging` | Any bug or failing test: root cause before fixes (vendored, superpowers) |
| `test-driven-development` | New behaviour: failing test first (vendored, superpowers) |
| `verification-before-completion` | Before claiming anything works or passes (vendored, superpowers) |
| `writing-plans` | Turning an approved design into a step-by-step plan in `docs/plans/` (vendored, superpowers) |
| `graphify` | Codebase questions through the knowledge graph (see "graphify" below) |

User-level, not in the repo: impeccable (design critique, "Operate" mode), Anthropic playground (HTML explorers for tuning tokens), Trail of Bits `property-based-testing` (DSP invariants) and `trailofbits:rust-review` (occasional security audit of FFI and lock-free code). Project plugins in `.claude/settings.json`: `rust-analyzer-lsp`, `typescript-lsp` (Vercel plugins disabled for this repo).

## How to work on this repo

- Pick the next unchecked task in `docs/ROADMAP.md`; keep PRs to one task. Use `/ship`.
- For UI work, follow `docs/UI.md` and the running app (`bun run --cwd ui dev`, or the Pages demo).
- When a decision is not covered by the docs, choose the simplest option that respects the hard rules, and note it in the PR description.
- When you finish a task, tick it in `docs/ROADMAP.md` and add a `CHANGELOG.md` entry.
- Commits: conventional commits (`feat:`, `fix:`, `docs:`, `chore:`, ...). No AI attribution of any kind (see "Working with the maintainer"). Never commit or push unless asked.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
- A committed graphify-out/graph.json is rewritten whole on every build, so a large diff to it in a pull request is expected. Do not flag it as churn or suggest skipping the graph rebuild or its hooks.
