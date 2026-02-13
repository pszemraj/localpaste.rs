# Development Guide

This is the primary development workflow document.
For topic-specific details, link to the canonical docs in `docs/README.md`.
This is the canonical source for binary/build/run command matrices.
Other docs should link here instead of repeating command matrices.

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

For editor-mode flags and tracing env vars, see [GUI notes](gui-notes.md).
For repeatable GUI perf validation, see [GUI perf protocol](gui-perf-protocol.md).

## Validation Loop

```bash
# 1) format
cargo fmt --all

# 2) lint
cargo clippy --workspace --all-targets --all-features

# 3) full compile check
cargo check --workspace --all-targets --all-features

# 4) targeted tests for touched areas
# cargo test -p <crate>

# 5) runtime smoke (server + CLI CRUD)
# run the AGENTS.md smoke flow for localpaste + lpaste
```

Parity/release gate status is tracked in [parity-checklist.md](parity-checklist.md).

## API Summary (High-Level)

Authoritative route wiring lives in `crates/localpaste_server/src/lib.rs`.
Authoritative request/response behavior lives in `crates/localpaste_server/src/handlers/paste.rs`.
Use this section as orientation only.

| Route | Response shape | Notes |
| --- | --- | --- |
| `POST /api/paste` | `Paste` | create |
| `GET /api/paste/:id` | `Paste` | full content fetch |
| `PUT /api/paste/:id` | `Paste` | blocked with `423` when paste is locked by GUI |
| `DELETE /api/paste/:id` | `{ success: true }` | blocked with `423` when paste is locked by GUI |
| `GET /api/pastes` | `Vec<PasteMeta>` | metadata-only list for bounded payloads |
| `GET /api/pastes/meta` | `Vec<PasteMeta>` | explicit metadata list endpoint |
| `GET /api/search?q=...` | `Vec<PasteMeta>` | preserves content-match semantics, returns metadata rows |
| `GET /api/search/meta?q=...` | `Vec<PasteMeta>` | metadata-only match (name/tags/language) |
| `POST/GET/PUT/DELETE /api/folder...` | folder payloads | deprecated; emits warning headers |

Current deprecation and parity status is tracked in [parity-checklist.md](parity-checklist.md).

## Database Notes

- Backend store: sled.
- Default DB path: `~/.cache/localpaste/db`.
- Use `DB_PATH` for isolated test runs.
- GUI sidebar list window is capped at `DEFAULT_LIST_PASTES_LIMIT` (`512`); use search/command palette for global discovery.

Lock recovery guidance (including what not to delete) lives in [docs/deployment.md](../deployment.md).

## Related Docs

- Security defaults and public exposure: [docs/security.md](../security.md)
- Service management: [docs/deployment.md](../deployment.md)
- Perf protocol: [gui-perf-protocol.md](gui-perf-protocol.md)
- Virtual editor rollout plan: [virtual-editor-plan.md](virtual-editor-plan.md)
- Rewrite parity checklist: [parity-checklist.md](parity-checklist.md)
- Folder audit matrix (2026-02-13): [folder-audit-matrix-2026-02-13.md](folder-audit-matrix-2026-02-13.md)
- Folder audit report (2026-02-13): [folder-audit-report-2026-02-13.md](folder-audit-report-2026-02-13.md)

## Deferred TODO Backlog (2026-02-13 Cold-Eyes Audit)

- [ ] Split `LocalPasteApp` into domain state groups (`EditorState`, `HighlightState`, `SearchState`, `UiState`) to reduce coupling and simplify test harness construction.
- [ ] Extract the virtual input-routing/control-flow block from `LocalPasteApp::update` into a dedicated per-frame input pipeline API.
- [ ] Add CI-friendly perf microbench coverage (list-from-metadata and highlight/layout path) to catch algorithmic regressions earlier than manual perf runs.
- [ ] Evaluate post-sled storage options (`redb` and `rusqlite`) and document migration constraints around current CAS-style folder/paste update paths.
- [ ] Revisit backend query cache invalidation strategy with metadata-aware generations/in-place cache patching where correctness permits.
- [ ] Replace process-list heuristics for stale-lock checks with PID-file ownership + liveness probing (exact process matching is now a stopgap hardening step).
