# Engineering Backlog

This file is the primary engineering backlog for deferred technical follow-ups.
Status uses the same checklist markers as other dev docs:

- [ ] not started
- [~] in progress / partially done
- [x] done

## Current Items

- [ ] Split `LocalPasteApp` into domain state groups (`EditorState`, `HighlightState`, `SearchState`, `UiState`) to reduce coupling and simplify test harness construction.
- [ ] Extract the virtual input-routing/control-flow block from `LocalPasteApp::update` into a dedicated per-frame input pipeline API.
- [ ] Add CI-friendly perf microbench coverage (list-from-metadata and highlight/layout paths) to catch regressions earlier than manual perf runs.
- [ ] Revisit backend query-cache invalidation strategy with metadata-aware generations/in-place cache patching where correctness permits.
- [ ] Decide whether legacy process-list diagnostics in `Database::new` should be retained or retired now that owner-lock probing is primary.
- [ ] Make dev validation deterministic under concurrent local runs (ephemeral smoke-test port selection and isolated `CARGO_TARGET_DIR`).
- [ ] Complete manual newline-burst highlight perf recheck (per [gui-perf-protocol.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md)) and capture refreshed perf evidence in release notes.
- [ ] Enforce key/value identity checks for primary paste rows (`tree` key must match decoded `Paste.id`) and define repair behavior for mismatches.
- [ ] Narrow `PasteDb` mutation API so folder assignment changes cannot bypass folder-count transaction paths.
- [ ] Track folder-count decrement failures with a persistent repair marker and run opportunistic `reconcile_folder_invariants` recovery in long-lived processes.
- [ ] Add an explicit runtime reconcile entrypoint/scheduler for metadata indexes so degraded states are repaired without restart.
- [ ] Add low-cost semantic drift detection for `pastes_meta` rows (without full content deserialization in list/search hot paths), e.g. metadata hash/version marker validation at write/reconcile time.
- [ ] Make backup creation crash-safe via temp-directory staging + atomic rename, and define cleanup rules for interrupted backup artifacts.
- [ ] Add structured output mode (`--output json`) for `check-ast-dupes` with stable category/severity/score fields and policy-aware `--fail-on-findings` handling.
- [ ] Add doc/help contract checks in CI (verify key `--help` sections and command examples stay synchronized with behavior).
- [ ] Review `check-ast-dupes --include-tests` hit in `crates/localpaste_gui/src/app/virtual_editor/layout.rs` (high-similarity test bodies) and decide whether to keep as-is with rationale or consolidate shared setup.
- [ ] Review `check-ast-dupes` hit between `TransactionOps::create_paste_with_folder` and `TransactionOps::move_paste_between_folders` in `crates/localpaste_core/src/db/transactions.rs`; either consolidate shared logic or document intentional duplication.
