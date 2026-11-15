## Summary

<!-- What this PR does and why, in 1 to 3 sentences. Link the ROADMAP task. -->

## Changes

<!-- Bullet list of the main changes, including every doc updated (CHANGELOG.md is always one). -->

-

## Decisions not covered by the docs

<!-- Any choice made because the docs were silent, and why it is the simplest option that respects the hard rules. Write "None" if none. -->

## Hard rules check

- [ ] No Python, no network calls besides the model download
- [ ] Audio callback untouched, or free of allocation, locks, I/O, logging and panics (docs/ARCHITECTURE.md, Threads)
- [ ] Original audio files never modified
- [ ] New dependencies (if any) have an allowed license (CONTRIBUTING.md, Hard rules) and were agreed in the issue
- [ ] No business logic added to the UI

## Test Plan

<!-- Commands run and their result, plus any manual check (OS and architecture). -->

- [ ] `bun run check` (every gate CI runs)
- [ ] Manual check in `bun run tauri dev`, if the change is visible in the app

Closes #
