# Security Policy

## Supported versions

Cratekit is in early development and has no releases yet. Security fixes land on `main`. Once v0.1 ships, this section will list the supported release lines.

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Use [GitHub's private vulnerability reporting](https://github.com/mohamedboukari/cratekit/security/advisories/new). The report stays private between you and the maintainer until a fix is available.

### What to include

- What is affected (module, file, command or setting) and on which OS and CPU architecture
- Steps to reproduce, or a proof of concept
- The impact you expect (for example: code execution, file overwrite, data loss, leaked data)
- Any suggested fix

### Response timeline

- Acknowledgement within 7 days
- A first assessment (confirmed or not, severity) within 14 days
- A fix or mitigation as soon as practical, with credit in the advisory unless you prefer to stay anonymous

## Scope

Cratekit is an **offline desktop app**. It has no accounts, no telemetry and no server. In scope:

- **The ML model download**: the only network call. It must verify the checksum before use; anything that bypasses or weakens that check is in scope.
- **File handling**: Cratekit must never modify the user's original audio files, and it backs up the VirtualDJ database before writing to it. Path traversal, writes outside Cratekit's own folders, or a corrupted VirtualDJ database are in scope.
- **Parsing untrusted files**: audio decoding, tag reading and the VirtualDJ database parser handle files from anywhere. Crashes or memory-safety issues triggered by a crafted file are in scope.
- **The Tauri layer**: the content security policy, the capability (permission) configuration, and the IPC commands between the UI and Rust.
- **The dependency policy**: a dependency with a known vulnerability or a disallowed license that slipped past `cargo deny`, Dependabot or dependency review.

Out of scope:

- Issues that need an attacker who already controls the user's account or machine
- Missing hardening that has no demonstrated impact
- Vulnerabilities in third-party software Cratekit works alongside (VirtualDJ, the OS, the webview), unless Cratekit makes them exploitable

## How the project defends itself

- `cargo deny` checks every Rust dependency for RustSec advisories and allowed licenses on every push.
- Dependabot keeps Rust, JavaScript and GitHub Actions dependencies current; dependency review blocks pull requests that add vulnerable or GPL-family packages.
- CodeQL scans the Rust and TypeScript code; gitleaks scans for committed secrets; zizmor audits the CI workflows.
- GitHub Actions are pinned to commit SHAs; workflows use read-only tokens by default.
- Workspace lints deny `unsafe` code unless a module explicitly opts in with a written reason.
