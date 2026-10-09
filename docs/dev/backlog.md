# Engineering Backlog

- [ ] not started
- [~] in progress / partially done

## Current Items

- [ ] Split `LocalPasteApp` into domain state groups (`EditorState`, `HighlightState`, `SearchState`, `UiState`) to reduce coupling and simplify test harness construction.
- [ ] Extract the virtual input-routing/control-flow block from `LocalPasteApp::update` into a dedicated per-frame input pipeline API.
- [ ] Add local perf microbench coverage (list-from-metadata and highlight/layout paths) to catch regressions earlier than manual perf runs.
- [~] Keep reducing highlight request payload churn: virtual-editor requests now send `Rope` snapshots with worker-side materialization; debounce tuning should be revisited with fresh perf traces.
- [~] Avoid full `Vec<HighlightRenderLine>` clone during patch merge (`queue_highlight_patch`) for very large files; one redundant `base.lines.clone()` was removed, but fallback-path `HighlightRender` cloning still needs structural refactor (e.g., base lookup plus move/patch without full render clone).
- [ ] Investigate worker-side highlight diffing that avoids full line-hash scans for every request (especially tiny edits), while preserving patch correctness and stale-result dropping semantics.
- [ ] Revisit backend query-cache invalidation strategy with metadata-aware generations/in-place cache patching where correctness permits.
- [ ] Replace the GUI's 30s refresh poll with invalidation for embedded-API writes; GUI backend mutations already refresh immediately through events.
- [ ] Add a synthetic same-frame focus-loss + `Ctrl+Delete` regression once the per-frame input pipeline is extracted, so the paste-delete shortcut and editor word-delete ownership stay covered under simultaneous pointer/key input.
- [ ] Decide whether legacy process-list diagnostics in `Database::new` should be retained or retired now that owner-lock probing is the preferred path.
- [ ] Make dev validation deterministic under concurrent local runs (ephemeral smoke-test port selection and isolated `CARGO_TARGET_DIR`).
- [ ] Finish native macOS GUI smoke/probe before merging phase-two UX hardening; cover keyboard ownership, ~~IME `cursor_rect` placement~~, and cold-start visibility because headless tests stop at the rendering boundary. Linux X11 and Windows navigation probe automation are covered in [gui-notes.md#navigation-probe](gui-notes.md#navigation-probe).
- [ ] Complete Windows release checks still missing from native navigation coverage: MSI installation, startup with a clean GUI profile, ~~composing IME~~, physical hold/release gestures, and intercepted F1. Synthetic chords and isolated databases do not establish those results. IME/CJK testing is optional; additional testing and insights welcome.
- [ ] Plan egui/eframe `0.34+` as a standalone migration with text-layout regression coverage and real Windows cold-start smoke testing; keep the current dependency line at `0.33.3` until that migration lands.
- [ ] Preserve paste shortcut modifiers in the native egui-winit backend. Under CPU load on X11, `Ctrl/Cmd+Shift+V` can arrive as `Paste` with neither original modifiers nor a V key-down event, so it inserts into the current editor. The app handles Paste-only input when modifiers survive; completing this fix requires the backend to retain the press chord before clipboard delivery.
- [ ] Capture newline-burst highlight perf evidence before adopting the tighter [performance gate](gui-perf-protocol.md#scope).
- [ ] Enforce key/value identity checks for authoritative paste rows (`tree` key must match decoded `Paste.id`) and define repair behavior for mismatches.
- [ ] Remove or explicitly re-approve the narrow redb row-shape reader fallbacks before stable release; storage compatibility policy is in [../storage.md#compatibility-policy](../storage.md#compatibility-policy).
- [ ] Narrow `PasteDb` mutation API so folder assignment changes cannot bypass folder-count transaction paths.
- [ ] Track folder-count decrement failures with a persistent repair marker and run opportunistic `reconcile_folder_invariants` recovery in long-lived processes.
- [ ] Add an explicit runtime reconcile entrypoint/scheduler for metadata indexes so degraded states are repaired without restart.
- [ ] Add low-cost semantic drift detection for `pastes_meta` rows (without full content deserialization in list/search hot paths), e.g. metadata hash/version marker validation at write/reconcile time.
- [ ] Migrate remaining test-only delete-undo bundle/capped coverage to the persisted staged-token restore APIs, then remove the `#[cfg(test)]` bundle helpers if they no longer protect distinct invariants.
- [ ] Add a muted second sidebar metadata line when a derived handle exists, now that persisted semantic retrieval metadata and hover/details surfaces are in place.
- [ ] Split history-reset worker failures out from generic `CoreErrorSource::SaveContent` so reset-specific UI transitions and error reporting do not rely on shared save-content handling.
- [ ] Evaluate code-editor-style smart Home behavior for the virtual editor (first non-whitespace <-> column 0) without regressing platform-native line/document key bindings.
- [ ] Decide whether sidebar recency grouping should keep the current rolling seven-day `This Week` behavior, switch to local-calendar week semantics, or rename the bucket to `Last 7 Days`.
- [ ] Define the virtual editor accessibility contract and decide whether to publish a read-only AccessKit text node with caret/selection metadata, or document bespoke editor screen-reader support as out of scope for the current local-tool UX.
- [ ] Make backup creation crash-safe via temp-directory staging + atomic rename, and define cleanup rules for interrupted backup artifacts.
- [ ] Add schema-repair backup retention/rotation so repeated upgrades cannot accumulate unbounded snapshots in the DB directory.
- [ ] Add structured output mode (`--output json`) for `check-ast-dupes` with stable category/severity/score fields and policy-aware `--fail-on-findings` handling.
- [ ] Revisit `check-ast-dupes --include-tests` duplicate/near-miss navigation and discovery input pairs only if a measured cleanup reduces LOC or clarifies behavior: `localpaste_gui/src/app/tests/{keyboard_navigation_audit,discovery_input_order}.rs` covers distinct cursor, event-order, and IME transitions that should stay explicit unless a better structure preserves the invariants. Shared frame/setup code accounts for the discovery-test findings.
- [ ] Add local doc/help contract checks (verify key `--help` sections and command examples stay synchronized with behavior).
- [ ] Expand standalone `verify-gui-packaging.yml` beyond macOS (at least Linux x64) so packaging script regressions are caught before release-tag runs.
- [ ] Revisit `TransactionOps` create/delete/move wrapper consolidation with a lock-safe transaction template only if we can preserve operation-specific invariants and error semantics without reducing readability.
- [ ] Revisit the duplicated selected-paste gate/setup in `crates/localpaste_gui/src/app/version_ui.rs` (`open_history_modal` vs `open_diff_modal`) and extract a shared helper only if it improves readability without hiding modal-specific state.
- [ ] Revisit overlapping heuristic detection matrix tests in `crates/localpaste_core/src/detection/tests.rs` (`heuristic_detects_existing_language_matrix` and `heuristic_detects_fallback_languages_and_conflict_matrix`) only if coverage stays explicit.
- [ ] Revisit Markdown-vs-YAML bias for top-level `- key: value` bullet-note content (no `---` doc start, no nesting) and decide whether product UX should prefer Markdown over YAML in that narrow shape.
- [ ] Evaluate a shared test bootstrap utility for temporary DB + backend event receive flows across GUI/server/core tests while keeping unit-vs-integration boundaries explicit (avoid forcing production API exposure only for tests).
- [ ] Re-evaluate whether `LocalPasteApp::{active_text_len_bytes, active_text_chars, active_revision, active_snapshot}` should remain separate explicit helpers or move behind a single active-buffer abstraction; keep separate until a clear readability/perf win is demonstrated.
- [ ] Replace real `sleep(1100ms)` version-history/retention test waits with an injectable clock or deterministic snapshot timestamp hook across core and server tests.
- [~] Extract focused helpers from `crates/localpaste_gui/src/app/state_ops.rs` (sidebar projection/navigation now lives in `state_ops/list_projection.rs`) before adding more GUI workflows. Version preview caching now lives in `version_ui/cache.rs`, returning `version_ui.rs` below the normal warning threshold.
- [ ] Extract `render_virtual_editor_panel` into smaller focused helpers when the virtual-input pipeline work lands; keep the current monolithic method stable until then.
- [ ] Decide whether YAML alias-only markers (`*alias`) should count as distinctive YAML structure or remain rejected as ambiguous prose.

- [ ] Consolidate the paired sidebar-to-Find tests in `crates/localpaste_gui/src/app/tests/editor_find.rs` if a table keeps the contrasting body-match and metadata-only assertions readable. The duplicate-code audit reports shared setup; behavior is intentionally distinct.

`app/tests/virtual_editor_behaviors.rs` is intentionally on the LOC watchlist while remaining below the 1,000-line limit. Its non-LF regression keeps edit byte offsets, rendered caret geometry, Undo/Redo, typing, and CRLF navigation/selection in one scenario so correct buffer contents cannot hide an invisible caret. Preserve these assertions when reorganizing the test module.

`detection/mod.rs` remains below 1,000 lines; its command lexical helpers are shared with semantic classification to keep both paths consistent. `detection/tests.rs` keeps the source/prose matrices together so fallback and Magika configurations exercise the same boundaries. The AST visibility suggestion for `detection/heuristic.rs::looks_like_python_source` is a false positive: `detection/mod.rs` re-exports it for `semantic.rs`, so crate visibility is required.
