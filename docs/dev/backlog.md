# Engineering Backlog

This file tracks deferred technical follow-ups.
Status uses the same checklist markers as other dev docs:

- [ ] not started
- [~] in progress / partially done
- [x] done

## Current Items

- [ ] Split `LocalPasteApp` into domain state groups (`EditorState`, `HighlightState`, `SearchState`, `UiState`) to reduce coupling and simplify test harness construction.
- [ ] Extract the virtual input-routing/control-flow block from `LocalPasteApp::update` into a dedicated per-frame input pipeline API.
- [ ] Add CI-friendly perf microbench coverage (list-from-metadata and highlight/layout paths) to catch regressions earlier than manual perf runs.
- [~] Keep reducing highlight request payload churn: virtual-editor requests now send `Rope` snapshots with worker-side materialization, but the text-edit path still clones owned text and debounce tuning should be revisited with fresh perf traces.
- [ ] Evaluate whether `VisualRowLayoutCache::prefix_rows` should move to a tree/indexed structure (Fenwick/segment-like) if million-line workloads become a target; current tail rebuild (`O(lines-after-edit)`) is intentional for simplicity.
- [~] Avoid full `Vec<HighlightRenderLine>` clone during patch merge (`queue_highlight_patch`) for very large files; one redundant `base.lines.clone()` was removed, but fallback-path `HighlightRender` cloning still needs structural refactor (e.g., base lookup plus move/patch without full render clone).
- [ ] Investigate worker-side highlight diffing that avoids full line-hash scans for every request (especially tiny edits), while preserving patch correctness and stale-result dropping semantics.
- [ ] Revisit backend query-cache invalidation strategy with metadata-aware generations/in-place cache patching where correctness permits.
- [ ] Decide whether legacy process-list diagnostics in `Database::new` should be retained or retired now that owner-lock probing is the preferred path.
- [ ] Make dev validation deterministic under concurrent local runs (ephemeral smoke-test port selection and isolated `CARGO_TARGET_DIR`).
- [ ] Complete manual newline-burst highlight perf recheck (per [gui-perf-protocol.md](gui-perf-protocol.md)), capture refreshed perf evidence in release notes, and decide gate flip from `p95 <= 25 ms` to `p95 <= 16 ms`.
- [ ] Enforce key/value identity checks for authoritative paste rows (`tree` key must match decoded `Paste.id`) and define repair behavior for mismatches.
- [ ] Narrow `PasteDb` mutation API so folder assignment changes cannot bypass folder-count transaction paths.
- [ ] Track folder-count decrement failures with a persistent repair marker and run opportunistic `reconcile_folder_invariants` recovery in long-lived processes.
- [ ] Add an explicit runtime reconcile entrypoint/scheduler for metadata indexes so degraded states are repaired without restart.
- [ ] Add low-cost semantic drift detection for `pastes_meta` rows (without full content deserialization in list/search hot paths), e.g. metadata hash/version marker validation at write/reconcile time.
- [ ] Make backup creation crash-safe via temp-directory staging + atomic rename, and define cleanup rules for interrupted backup artifacts.
- [ ] Add structured output mode (`--output json`) for `check-ast-dupes` with stable category/severity/score fields and policy-aware `--fail-on-findings` handling.
- [ ] Add doc/help contract checks in CI (verify key `--help` sections and command examples stay synchronized with behavior).
- [ ] Revisit `TransactionOps` create/delete/move wrapper consolidation with a lock-safe transaction template only if we can preserve operation-specific invariants and error semantics without reducing readability.
- [ ] Resolve `check-ast-dupes` finding in `crates/localpaste_core/src/db/transactions.rs` (`create_paste_with_folder` vs `move_paste_between_folders`) by extracting shared transactional structure while keeping operation-specific invariant checks explicit.
- [ ] De-duplicate overlapping heuristic detection matrix tests in `crates/localpaste_core/src/detection/tests.rs` (`heuristic_detects_expanded_fallback_languages` vs `heuristic_handles_shebang_and_import_conflict_matrix`) without reducing scenario coverage.
- [ ] Evaluate a shared test bootstrap utility for temporary DB + backend event receive flows across GUI/server/core tests while keeping unit-vs-integration boundaries explicit (avoid forcing production API exposure only for tests).
- [ ] Re-evaluate whether `LocalPasteApp::{active_text_len_bytes, active_text_chars, active_revision, active_snapshot}` should remain separate explicit helpers or move behind a single active-buffer abstraction; keep separate until a clear readability/perf win is demonstrated.
- [x] Add explicit `Paste as new paste` UX (`Ctrl/Cmd+Shift+V` + command palette action) so new-paste clipboard flow does not depend on editor blur/focus heuristics.
