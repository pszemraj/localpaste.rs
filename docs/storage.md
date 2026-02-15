# Storage Contract

This is the canonical storage and on-disk compatibility contract for LocalPaste.rs.
Other docs should link here instead of repeating backend/storage details.

## Backend and File Layout

- Storage backend: `redb` 3.x.
- Database file: `DB_PATH/data.redb`.
- Writer coordination lock file: `DB_PATH/db.owner.lock`.
- Embedded GUI endpoint discovery file (GUI runtime only): `DB_PATH/.api-addr`.

## Compatibility Policy

- Until stable release, backward compatibility is not required.
- This project does not provide a sled-to-redb migration path.
- Existing sled-era artifacts are considered incompatible with current runtime.
- If `data.redb` is missing and legacy sled artifacts are present, startup fails with an explicit incompatible-storage error.

## Durability and Atomicity

- redb write transactions are commit-durable.
- LocalPaste relies on `commit()` durability; there is no required explicit flush step.
- Multi-table write operations are executed inside single redb write transactions where invariant coupling matters.

## Operational Expectations

- One writer process per `DB_PATH` at a time.
- Do not run `localpaste-gui` and standalone `localpaste` concurrently on the same `DB_PATH`.
- For isolated local testing, use distinct `DB_PATH` directories.

## Related Canonical Docs

- System architecture: [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md)
- Service operations: [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md)
- Lock semantics: [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md)
- Security posture: [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md)
