# Contributing to Cratekit

Thanks for your interest. Cratekit is in early development, so the best way to help right now is to pick an open issue, try the app, or report what breaks on your machine.

## Getting started

Follow [README.md](README.md) for prerequisites and setup (Rust via rustup, bun, and your OS's system libraries), then:

```bash
bun install          # also installs the git hooks
bun run tauri dev    # run the app
bun run setup:stems  # once, so stems work in development builds
bun run check        # run every gate before pushing
```

The design documents are in [docs/](docs/): [SPEC.md](docs/SPEC.md) (what to build and the acceptance criteria), [ARCHITECTURE.md](docs/ARCHITECTURE.md) (how it fits together), [ROADMAP.md](docs/ROADMAP.md) (what is next) and [UI.md](docs/UI.md) (design system).

## Workflow

1. **Pick or open an issue.** Comment that you are working on it. For anything non-trivial, describe your approach in the issue first so we can agree on it before you write code.
2. **Branch** from `main`: `<type>/<issue-number>-<short-slug>`, for example `feat/12-camelot-mapping`.
3. **One task per pull request.** Keep PRs small and focused.
4. **Open the PR** with the template: summary, changes (including docs), test plan, `Closes #N`.
5. CI must be green before merge. PRs are squash-merged.

## Commit messages

[Conventional commits](https://www.conventionalcommits.org), enforced by commitlint:

```
<type>: <subject>          # header max 100 characters

<body: what and why>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.

## Gates and hooks

`bun run check` runs everything CI runs, in this order:

- **Rust:** `cargo fmt --check`, `taplo fmt --check`, `cargo clippy -D warnings`, `cargo nextest`, doc tests, `cargo deny`, `cargo shear`, `typos`
- **UI:** `tsc`, `oxlint`, `prettier --check`, `knip`, `vitest`

Git hooks run automatically: **pre-commit** formats and lints staged files, **commit-msg** checks the message, **pre-push** runs `bun run check`. Please don't bypass them with `--no-verify`; fix the cause instead.

## Hard rules

These come from [CLAUDE.md](CLAUDE.md), which holds the full project conventions:

- **Rust backend only, no Python** in the app.
- **Offline first.** The only network call is the one-time ML model download (with progress and checksum). No telemetry, no analytics, no accounts.
- **Never block the audio thread**: no allocation, locks, I/O or logging inside the audio callback.
- **Never modify the user's original audio files.** Writing to VirtualDJ is opt-in and backs up its database first.
- **Licenses:** dependencies must be MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0 or Unicode-3.0 (OFL-1.1 for fonts only). **No GPL, AGPL or LGPL**, and no code copied from GPL projects (reimplementing a published algorithm from its description is fine). Ask in the issue before adding any new dependency.
- **No business logic in the UI**: the React UI renders state and sends commands; analysis, mastering and library logic live in Rust.
- **Tests with synthetic audio only.** Never commit copyrighted audio.

## Code style

- **Rust:** edition 2024, Clippy clean with `-D warnings`. No `unwrap()` or `expect()` in library code (return typed errors with `thiserror`); use `#[expect(lint, reason = "...")]` instead of `#[allow]`. Every DSP function gets a unit test with synthetic audio.
- **UI:** TypeScript strict, no `any`, no unsafe casts. React function components; all calls to Rust go through `ui/src/lib/ipc.ts`.
- **IPC payload types:** `ui/src/lib/bindings/` is generated from the Rust structs by the Rust tests (ts-rs). After changing a payload struct, run `bun run test:rust` and commit the updated files; CI fails when they are stale. See `docs/ARCHITECTURE.md`, IPC.
- **Docs:** update the relevant files in `docs/` and add a `CHANGELOG.md` entry under `[Unreleased]` in the same PR.

## Reporting bugs and security issues

- Bugs and feature ideas: [open an issue](https://github.com/mohamedboukari/cratekit/issues/new/choose) using the templates.
- Security problems: **don't open an issue**; see [SECURITY.md](SECURITY.md).

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). By participating you agree to uphold it.

## License

Cratekit is [MIT licensed](LICENSE). By contributing, you agree that your contributions are licensed under the MIT License.
