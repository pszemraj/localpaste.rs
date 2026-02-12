# Development Guide

This is the primary development workflow document.
For topic-specific details, link to the canonical docs in `docs/README.md`.

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

For editor-mode flags and tracing env vars, see [GUI notes](gui-notes.md).
For repeatable GUI perf validation, see [GUI perf protocol](gui-perf-protocol.md).

## Validation Loop

```bash
# 1) format
cargo fmt

# 2) lint
cargo clippy --all-targets --all-features

# 3) tests
cargo test

# 4) runtime smoke (server + CLI CRUD)
# start localpaste, run lpaste new/list/search/get/delete, then stop localpaste
```

Parity/release gate status is tracked in [parity-checklist.md](parity-checklist.md).

## API Summary

- `POST /api/paste`
- `GET /api/paste/:id`
- `PUT /api/paste/:id`
- `DELETE /api/paste/:id`
- `GET /api/pastes`
- `GET /api/search?q=`
- `POST /api/folder`
- `GET /api/folders`
- `PUT /api/folder/:id`
- `DELETE /api/folder/:id`

Folder updates reject cyclic parent relationships (`400 Bad Request`).

## Database Notes

- Backend store: sled.
- Default DB path: `~/.cache/localpaste/db`.
- Use `DB_PATH` for isolated test runs.

Lock recovery guidance (including what not to delete) lives in [docs/deployment.md](../deployment.md).

## Related Docs

- Security defaults and public exposure: [docs/security.md](../security.md)
- Service management: [docs/deployment.md](../deployment.md)
- Perf protocol: [gui-perf-protocol.md](gui-perf-protocol.md)
- Virtual editor rollout plan: [virtual-editor-plan.md](virtual-editor-plan.md)
- Rewrite parity checklist: [parity-checklist.md](parity-checklist.md)
