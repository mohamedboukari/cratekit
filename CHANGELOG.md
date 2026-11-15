# Changelog

All notable changes to Cratekit are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Project docs: product spec, architecture, roadmap, UI design system and clickable mockup.
- Dev tooling for Claude Code: project skills (`ship`, `realtime-audio`, `license-check`, graphify), permissions allowlist, no-em-dash hook, context7 MCP, rust-analyzer and TypeScript LSP plugins (Vercel plugins disabled for this project), graphify knowledge graph with git hooks.
- `CHANGELOG.md`, PR template, `.gitignore` that keeps test audio and VirtualDJ files out of git.

- Cargo workspace (`cratekit-core`, `cratekit-audio`, `src-tauri`) on Rust 1.100, edition 2024, with workspace lints (no unwrap/expect, no println!, no unsafe, `#[expect]` over `#[allow]`).
- Runnable Tauri 2 app shell (`io.github.mohamedboukari.cratekit`, native titlebar, 1280x800, min 1024x640, strict CSP) with a first IPC command (`app_version`).
- React 19 + TypeScript 7 + Vite 8 UI with Tailwind v4, shadcn/ui (Base UI), bundled Space Grotesk and JetBrains Mono fonts, Vitest.
- Tooling: cargo-nextest, cargo-deny, cargo-shear, cargo-llvm-cov, taplo, typos, bacon; oxlint (type-aware), Prettier, knip; husky + lint-staged + commitlint; GitHub Actions CI across six targets.
- README (developer setup) and MIT LICENSE.
- Open source release: SECURITY.md (private vulnerability reporting), CONTRIBUTING.md, CODE_OF_CONDUCT.md (Contributor Covenant 2.1), issue templates, Dependabot (cargo, bun, GitHub Actions), CodeQL (Rust, TypeScript), gitleaks, zizmor and dependency review workflows; CI runs all six targets on every pull request.
- Doc tests in the gates; pre-push skips delete-only pushes.
- GitHub issues and milestones for every ROADMAP task; each ROADMAP line links to its issue.
- UI foundation for the redesign (#1): typed `CratekitApi` with a Tauri implementation and a fake-data mock (never in production builds), Zustand stores, laptop-first resizable shell (sidebar, deck, track browser, tool panel, status bar), keyboard shortcut map, toasts, and a GitHub Pages demo in mock mode.

### Changed

- UI stack: React 19 + Vite + TypeScript with bun and Vitest (the first draft said Svelte 5 with pnpm).
- Database notes: SQLite through rusqlite in WAL mode with a single writer connection.
- Platforms: macOS universal (arm64 + x86_64), Windows x86_64 + ARM64, Linux x86_64 + arm64, all tested in CI. Stems use the GPU when available (CoreML / DirectML) with CPU fallback.
- Hot cues: 8 per track (pads page 1 to 4 / 5 to 8). New Settings module (audio output device, folders, processing backend).
