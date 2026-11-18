# Cratekit

[![CI](https://github.com/mohamedboukari/cratekit/actions/workflows/ci.yml/badge.svg)](https://github.com/mohamedboukari/cratekit/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-c6f432.svg)](LICENSE)

A free, open-source desktop app for DJs and producers: track analysis (BPM, key, Camelot, energy, loudness), one-click mastering, stem separation, DJ controller support and VirtualDJ export. Everything runs offline on your laptop.

> **Status: early development.** The full UI runs on demo data, but the Rust features behind it are still being built: **try the [live UI demo](https://mohamedboukari.github.io/cratekit/)** (fake data, nothing to install). Contributions are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md) and the [open issues](https://github.com/mohamedboukari/cratekit/issues). The full project README comes with v0.1; progress is tracked in [docs/ROADMAP.md](docs/ROADMAP.md).

## Stack

Tauri 2 desktop app: a Rust backend (Cargo workspace) and a React 19 + TypeScript + Vite UI, managed with bun. Runs on macOS (Apple Silicon and Intel), Windows (x64 and ARM64) and Linux (x64 and arm64).

## Prerequisites

| Tool | Install |
| --- | --- |
| Rust | [rustup](https://rustup.rs). The repo pins the toolchain in `rust-toolchain.toml`, so rustup installs the right version on first build |
| bun | [bun.sh](https://bun.sh) |
| System libraries | macOS: Xcode Command Line Tools (`xcode-select --install`). Windows: WebView2 (preinstalled on Windows 11) and the MSVC build tools. Linux: see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) (`libwebkit2gtk-4.1-dev` and friends), plus the ALSA headers for audio output (`libasound2-dev` on Debian and Ubuntu, `alsa-lib-devel` on Fedora, `alsa-lib` on Arch) |
| Dev tools (for the checks) | `cargo-nextest`, `cargo-deny`, `cargo-shear`, `typos-cli`, `taplo`. With Homebrew: `brew install cargo-nextest cargo-deny cargo-shear typos-cli taplo` |

## Getting started

```bash
bun install          # JS dependencies for the root and ui/ (also installs the git hooks)
bun run tauri dev    # build and open the app (the first Rust build takes a few minutes)
```

## Scripts

| Command | What it does |
| --- | --- |
| `bun run tauri dev` | Run the app with hot reload |
| `bun run --cwd ui dev` | Run only the UI in a browser, on demo data (same as the live demo) |
| `bun run check` | Every gate CI runs: Rust (fmt, taplo, clippy, nextest, doc tests, cargo-deny, cargo-shear, typos) then UI (tsc, oxlint, prettier, knip, vitest) |
| `bun run test` / `bun run test:rust` | UI tests (Vitest) / Rust tests (nextest) |
| `bun run lint` / `bun run format` | oxlint (type-aware) / Prettier |
| `bun run coverage` | Rust coverage report (cargo-llvm-cov) |

Git hooks (husky): pre-commit formats and lints staged files, commit-msg enforces [conventional commits](https://www.conventionalcommits.org), pre-push runs `bun run check`.

## Repository layout

```
crates/cratekit-core/    pure Rust: decode, analysis, mastering, stems, library, export (no Tauri)
crates/cratekit-audio/   real-time playback engine
src-tauri/               thin Tauri app: commands, events, MIDI, wiring
ui/                      React UI
docs/                    spec, architecture, roadmap, UI (tokens, layout, screens, rules)
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) (workflow, commit style, hard rules) and the design docs in [docs/](docs/). Please follow the [Code of Conduct](CODE_OF_CONDUCT.md). Security issues: see [SECURITY.md](SECURITY.md), and please don't open a public issue.

## License

[MIT](LICENSE). Bundled fonts (Space Grotesk, JetBrains Mono) are under the SIL Open Font License 1.1.
