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

Retention pruning keeps the newest configured snapshot metadata rows and removes older matching `paste_versions_content` rows in the same write transaction. Writes also prune existing over-limit histories when the configured retention limit was lowered, even when the write does not record a new snapshot.

GUI delete undo moves the paste row and its version rows into `deleted_*` staging tables inside the same write transaction that removes the active rows and updates folder projections. Undo restore moves those rows back into the active tables by token. Expired or overflowed undo tokens are discarded by the GUI backend worker.

## Compatibility Policy

- Until stable release, backward compatibility is not required.
- Pre-stable redb row-shape changes are not migrated by default; current builds expect current bincode row schemas and may reject older `data.redb` files created by earlier pre-stable builds.
- Do not add broad pre-stable row-shape migrations just because an older development build wrote different bincode bytes. Prefer current schemas and fresh `DB_PATH` directories unless there is a narrow active-development reason to keep a reader fallback.
- A temporary reader fallback must stay local to the decode helper, have regression coverage, define deterministic defaults for new fields, and remain cheap enough that deleting it later is straightforward.
- Current narrow exceptions are the bincode reader fallbacks for `pastes`, `pastes_meta`, and `paste_versions_meta` rows that predate the language/manual/derived metadata fields. They protect active development databases only; they are not a general compatibility guarantee.
- When an existing redb database needs startup schema or projection repair, startup creates a pre-repair redb backup next to the DB path before mutating derived/index tables; backup failure aborts startup instead of repairing in place without a snapshot.
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
