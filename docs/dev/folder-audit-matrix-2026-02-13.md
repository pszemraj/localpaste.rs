# Complete Folder Audit Matrix (2026-02-13)

Scope: folder-core and folder-adjacent mutation paths across `localpaste_core`, `localpaste_server`, `localpaste_gui`, and `localpaste_tools`.

Locked policy:
- Strict no-orphan canonical folder references.
- Owner-write lock behavior unchanged for direct GUI editing.
- Indirect/bulk mutations touching locked descendants must reject.
- Tooling mutation paths are in-scope and must preserve folder invariants.

Invariant set:
1. Folder reference integrity (`paste.folder_id` is `None` or existing folder).
2. Folder count integrity (`Folder.paste_count` equals canonical ownership count).
3. Tree integrity (cycle-safe parent updates).
4. Lock integrity (delete-tree migration blocked by locked descendant).
5. Error integrity (`Err` must not leave stricter-invariant violations from the attempted operation).
6. Normalization parity (whitespace/empty folder id semantics match server + GUI backend).

## Mutation Path Inventory

| Entrypoint | Canonical writes | Derived writes | Required guards | Enforcement layer(s) | Err contract | Primary tests | Gap status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `POST /api/paste` with `folder_id` | `pastes.create` + folder count reserve | meta/index rows + folder count | size, folder assignable (exists + not deleting) | `server::handlers::paste`, `core::TransactionOps::create_paste_with_folder` | `Err` => no orphan row | `test_invalid_folder_association`, `test_server_rejects_assignments_to_delete_marked_folder`, `test_create_with_folder_*` | Covered |
| `POST /api/paste` without folder | `pastes.create` | meta/index | size | server + core paste db | best-effort derived index | `test_paste_lifecycle` | Covered |
| `PUT /api/paste/:id` folder move/clear | CAS `pastes.update_if_folder_matches` | meta/index + folder counts | lock, size, destination assignable | `server::handlers::paste`, `core::TransactionOps::move_paste_between_folders` | CAS-safe + reservation rollback on non-commit | `test_move_between_folders_*`, `test_update_locked_paste_rejected` | Covered |
| `PUT /api/paste/:id` non-folder fields | `pastes.update` | meta/index | lock, size | server + core paste db | canonical commit preserved on derived failure | `test_paste_lifecycle` | Covered |
| `DELETE /api/paste/:id` | `pastes.delete_and_return` | meta/index remove + folder count decrement | lock | `server::handlers::paste`, `core::TransactionOps::delete_paste_with_folder` | canonical delete source-of-truth | `test_delete_locked_paste_rejected`, `test_delete_uses_folder_from_deleted_record_not_stale_context` | Covered |
| `POST /api/folder` | `folders.create` | none | parent assignable (if provided) | `server::handlers::folder` + core helper | no partial commit on validation error | `test_folder_lifecycle`, `test_whitespace_folder_ids_normalize_consistently` | Covered |
| `PUT /api/folder/:id` | `folders.update` | none | self-parent reject, parent exists+assignable, cycle reject | `server::handlers::folder`, `core::folder_ops::introduces_cycle` | no partial commit on validation error | `test_update_folder_rejects_cycle` | Covered |
| `DELETE /api/folder/:id` | folder tree deletes + paste migrations | paste meta/index reconcile + count movement | guarded descendant mutation set; delete-marker set | `server::handlers::folder`, `core::folder_ops::delete_folder_tree_and_migrate_guarded` | reject on lock conflict; no orphan target refs | `test_delete_folder_rejects_when_descendant_paste_is_locked`, `test_folder_lifecycle` | Covered |
| GUI `CoreCmd::CreatePaste` | `pastes.create` | meta/index | size | `gui::backend::worker` | best-effort derived index | `backend_creates_updates_and_deletes_paste` | Covered |
| GUI `CoreCmd::UpdatePaste` / `UpdatePasteVirtual` | `pastes.update` | meta/index | size; owner-write flow | GUI backend + app state | save errors surface via `CoreEvent::Error` | `backend_virtual_update_persists_content`, `real_backend_virtual_save_error_updates_ui_state` | Covered |
| GUI `CoreCmd::UpdatePasteMeta` with folder change | CAS move | meta/index + folder counts | destination assignable | GUI backend + `TransactionOps::move_paste_between_folders` | rollback reservation on CAS/error | `backend_updates_paste_metadata`, `backend_rejects_assignment_into_delete_marked_folder` | Covered |
| GUI `CoreCmd::DeletePaste` | `pastes.delete_and_return` | meta/index remove + folder count decrement | existence | GUI backend + transaction ops | canonical delete source-of-truth | `backend_delete_paste_updates_folder_count` | Covered |
| GUI `CoreCmd::CreateFolder` | `folders.create` | none | parent assignable (if provided) | GUI backend + core helper | no partial commit on validation error | `backend_create_folder_trims_parent_id` | Covered |
| GUI `CoreCmd::UpdateFolder` | `folders.update` | none | parent exists+assignable, cycle reject | GUI backend + core helper | no partial commit on validation error | `backend_folder_commands_enforce_parenting_rules_and_migrate_on_delete` | Covered |
| GUI `CoreCmd::DeleteFolder` | delete-tree + migration | index reconcile + count movement | guarded descendant mutation set | GUI backend uses `core::delete_folder_tree_and_migrate_guarded` | reject on locked descendant | `locked_descendant_blocks_backend_folder_delete` | Covered |
| Embedded API + GUI parity path | mixed (API + backend on shared db) | mixed | parity for lock/marker/cycle semantics | shared core helpers | equivalent invariants across surfaces | `folder_delete_marker_rejects_new_assignments_server_and_gui`, `locked_paste_blocks_api_*` | Covered |
| Tool `persist_generated_paste` | create with optional folder | meta/index + counts | folder create path via transaction ops when foldered | `localpaste_tools` + core transaction ops | no orphan on error | `persist_generated_paste_updates_folder_count` | Covered |
| Tool `--clear` path | iterative paste deletes + folder tree deletes | index reconcile + count movement | none (offline tooling) but invariant-safe mutation paths only | `localpaste_tools::clear_existing_data` | no orphan/count drift after clear | `tooling_generation_and_clear_preserve_folder_invariants` | Covered |
| Startup `Database::new` | possible canonical folder-id repairs + exact count rewrites | meta/index reconcile as needed + delete marker clear | unconditional marker clear + folder invariant reconcile | `core::db::Database::new` + `reconcile_folder_invariants` | startup repair path | `test_database_new_reconciles_folder_count_drift`, `test_database_new_reconciles_orphan_folder_refs`, `test_database_new_clears_stale_folder_delete_markers` | Covered |
| Runtime metadata fallback (`list_meta`/`search_meta`) | none | read-only canonical fallback | detect index inconsistency/ghosts | `core::db::paste` | no mutation; correctness-preserving fallback | `test_equal_length_index_mismatch_does_not_leak_stale_metadata` | Covered |

## Write Callsite Completeness (A1)

### Canonical paste write primitives
- `PasteDb::create`: routed through server create, GUI create, tooling generation, and transaction helpers.
- `PasteDb::update/update_if_folder_matches`: routed through server update, GUI metadata/content update, folder migration, and transaction helpers.
- `PasteDb::delete_and_return/delete`: routed through server delete, GUI delete, tooling clear, and compensation paths.

### Canonical folder write primitives
- `FolderDb::create/update/delete`: routed through server folder handlers, GUI folder commands, tooling clear, startup reconcile support methods.
- Delete marker tree (`folders_deleting`) writes: `mark_deleting`, `unmark_deleting`, `clear_delete_markers`.

### Indirect/bulk mutators
- `folder_ops::delete_folder_tree_and_migrate` (server + GUI).
- Tooling bulk clear (`clear_existing_data`).
- Startup reconcile (`Database::new` -> `reconcile_folder_invariants`).

All known folder-touching write paths are mapped and tested as `covered + passing` for this pass.
