# Engineering Backlog

This file is the canonical engineering backlog for deferred technical follow-ups.
Status uses the same checklist markers as other dev docs:

- [ ] not started
- [~] in progress / partially done
- [x] done

## Current Items

- [ ] Split `LocalPasteApp` into domain state groups (`EditorState`, `HighlightState`, `SearchState`, `UiState`) to reduce coupling and simplify test harness construction.
- [ ] Extract the virtual input-routing/control-flow block from `LocalPasteApp::update` into a dedicated per-frame input pipeline API.
- [ ] Add CI-friendly perf microbench coverage (list-from-metadata and highlight/layout paths) to catch regressions earlier than manual perf runs.
- [x] Migrate storage backend from sled to redb 3.x with typed table definitions and atomic multi-table commits.
- [ ] Revisit backend query-cache invalidation strategy with metadata-aware generations/in-place cache patching where correctness permits.
- [ ] Decide whether legacy process-list diagnostics in `Database::new` should be retained or retired now that owner-lock probing is primary.
- [x] Remove sled `update_and_fetch` closure side-channel handling from paste/folder mutation paths.
- [ ] Make dev validation deterministic under concurrent local runs (ephemeral smoke-test port selection and isolated `CARGO_TARGET_DIR`).
- [ ] Complete manual newline-burst highlight perf recheck (per [gui-perf-protocol.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md)) and close remaining parity gate.
- [ ] Enforce key/value identity checks for canonical paste rows (`tree` key must match decoded `Paste.id`) and define repair behavior for mismatches.
- [ ] Narrow `PasteDb` mutation API so folder assignment changes cannot bypass folder-count transaction paths.
- [ ] Track folder-count decrement failures with a persistent repair marker and run opportunistic `reconcile_folder_invariants` recovery in long-lived processes.
- [ ] Add an explicit runtime reconcile entrypoint/scheduler for metadata indexes so degraded states are repaired without restart.
- [ ] Add low-cost semantic drift detection for `pastes_meta` rows (without canonical content deserialization in list/search hot paths), e.g. metadata hash/version marker validation at write/reconcile time.
- [ ] Make backup creation crash-safe via temp-directory staging + atomic rename, and define cleanup rules for interrupted backup artifacts.
- [ ] Add structured output mode (`--output json`) for `check-ast-dupes` with stable category/severity/score fields and policy-aware `--fail-on-findings` handling.
- [ ] Add doc/help contract checks in CI (verify key `--help` sections and command examples stay synchronized with behavior).
- [ ] Retire `Database::flush()` compatibility plumbing from GUI worker shutdown (`CoreCmd::Shutdown { flush }`) once UI/worker shutdown contracts are updated for redb commit semantics.
- [ ] Re-evaluate whether `db/tests/basic_ops.rs::test_create_database_and_flush_noop` should remain after the `Database::flush()` compatibility API is removed.
