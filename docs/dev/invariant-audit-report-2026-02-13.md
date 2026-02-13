# Cross-Layer Invariant Audit Report (2026-02-13)

Audit order: `B -> C -> E -> D -> F -> A`

Validation baseline:
- `cargo check --workspace --all-targets --all-features`
- `cargo test -p localpaste_core`
- `cargo test -p localpaste_server`
- `cargo test -p localpaste_gui --all-targets`

## Findings (Severity Ordered)

## 1. High: GUI backend folder delete bypassed locked-descendant invariant

- Class: `C` (embedded-vs-headless parity), `A` (indirect mutation bypass)
- Surface: GUI `CoreCmd::DeleteFolder` path
- Impact: backend-initiated folder delete could migrate a locked paste, while API route correctly rejected this with `423`.
- Reproduction (pre-fix):
1. In embedded mode, create folder + paste in folder.
2. Lock paste id in shared lock manager.
3. Issue GUI backend `CoreCmd::DeleteFolder`.
4. Observe migration/delete proceeds without lock rejection.
- Fix:
  - Added lock precheck in `gui::backend::worker` mirroring server behavior before `delete_folder_tree_and_migrate`.
  - Added shared-lock spawn path (`spawn_backend_with_locks`) and wired app runtime to pass the shared lock manager.
- Tests:
  - `crates/localpaste_gui/tests/headless_workflows.rs`: `locked_descendant_blocks_backend_folder_delete`

## 2. Medium: transaction rollback-on-error path lacked deterministic coverage

- Class: `B` (`Err` semantics vs state)
- Surface: `TransactionOps::move_paste_between_folders`
- Impact: reservation rollback logic existed but did not have deterministic error-window test coverage.
- Reproduction strategy:
  - Inject failure after destination count reservation, before CAS update.
- Fix:
  - Added test-only failpoint (`MoveAfterDestinationReserveOnce`) in `localpaste_core::db`.
  - Added deterministic rollback-state assertions.
- Tests:
  - `crates/localpaste_core/src/db/tests.rs`: `test_move_between_folders_injected_error_rolls_back_reservation_and_preserves_state`

## 3. Medium: equal-length semantic index mismatch is not proactively reconciled at startup

- Class: `D` (durability/reconcile guarantees)
- Surface: `PasteDb::needs_reconcile_meta_indexes` marker + size checks
- Impact: a semantic mismatch with equal lengths can pass startup checks; correctness then relies on runtime canonical fallback.
- Outcome:
  - External correctness is preserved by runtime fallback paths in `list_meta` and `search_meta`.
  - Startup does not currently force reconcile in this specific blind spot.
- Tests:
  - `crates/localpaste_core/src/db/tests.rs`: `test_equal_length_index_mismatch_does_not_leak_stale_metadata`
- Status: tracked residual risk (performance/reconcile eagerness), no externally visible stale result leak.

## 4. Low: real backend-generated virtual-save error propagation needed explicit UI-state test

- Class: `F` (async error propagation)
- Surface: `CoreCmd::UpdatePasteVirtual` oversized content
- Impact: behavior existed but was only indirectly covered by synthetic app-event tests.
- Fix:
  - Added real-backend integration-style app test that receives actual `CoreEvent::Error` and validates UI save state transitions.
- Tests:
  - `crates/localpaste_gui/src/app/tests/save_and_metadata.rs`: `real_backend_virtual_save_error_updates_ui_state`

## 5. Low: folder update error-path coverage gap

- Class: `B`
- Surface: `FolderDb::update` corrupt-record behavior
- Impact: parity with `update_count` corruption handling was not explicitly tested.
- Fix:
  - Added corrupt-record preservation test for `FolderDb::update`.
- Tests:
  - `crates/localpaste_core/src/db/tests.rs`: `test_folder_update_preserves_corrupt_record_on_error`

## Added Race Coverage

- `E1` backend virtual update vs API delete:
  - `crates/localpaste_gui/tests/headless_workflows.rs`: `backend_virtual_update_and_api_delete_race_keeps_consistent_visibility`
- `E2` backend folder move metadata update vs API folder delete:
  - `crates/localpaste_gui/tests/headless_workflows.rs`: `backend_folder_move_and_api_folder_delete_race_preserves_folder_counts`

## Startup Reconcile Coverage Added

- `D1` derived-only rows repaired on startup:
  - `crates/localpaste_core/src/db/tests.rs`: `test_database_new_reconciles_derived_only_rows_on_startup`

## Inventory Artifact

- Full mutation-path and contract matrix:
  - `docs/dev/invariant-audit-matrix.md`

## Residual Risks

1. Startup reconcile does not deep-validate equal-length semantic mismatches; runtime fallback preserves correctness but may defer repair.
2. Tooling mutation paths (`generate-test-data`) remain intentionally out of user-path guard policy; they are documented as controlled test-data operations.
