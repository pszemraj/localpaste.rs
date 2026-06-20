# Storage Contract

## Backend And File Layout

- Storage backend: `redb` 3.x.
- Database file: `DB_PATH/data.redb`.
- Writer coordination lock file: `DB_PATH/db.owner.lock`.
- Embedded GUI endpoint discovery file (GUI runtime only): `DB_PATH/.api-addr`.

## Tables And Projections

Authoritative tables:

- `pastes`: full paste rows.
- `folders`: folder rows.
- `folders_deleting`: in-progress delete markers for folder-tree operations.

Derived/index tables:

- `pastes_meta`: list/search/filter projection, including derived retrieval metadata (`kind`, compact `handle`, top `terms`).
- `pastes_meta_state`: projection schema marker; startup rebuilds `pastes_meta` from authoritative paste rows when the marker is missing or stale.
- `pastes_by_updated`: recency index keyed by `(reverse_millis, paste_id)`.
- `paste_versions_meta`: newest-first historical snapshot metadata per paste.
- `paste_versions_content`: historical snapshot content keyed by `(paste_id, version_id_ms)`.
- `deleted_pastes`: GUI delete-undo staging rows keyed by undo token.
- `deleted_paste_versions_meta`: staged historical snapshot metadata keyed by undo token.
- `deleted_paste_versions_content`: staged historical snapshot content keyed by `(undo_token, version_id_ms)`.

## Version History Storage

Content-changing writes may archive the outgoing head content as a historical snapshot. Snapshot interval and retention settings are listed in [security.md#environment-variables](security.md#environment-variables).

History reset workflows may temporarily preserve one confirmed reset target in addition to the normal retention limit while saving the current head before the reset. This prevents the selected rollback target from being pruned between user confirmation and the backend reset transaction. GUI reset transactions archive the outgoing head as a recovery snapshot before restoring the selected historical version.

Retention pruning keeps the newest configured snapshot metadata rows and removes older matching `paste_versions_content` rows in the same write transaction that records a new version.

GUI delete undo moves the paste row and its version rows into `deleted_*` staging tables inside the same write transaction that removes the active rows and updates folder projections. Undo restore moves those rows back into the active tables by token. Expired or overflowed undo tokens are discarded by the GUI backend worker.

## Compatibility Policy

- Until stable release, backward compatibility is not required.
- Pre-stable redb row-shape changes are not migrated; current builds expect current bincode row schemas and may reject older `data.redb` files created by earlier pre-stable builds.
- This project does not provide a sled-to-redb migration path.
- Existing sled-era artifacts are considered incompatible with current runtime.
- If `data.redb` is missing and legacy sled artifacts are present, startup fails with an explicit incompatible-storage error.

> [!CAUTION]
> Sled-era data is not auto-migrated. Use a fresh `DB_PATH` for current builds unless you explicitly convert data yourself.

## Durability and Atomicity

- redb write transactions are commit-durable.
- LocalPaste relies on `commit()` durability; there is no required explicit flush step.
- Multi-table write operations are executed inside single redb write transactions where invariant coupling matters.

## Operational Expectations

- One writer process per `DB_PATH` at a time.
- Do not run `localpaste-gui` and standalone `localpaste` concurrently on the same `DB_PATH`.
- If the GUI owns a DB, use its embedded API for CLI/automation access instead of starting standalone `localpaste` on that path.
- For isolated local testing, use distinct `DB_PATH` directories.
