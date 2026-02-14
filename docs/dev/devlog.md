# Development Guide

This is the primary development workflow document.
For topic-specific details, link to the canonical docs in [docs/README.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/README.md).
This is the canonical source for binary/build/run command matrices.
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
- Embedded GUI API writes the active endpoint to `.api-addr` in the parent directory of `DB_PATH`; `lpaste` auto-uses it when `--server` and `LP_SERVER` are unset (unless `--no-discovery` is set).

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

# 5) targeted tests for touched areas
# cargo test -p <crate>

# 6) runtime smoke (server + CLI CRUD)
# run isolated server+CLI CRUD flow with localpaste + lpaste
# (new -> list -> search -> get -> delete), then verify persistence across restart
```

Parity/release gate status is tracked in [parity-checklist.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/parity-checklist.md).

## API Summary (High-Level)

Authoritative route wiring lives in
[`crates/localpaste_server/src/lib.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/lib.rs).
Authoritative request/response behavior lives in
[`crates/localpaste_server/src/handlers/paste.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/handlers/paste.rs).
Use this section as orientation only and avoid copying route-by-route behavior here.

Key shape expectations:
- `/api/pastes` and `/api/pastes/meta` return metadata rows (`PasteMeta`).
- `/api/search` preserves content-match semantics but returns metadata rows (`PasteMeta`).
- `/api/search/meta` performs metadata-only matching (name/tags/language).
- Folder routes are deprecated and emit warning headers.

Current deprecation and parity status is tracked in [parity-checklist.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/parity-checklist.md).

## Database Notes

- Backend store: sled.
- Default DB path: platform cache dir (`%LOCALAPPDATA%\\localpaste\\db` on Windows, `~/.cache/localpaste/db` elsewhere).
- Use `DB_PATH` for isolated test runs.
- GUI sidebar list window is capped at `DEFAULT_LIST_PASTES_LIMIT` (`512`); use search/command palette for global discovery.

Lock recovery guidance (including what not to delete) lives in [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md).

## Related Docs

- System architecture: [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md)
- Security defaults and public exposure: [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md)
- Service management: [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md)
- Lock behavior model: [locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md)
- Perf protocol: [gui-perf-protocol.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md)
- Storage split follow-up: [storage-split-plan.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/storage-split-plan.md)
- Rewrite parity checklist: [parity-checklist.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/parity-checklist.md)
- Engineering backlog: [backlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/backlog.md)
- Folder audit matrix (2026-02-13): [folder-audit-matrix-2026-02-13.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/folder-audit-matrix-2026-02-13.md)
- Folder audit report (2026-02-13): [folder-audit-report-2026-02-13.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/folder-audit-report-2026-02-13.md)

## Backlog

Deferred technical work is tracked in [backlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/backlog.md).
