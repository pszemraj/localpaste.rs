# Development Guide

This is the primary development workflow document.
For topic-specific details, link to the primary docs in [docs/README.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/README.md).
This is the primary source for binary/build/run command matrices.
Other docs should link here instead of repeating command matrices.
System architecture context lives in [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md).

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

For editor-mode flags and tracing env vars, see [GUI notes](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-notes.md).
For repeatable GUI perf validation, see [GUI perf protocol](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md).

## Validation Loop

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

Language detection/normalization/highlight behavior is tracked in [docs/language-detection.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/language-detection.md).

## Behavior Contracts

This file is intentionally command/workflow-focused. For runtime behavior contracts, use:

- System/runtime architecture: [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md)
- Security defaults and env policy: [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md)
- Service operation and lock recovery: [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md)
- Lock semantics and API `423 Locked` behavior: [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md)
- Detection/normalization/highlight behavior: [docs/language-detection.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/language-detection.md)
- API wiring + handler behavior in code:
  - [`crates/localpaste_server/src/lib.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/lib.rs)
  - [`crates/localpaste_server/src/handlers/paste.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/handlers/paste.rs)

## Related Docs

- System architecture: [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md)
- Security defaults and public exposure: [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md)
- Service management: [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md)
- Storage/backend compatibility: [docs/storage.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/storage.md)
- Lock behavior model: [locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md)
- Perf protocol: [gui-perf-protocol.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md)
- Language detection/normalization/highlight behavior: [docs/language-detection.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/language-detection.md)
- Engineering backlog: [backlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/backlog.md)

## Backlog

Deferred technical work is tracked in [backlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/backlog.md).
