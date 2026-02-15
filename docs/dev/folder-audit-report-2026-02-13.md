# Folder Audit Report (2026-02-13)

This is a historical summary artifact.
Detailed path-level findings and coverage mapping are canonical in:
[docs/dev/folder-audit-matrix-2026-02-13.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/folder-audit-matrix-2026-02-13.md).

## Scope

Audit order executed: `B -> C -> E -> D -> F -> A`.

Evaluated surfaces:

- `localpaste_core` folder/paste transaction paths
- `localpaste_server` folder + paste handlers
- `localpaste_gui` backend folder/paste command flows
- `localpaste_tools` generation/clear mutation paths

## Validation Gates Run

Executed and passing during the audit pass:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets --all-features`
- `cargo check --workspace --all-targets --all-features`
- `cargo test -p localpaste_core`
- `cargo test -p localpaste_server`
- `cargo test -p localpaste_gui --all-targets`
- `cargo test -p localpaste_tools`
- `rustdoc-checker crates --strict`
- isolated API/core smoke flow (`localpaste` + `lpaste`, including restart persistence)

## Findings Summary

Highest-impact remediations completed in that pass:

1. Hardened folder assignability and compensation paths for create/move/delete race windows.
2. Centralized guarded folder-delete behavior so API and GUI backend use shared core helpers.
3. Added startup folder invariant reconcile coverage (counts, orphan references, stale delete markers).
4. Aligned tooling clear paths with invariant-preserving transactional delete semantics.

For affected callsites and tests, use the matrix:
[docs/dev/folder-audit-matrix-2026-02-13.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/folder-audit-matrix-2026-02-13.md).

## Residual Risk Notes

These notes were true at audit time and are historical context only.
Current runtime/storage behavior is canonical in:

- [docs/storage.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/storage.md)
- [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md)
- [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md)

## Follow-Up Routing

- Active deferred work belongs in:
  [docs/dev/backlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/backlog.md)
- Runtime/storage architecture context belongs in:
  [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md)
