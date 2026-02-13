# Cross-Layer Invariant Audit Matrix (2026-02-13)

Scope: `localpaste_server`, `localpaste_gui` backend worker, and `localpaste_core`.

Legend:
- `Canonical` = `pastes` tree and `folders` tree truth.
- `Derived` = `pastes_meta`, `pastes_by_updated`, and folder counts.
- `Err contract`:
  - `no-commit` means `Err` implies no canonical mutation committed.
  - `best-effort-derived` means canonical may commit while derived maintenance can fail without surfacing `Err`.

## Mutation Paths

| Entrypoint | Canonical writes | Derived writes | Required guards | Enforcement layer(s) | Err contract | Existing test coverage | Gap status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `POST /api/paste` | `pastes.create` | meta/index + optional folder count | size, optional folder existence | `server::handlers::paste::create_paste` + `core::TransactionOps` | `best-effort-derived` for meta/index | `api_integration::test_paste_lifecycle`, max size tests, folder tests | Covered |
| `PUT /api/paste/:id` | `pastes.update` or CAS move | meta/index + optional folder counts | lock, size, folder existence | `server::handlers::paste::update_paste` + `core::TransactionOps` | `no-commit` for CAS/update errors; `best-effort-derived` for meta/index | locked update + folder move tests | Covered |
| `DELETE /api/paste/:id` | `pastes.delete_and_return` | meta/index remove + folder count decrement | lock, existence | `server::handlers::paste::delete_paste` + `core::TransactionOps` | canonical delete should not false-fail on folder-count drift | locked delete tests | Covered |
| `DELETE /api/folder/:id` | folder tree delete + paste migrations | folder hierarchy + paste meta/index reconcile | reject when locked descendant would be migrated | `server::handlers::folder::delete_folder` + `core::folder_ops` | `no-commit` when precheck fails | locked descendant test in server integration | Covered |
| GUI `CoreCmd::CreatePaste` | `pastes.create` | meta/index | size | `gui::backend::worker` | `best-effort-derived` for meta/index | backend tests + headless workflows | Covered |
| GUI `CoreCmd::UpdatePaste` | `pastes.update` | meta/index | size (owner-write allowed) | `gui::backend::worker` | `best-effort-derived` for meta/index | backend tests + app save tests | Covered |
| GUI `CoreCmd::UpdatePasteVirtual` | `pastes.update` | meta/index | size (owner-write allowed) | `gui::backend::worker` | `best-effort-derived` for meta/index | backend virtual update + app save tests | Covered |
| GUI `CoreCmd::UpdatePasteMeta` (folder unchanged) | `pastes.update` | meta/index | optional folder existence, metadata normalization | `gui::backend::worker` | `best-effort-derived` for meta/index | backend metadata tests | Covered |
| GUI `CoreCmd::UpdatePasteMeta` (folder move) | CAS update in `pastes` | folder count reserve/rollback + meta/index | optional folder existence, CAS expected folder | `gui::backend::worker` + `core::TransactionOps` | `no-commit` on CAS error; rollback destination reservation | backend metadata + core concurrency tests | Covered |
| GUI `CoreCmd::DeletePaste` | `pastes.delete_and_return` | meta/index remove + folder count decrement | existence | `gui::backend::worker` + `core::TransactionOps` | canonical delete remains source of truth | backend delete/folder-count tests | Covered |
| GUI `CoreCmd::DeleteFolder` | folder delete + paste migrations | metadata/index reconcile | reject locked descendants in delete set | `gui::backend::worker` + `core::folder_ops` | `no-commit` on lock precheck fail | headless workflow lock test | **Fixed in audit** |
| GUI `CoreCmd::CreateFolder` | `folders.create` | none | optional parent existence | `gui::backend::worker` | `no-commit` | backend folder tests | Covered |
| GUI `CoreCmd::UpdateFolder` | `folders.update` | none | parent existence, self-parent, cycle prevention | `gui::backend::worker` | `no-commit` | backend folder tests | Covered |
| CLI `lpaste new` | API-backed create | API-backed | API guards | `localpaste_cli` via server routes | follows API | cli smoke + API integration | Covered |
| CLI `lpaste delete` | API-backed delete | API-backed | API lock/existence guards | `localpaste_cli` via server routes | follows API | cli smoke + API integration | Covered |
| Tool `generate-test-data` create | `pastes.create` or `TransactionOps::create_paste_with_folder` | meta/index + folder count | none (test tooling path) | `localpaste_tools` | best-effort for data-gen workloads | tool integration smoke usage | Out of user-path scope |
| Tool `generate-test-data --clear` | iterative `pastes.delete`, `folders.delete` | derived maintenance via core calls | none (offline test-data reset) | `localpaste_tools` | best-effort for tooling | tool usage docs | Out of user-path scope |
| Startup reconcile (`Database::new`) | possible rewrite from canonical into derived trees | full derived rebuild | marker/version/dirty detection + drift checks | `core::db::Database::new` -> `PasteDb::needs_reconcile_meta_indexes`/`reconcile_meta_indexes` | startup repair path | core DB startup/reconcile tests | Covered (with known equal-length blind spot mitigated by runtime fallback) |
| Runtime fallback `list_meta` / `search_meta` | none | fallback to canonical projection | detect ghosts/decode mismatch/index inconsistencies | `core::db::paste` | no mutation | core paste tests for fallback | Covered |

## Error-Semantic Classification

| Function | Contract summary | Audit verdict |
| --- | --- | --- |
| `TransactionOps::create_paste_with_folder` | `Err` only when canonical create fails or pre-check fails; folder-count rollback is best effort | Covered by existing and audit tests |
| `TransactionOps::move_paste_between_folders` | temporary destination reservation must roll back on non-commit errors | Covered; deterministic failpoint test added |
| `TransactionOps::delete_paste_with_folder` | canonical delete is source-of-truth; folder count decrement failure is logged and non-fatal | Covered |
| `folder_ops::delete_folder_tree_and_migrate` | migration/delete errors surface; lock precheck handled by caller layers | Covered in server + backend after fix |
| `FolderDb::update` | serialization/update errors must preserve old bytes | Covered by new corrupt-record test |
| `FolderDb::update_count` | serialization/update errors must preserve old bytes | Covered by existing corrupt-record test |

## Non-User Paths and Rationale

- `localpaste_tools` direct mutations are treated as controlled tooling operations for test-data generation.
- `reconcile_meta_indexes` is an administrative repair path; invariants focus on external visibility correctness and recoverability.
- lock recovery (`--force-unlock`) and backup paths do not mutate paste/folder domain rows directly and are excluded from mutation guard policy.
