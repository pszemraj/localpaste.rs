# Locking Model

This document describes lock behavior in LocalPaste.

---

- [Scope](#scope)
- [Paste Edit Lock Semantics](#paste-edit-lock-semantics)
- [Handler/Worker Enforcement](#handlerworker-enforcement)
- [GUI Ownership](#gui-ownership)
- [Error Surface Contract](#error-surface-contract)
- [Regression Coverage](#regression-coverage)

---

## Scope

LocalPaste has two lock layers with different purposes:

1. **Database owner lock (filesystem / process-wide)**
   - File: `db.owner.lock`
   - Purpose: prevent multiple LocalPaste writers on the same `DB_PATH`.
   - Primary implementation: [`crates/localpaste_core/src/db/lock.rs`](../../crates/localpaste_core/src/db/lock.rs)
2. **Paste edit locks (in-memory / paste-scoped)**
   - Purpose: prevent API/CLI or bulk folder mutations from mutating a paste currently open in GUI editing flows.
   - Primary implementation: [`crates/localpaste_server/src/locks.rs`](../../crates/localpaste_server/src/locks.rs)

## Paste Edit Lock Semantics

Lock manager type:

- [`PasteLockManager`](../../crates/localpaste_server/src/locks.rs)

Owner identity:

- Locks are owner-aware (`LockOwnerId`), not boolean/id-only.
- The same paste can be held by multiple owners.
- A paste is only fully unlocked after all owners release.

Operations:

1. `acquire(paste_id, owner_id)`
   - Idempotent for the same owner on the same paste.
   - Rejected if the paste is currently under a mutation guard.
2. `release(paste_id, owner_id)`
   - Must match an existing owner hold.
   - Returns a typed `NotHeld` error on owner mismatch.
3. `begin_mutation(paste_id)` / `begin_batch_mutation(paste_ids)`
   - Reserves one or more paste IDs for a mutation critical section.
   - Fails if any target paste is currently held by owners.
   - While the guard is alive, new acquisitions on those IDs are rejected.

Poison handling:

- Lock-manager poisoning is mapped to typed lock errors, not panics.

## Handler/Worker Enforcement

Single-paste mutation paths:

- API update/delete acquire `begin_mutation` before storage mutation.
  - [`crates/localpaste_server/src/handlers/paste.rs`](../../crates/localpaste_server/src/handlers/paste.rs)

Folder delete path:

- Folder delete computes affected descendant paste IDs under the folder transaction lock.
- It then acquires a batch mutation guard for exactly that set before migration/delete proceeds.
  - Core guarded entrypoint: [`crates/localpaste_core/src/folder_ops.rs`](../../crates/localpaste_core/src/folder_ops.rs)
  - API handler usage: [`crates/localpaste_server/src/handlers/folder.rs`](../../crates/localpaste_server/src/handlers/folder.rs)
  - GUI backend parity usage: [`crates/localpaste_gui/src/backend/worker/folder.rs`](../../crates/localpaste_gui/src/backend/worker/folder.rs)

## GUI Ownership

Each GUI app instance uses a stable lock owner ID for its session lifetime:

- Acquire on selection/open.
- Release on deselection/drop.
- Primary paths:
  - [`crates/localpaste_gui/src/app/mod.rs`](../../crates/localpaste_gui/src/app/mod.rs)
  - [`crates/localpaste_gui/src/app/state_ops.rs`](../../crates/localpaste_gui/src/app/state_ops.rs)

## Error Surface Contract

Lock conflicts map to `423 Locked` on API paths.
Unexpected lock-manager failures map to storage/internal errors.

Shared mapping helpers:

- [`map_paste_mutation_lock_error`](../../crates/localpaste_server/src/locks.rs)
- [`map_folder_delete_lock_error`](../../crates/localpaste_server/src/locks.rs)

## Regression Coverage

Primary lock tests:

- Server lock manager unit tests:
  - [`crates/localpaste_server/src/locks.rs`](../../crates/localpaste_server/src/locks.rs)
- API integration lock behavior:
  - [`crates/localpaste_server/tests/api_integration.rs`](../../crates/localpaste_server/tests/api_integration.rs)
- GUI/backend parity + lock behavior:
  - [`crates/localpaste_gui/tests/headless_workflows.rs`](../../crates/localpaste_gui/tests/headless_workflows.rs)
