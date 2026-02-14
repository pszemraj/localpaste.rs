# Complete Folder Audit Report (2026-02-13)

Audit order executed: `B -> C -> E -> D -> F -> A`.

Matrix artifact:
[folder-audit-matrix-2026-02-13.md](folder-audit-matrix-2026-02-13.md).

## Validation Gate Results

Executed and passing:
- `cargo fmt --all`
- `cargo clippy --workspace --all-targets --all-features`
- `cargo check --workspace --all-targets --all-features`
- `cargo test -p localpaste_core`
- `cargo test -p localpaste_server`
- `cargo test -p localpaste_gui --all-targets`
- `cargo test -p localpaste_tools`
- `rustdoc-checker crates --strict`

API/core smoke executed against isolated DB path:
- Built `localpaste` + `lpaste`.
- Ran CLI CRUD (`new/list/search/get/delete`).
- Ran folder extension (parent+child+paste, delete parent, verify unfiled migration + empty folders + list/search consistency).
- Restarted server and verified migrated paste persisted as unfiled.

## Severity-Ordered Findings and Remediation

## 1. High (B): Create-with-folder could leave orphan assignment across race windows

- Risk: folder disappears mid-create and canonical paste can be left with missing `folder_id`.
- Remediation:
  - Hardened `TransactionOps::create_paste_with_folder` with assignability checks before reservation, after reservation, and after canonical create.
  - Added compensating delete + count rollback when destination becomes unassignable after canonical insert.
  - Added deterministic failpoints for pre/post create race windows.
- File references:
  - `crates/localpaste_core/src/db/mod.rs`
  - `crates/localpaste_core/src/db/tests.rs`
- Reproduction/tests:
  - `test_create_with_folder_injected_error_rolls_back_reservation_and_leaves_no_paste`
  - `test_create_with_folder_rejects_destination_deleted_after_reservation_without_orphan`
  - `test_create_with_folder_destination_deleted_after_create_rolls_back_canonical_insert`

## 2. High (B/E): Folder move/delete race and post-commit destination drift protection

- Risk: move path may race destination deletion and drift into inconsistent folder assignment/count state.
- Remediation:
  - `move_paste_between_folders` now validates destination assignability before reservation and immediately before CAS.
  - Added post-CAS destination revalidation with compensating revert + reservation rollback if destination became unassignable.
  - Retained bounded CAS retries and rollback behavior on conflicts/errors.
- File references:
  - `crates/localpaste_core/src/db/mod.rs`
  - `crates/localpaste_core/src/folder_ops.rs`
- Reproduction/tests:
  - `test_move_between_folders_rejects_destination_deleted_after_reservation`
  - `delete_folder_tree_and_concurrent_move_preserve_no_orphan_and_counts`
  - `backend_folder_move_and_api_folder_delete_race_preserves_folder_counts`

## 3. High (C/A): Server vs GUI duplicate lock-policy logic and indirect mutation bypass risk

- Risk: divergent lock precheck behavior for folder delete across server and GUI backend.
- Remediation:
  - Removed duplicate implementations and centralized on `core::folder_ops::delete_folder_tree_and_migrate_guarded`.
  - Server + GUI now both call the same helper before delete-tree migration.
- File references:
  - `crates/localpaste_core/src/folder_ops.rs`
  - `crates/localpaste_server/src/handlers/folder.rs`
  - `crates/localpaste_gui/src/backend/worker.rs`
- Reproduction/tests:
  - `test_delete_folder_rejects_when_descendant_paste_is_locked`
  - `locked_descendant_blocks_backend_folder_delete`

## 4. High (C/A): Assignability guard did not cover delete-marker state on all write surfaces

- Risk: new assignments into folder delete set during in-progress tree delete.
- Remediation:
  - Added shared assignability helper (`ensure_folder_assignable`: exists + not delete-marked).
  - Applied across server paste/folder handlers and GUI backend metadata/folder parent flows.
  - Added parity tests proving both server and GUI reject assignment into marked folders.
