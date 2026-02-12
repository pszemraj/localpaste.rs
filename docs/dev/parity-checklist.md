# Plan-Aligned Rewrite Checklist

This checklist tracks PLAN.md phases and merge-gate readiness for the native rewrite.
Strict parity is NOT required - we only port or replace behaviors that match the plan and desired UX.

Status key:

- [x] Done
- [~] Partial
- [ ] Not started

Decision key:

- [Keep] Same behavior as legacy
- [Replace] New approach replaces legacy behavior
- [Drop] Intentionally not ported

---

## Audit Snapshot (2026-02-12)

Codebase audit against this checklist found:

- Auto language re-detection on update was the essential language parity gap; it is now implemented in core update flow when `language_is_manual = false`.
- Compact metadata now uses an inline header row (title + chips + quick actions), with infrequent edits moved into a right-side Properties drawer.
- Sidebar navigation now defaults to smart faceted filters (`All`, `Today`, `This Week`, `Recent`, `Unfiled`, `Code`, `Config`, `Logs`, `Links`) instead of folder-tree-first navigation.

---

- [Plan-Aligned Rewrite Checklist](#plan-aligned-rewrite-checklist)
  - [Phase 0: Baseline \& Guardrails](#phase-0-baseline--guardrails)
  - [Phase 1: Extract localpaste\_core](#phase-1-extract-localpaste_core)
  - [Phase 2: Native App Skeleton (Current)](#phase-2-native-app-skeleton-current)
  - [Phase 3: Fast List + Collections](#phase-3-fast-list--collections)
  - [Phase 4: Editor + Autosave](#phase-4-editor--autosave)
  - [Phase 5: Search + Command Palette](#phase-5-search--command-palette)
  - [Phase 6: Polish + Intelligence](#phase-6-polish--intelligence)
  - [Language + Highlighting](#language--highlighting)
  - [Naming + Metadata](#naming--metadata)
  - [Folders](#folders)
  - [UX + Theme](#ux--theme)
  - [Intentional Deviations (per PLAN.md)](#intentional-deviations-per-planmd)
  - [Legacy Removal Status](#legacy-removal-status)
  - [Removal Gate](#removal-gate)

---

## Phase 0: Baseline & Guardrails

- [x] Headless perf baseline documented ([perf-baseline.md](perf-baseline.md))
- [x] Test data generator supports full clear + large datasets
- [x] Manual profiler panel (profile feature, no puffin_egui)
- [x] CLI timing flag for API request baselines

## Phase 1: Extract localpaste_core

- [x] Core extracted to `localpaste_core`
- [x] API/CLI builds use core without GUI deps
- [x] Default port updated + documented
- [x] API delete lock enforcement (blocked when paste open in GUI)

## Phase 2: Native App Skeleton (Current)

- [x] Native app launches (eframe)
- [x] Backend thread + command/event channel
- [x] List pastes (basic)
- [x] Select -> async load content
- [x] Missing paste handling (list refreshes; selection cleared)

## Phase 3: Fast List + Collections

- [x] Virtualized list (show_rows) for 10k items
- [x] Smart collections + secondary language filter (smart facets in sidebar + language filter in status bar) [Replace]
- [x] Keyboard navigation (up/down, enter)

## Phase 4: Editor + Autosave

- [x] Editable multiline editor
- [x] Read-only virtual preview mode behind `LOCALPASTE_VIRTUAL_PREVIEW`
- [x] Editable virtual rope editor is the default mode (`LOCALPASTE_VIRTUAL_EDITOR=0` keeps `TextEdit` fallback)
- [x] Dirty state tracking + save indicator
- [x] Autosave debounce (UI non-blocking) [Replace]
- [x] Manual save (Ctrl/Cmd+S)
- [x] New paste (Ctrl/Cmd+N)
- [x] Smart paste creation when unfocused (Ctrl/Cmd+V)
- [x] Delete selected (Ctrl/Cmd+Delete)
- [x] Export (file dialog + extension mapping)
- [x] Native GUI edit locks (open paste blocks API/CLI deletion)

### Virtual Editor Reliability Gates (Validated For Default Mode)

- [x] Clipboard reliability (`Ctrl/Cmd+C/X/V`) with external paste verification
- [x] `Ctrl/Cmd+V` non-regression: when app window is active but editor is unfocused, paste creates a new paste and does not mutate current editor content
- [x] Focus-gated virtual command routing: only `Copy` is selection-driven without focus; mutating/edit commands require focused virtual editor
- [x] Triple-click whole-line selection behavior (repeatable, non-intermittent)
- [x] Selection visuals: style-driven low-opacity fill from `ui.visuals().selection` (no custom multi-line left rail)
- [x] Drag-selection auto-scroll at viewport edges in virtual preview/editor (selection anchor preserved while scrolling)
- [x] Manual recheck (2026-02-12): drag auto-scroll upward/downward both pass; unfocused `Ctrl/Cmd+V` still creates a new paste without mutating current editor
- [~] Highlight recovery: keep current render visible while async refresh is pending (newline-burst scenario fixed in code path; perf gate recheck pending)
- [x] Stale staged-highlight renders are dropped before apply (no unnecessary `highlight_version` bumps)
- [x] Scope policy: multilingual/IME-specific UX and validation are explicitly out of scope for release gating (English-first workflow only)
- [x] Trace protocol documented and validated with:
  - `LOCALPASTE_EDITOR_INPUT_TRACE=1`
  - `LOCALPASTE_HIGHLIGHT_TRACE=1`
  - Input trace expectation: deterministic routing with accurate `copied/cut/pasted` outcomes.
  - Highlight trace expectation: deterministic staged highlight lifecycle with stale render drops.

Detailed perf and trace protocol lives in [gui-perf-protocol.md](gui-perf-protocol.md).

## Phase 5: Search + Command Palette

- [x] Debounced search (150ms)
- [x] Command palette (Ctrl+K)
- [x] Result ranking
- [~] Quick actions (delete/copy/copy-fenced done; pin deferred)

## Phase 6: Polish + Intelligence

- [ ] Duplicate detection
- [ ] LLM output heuristic
- [ ] Optional folder tree (if kept) [Keep]
- [ ] Drag-drop to folder (paste row -> folder row, keep active scope)
- [x] Copy as fenced code block
- [ ] Context menus

## Language + Highlighting

- [x] Auto-detect language on content (detect on create; re-detect on content update and when switching back to auto mode)
- [x] Manual language override + `language_is_manual`
- [x] Async syntect highlighting with staged apply and line-state reuse [Replace]
- [x] Large-paste fallback to plain text
- [x] Plain highlight threshold (aligned with perf budget)
- [x] Highlighting debounce while typing for performance

## Naming + Metadata

- [x] Auto-name generation on create (content-derived with random fallback)
- [x] Rename behavior (explicit editor metadata field) [Replace]
- [x] Tags edit + persistence

## Folders

- [~] Folder APIs remain available in core/server, but folder controls are removed from the rewrite GUI in favor of smart filters
- [x] Folder delete migrates pastes to unfiled (API/core behavior)

## UX + Theme

- [x] Native theme consistent with palette direction (dark + accent) [Replace]
- [x] Status feedback (status bar + toast notifications)
- [x] Shortcut hints in UI (top-bar help entry + F1 shortcut window)

---

## Intentional Deviations (per PLAN.md)

- [x] Metadata editing lives in a compact editor header + properties drawer instead of a large always-on form
- [x] Manual folders as primary nav -> Smart Collections + search
- [Replace] Export button as primary save -> autosave + subtle indicator
- [Replace] Highlight/layout path -> async syntect render + cache lifecycle keyed by editor revision and highlight epoch
- [Replace] Blocking DB calls in UI -> backend thread + channels
- [Drop] Any legacy-only UI quirks that fight the new model

---

## Legacy Removal Status

- [x] `legacy/` source files removed from tracked workspace content
- [x] `localpaste-gui-legacy` bin wiring removed
- [x] `gui-legacy` feature wiring removed from `crates/localpaste_gui/Cargo.toml`

## Removal Gate

Rewrite merge gate before release:

- Phase 3 list performance is met (virtualized, 10k OK)
- Phase 4 editor + autosave UX is complete
- Phase 5 search + command palette is complete
- Smart filters + language filter flows work end-to-end
- Large-paste handling + highlight strategy is stable
