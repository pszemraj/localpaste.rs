# Development Guide

This guide covers the development workflow.
Topic-specific ownership references are listed in
[docs/README.md](../README.md).
This guide includes the binary/build/run command matrix used in day-to-day development.
System architecture context lives in
[docs/architecture.md](../architecture.md).

## Workspace Layout

```text
localpaste.rs/
|-- Cargo.toml
|-- crates/
|   |-- localpaste_core/    # config, db, models, naming, errors
|   |-- localpaste_server/  # axum API + embedded server
|   |-- localpaste_gui/     # native rewrite desktop app
|   |-- localpaste_cli/     # lpaste binary
|   `-- localpaste_tools/   # dataset generators / utilities
|-- docs/
|-- assets/
`-- target/
```

## Binary Map

- `localpaste-gui` - rewrite desktop app (`crates/localpaste_gui`)
- `localpaste` - headless API server (`crates/localpaste_server`)
- `lpaste` - CLI client (`crates/localpaste_cli`)
- `generate-test-data` - synthetic dataset tool (`crates/localpaste_tools`)
- `check-loc` - line-count policy checker (`crates/localpaste_tools`)
- `check-ast-dupes` - semantic duplicate/dead-symbol audit (`crates/localpaste_tools`)

## Build Matrix

```bash
# GUI
cargo build -p localpaste_gui --bin localpaste-gui --release

# Server
cargo build -p localpaste_server --bin localpaste --release

# CLI
cargo build -p localpaste_cli --bin lpaste --release

# Tooling
cargo build -p localpaste_tools --bin generate-test-data --release
cargo build -p localpaste_tools --bin check-loc --release
cargo build -p localpaste_tools --bin check-ast-dupes --release
```

## Run Matrix

```bash
# Rewrite GUI
cargo run -p localpaste_gui --bin localpaste-gui

# Server
cargo run -p localpaste_server --bin localpaste --release

# CLI (built binary)
./target/release/lpaste --help
```

Topology note:

- `localpaste-gui` owns the DB lock for its `DB_PATH` and hosts an embedded API endpoint for compatibility.
- Do not run `localpaste` concurrently on the same `DB_PATH` as the GUI.
- Use standalone `localpaste` for headless/server-only operation.
- Embedded GUI API writes the active endpoint to `DB_PATH/.api-addr`; `lpaste` auto-uses it when `--server` and `LP_SERVER` are unset (unless `--no-discovery` is set).

For editor-mode flags and tracing env vars, see
[docs/dev/gui-notes.md](gui-notes.md).
For repeatable GUI perf validation, see
[docs/dev/gui-perf-protocol.md](gui-perf-protocol.md).

## Validation Loop

Policy source of truth:
This document and linked `docs/dev/*` references define mandatory validation gates and when smoke/manual GUI checks are required.
This section is the quick command reference used during active development.

```bash
# 1) format
cargo fmt --all

# 2) lint
cargo clippy --workspace --all-targets --all-features

# 3) full compile check
cargo check --workspace --all-targets --all-features

# 4) LoC policy check
cargo run -p localpaste_tools --bin check-loc -- --max-lines 1000 --warn-lines 900

# 5) duplicate/dead-symbol audit (required on refactors)
cargo run -p localpaste_tools --bin check-ast-dupes -- --root crates

# 6) targeted tests for touched areas
# cargo test -p <crate>

# 7) runtime smoke (server + CLI CRUD)
# run isolated server+CLI CRUD flow with localpaste + lpaste
# (new -> list -> search -> get -> delete), then verify persistence across restart

# 8) docs contract check
rustdoc-checker crates --strict
```

- Canonical manual GUI checklist:
  [docs/dev/gui-notes.md#manual-gui-human-step-checklist-comprehensive](gui-notes.md#manual-gui-human-step-checklist-comprehensive)

Language detection/normalization/highlight behavior is tracked in
[docs/language-detection.md](../language-detection.md).

## GUI Release Pipeline

GitHub Actions workflow:
`.github/workflows/release-gui.yml`

Packaging configs:

- `packaging/windows/packager.json`
- `packaging/linux/packager.json`
- `packaging/macos/packager.json`

Release behavior:

- Tag push (`v*`) resolves the tag, enforces tag/version match, runs smoke
  validation, builds GUI installers/portable assets, and publishes assets to
  that tag's release.
- Manual run (`workflow_dispatch`) accepts an existing tag and reruns the same
  smoke/build/publish flow against that tag.

Release gates for release workflow:

- Tag sanity check (`v*`) and existence validation
- `Cargo.toml` workspace version must match the release tag version
- Server + CLI smoke test with restart persistence verification

Release workflow intentionally excludes:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features`
- `cargo check --workspace --all-targets --all-features`
- `check-loc`
- `check-ast-dupes`

Expected GUI release assets:

- `localpaste-<tag>-windows-x86_64.msi`
- `localpaste-<tag>-windows-x86_64.zip`
- `localpaste-<tag>-linux-x86_64.AppImage`
- `localpaste-<tag>-linux-x86_64.tar.gz`
- `localpaste-<tag>-macos-aarch64.dmg`
- `localpaste-<tag>-macos-aarch64.app.tar.gz`
- `checksums.sha256`

## Behavior Contracts

This file is intentionally command/workflow-focused. For runtime behavior contracts, use:

- System/runtime architecture: [docs/architecture.md](../architecture.md)
- Security defaults and env policy: [docs/security.md](../security.md)
- Service operation and lock recovery: [docs/deployment.md](../deployment.md)
- Lock semantics and API `423 Locked` behavior: [docs/dev/locking-model.md](locking-model.md)
- Detection/normalization/highlight behavior: [docs/language-detection.md](../language-detection.md)
- API wiring + handler behavior in code:
  - [`../../crates/localpaste_server/src/lib.rs`](../../crates/localpaste_server/src/lib.rs)
  - [`../../crates/localpaste_server/src/handlers/paste.rs`](../../crates/localpaste_server/src/handlers/paste.rs)
- Engineering backlog: [docs/dev/backlog.md](backlog.md)