- File references:
  - `crates/localpaste_core/src/folder_ops.rs`
  - `crates/localpaste_server/src/handlers/paste.rs`
  - `crates/localpaste_server/src/handlers/folder.rs`
  - `crates/localpaste_gui/src/backend/worker.rs`
  - `crates/localpaste_server/tests/api_integration.rs`
  - `crates/localpaste_gui/src/backend/mod.rs`
  - `crates/localpaste_gui/tests/headless_workflows.rs`
- Reproduction/tests:
  - `test_server_rejects_assignments_to_delete_marked_folder`
  - `backend_rejects_assignment_into_delete_marked_folder`
  - `folder_delete_marker_rejects_new_assignments_server_and_gui`
  - `create_with_folder_rejects_when_folder_is_marked_for_delete`

## 5. Medium (D): Startup folder invariant reconcile coverage gap

- Risk: stale delete markers, folder count drift, and orphan folder refs persisting across restart.
- Remediation:
  - `Database::new` now clears stale folder delete markers and runs `reconcile_folder_invariants`.
  - Reconcile repairs canonical orphan folder refs and exact folder counts from canonical rows.
  - Startup metadata reconcile remains in place; post-folder reconcile metadata check/reconcile added.
- File references:
  - `crates/localpaste_core/src/db/mod.rs`
  - `crates/localpaste_core/src/folder_ops.rs`
  - `crates/localpaste_core/src/db/folder.rs`
  - `crates/localpaste_core/src/db/tests.rs`
- Reproduction/tests:
  - `test_database_new_reconciles_folder_count_drift`
  - `test_database_new_reconciles_orphan_folder_refs`
  - `test_database_new_clears_stale_folder_delete_markers`

## 6. Medium (A/C): Tooling mutation path used weaker direct clears

- Risk: `generate-test-data --clear` path bypassed transactional folder-aware deletion behavior.
- Remediation:
  - Added `clear_existing_data` using transactional paste delete + folder tree delete migration helper.
  - Added invariant checker and test coverage for generation+clear roundtrip.
- File references:
  - `crates/localpaste_tools/src/main.rs`
- Reproduction/tests:
  - `tooling_generation_and_clear_preserve_folder_invariants`

## 7. Medium (F): Real backend save-error propagation verification

- Status:
  - Existing real-backend save-error test remains passing after folder hardening.
  - No regression introduced in save-state transitions.
- File references:
  - `crates/localpaste_gui/src/app/tests/save_and_metadata.rs`
- Reproduction/tests:
  - `real_backend_virtual_save_error_updates_ui_state`

## Dedup/Consolidation Outcomes

Consolidated duplicate policy logic:
- Removed duplicated locked-descendant folder-delete scanning from server and GUI backend.
- Centralized shared lock-delete precheck and assignability logic in `localpaste_core::folder_ops`.

Dead/semantic-duplicate behavior reduced:
- Tooling clear flow now uses the same invariant-preserving deletion semantics as user-facing paths instead of custom direct deletes.

## Residual Risks

1. Sled cross-tree atomicity is still emulated; canonical rows remain source of truth with reconcile/fallback repair for derived structures.
2. Post-commit move/create compensation paths depend on best-effort follow-up writes under rare race windows; coverage is strong but cannot make cross-tree operations physically atomic.
3. Windows policy wrapper blocked automated removal of one temporary smoke DB directory during teardown; runtime correctness checks still completed.

## Recommended Remediation Order for Any Future Folder Work

1. Keep all new mutation paths on shared core helpers (`ensure_folder_assignable`, `delete_folder_tree_and_migrate_guarded`, transaction ops).
2. Add deterministic failpoint coverage before changing transaction sequencing.
3. Preserve startup reconcile execution order (`meta reconcile -> folder marker clear/reconcile -> meta recheck`).
4. Require parity tests when touching both API handlers and GUI backend worker.
