# Plan-Aligned Rewrite Checklist (Legacy Reference)

This checklist tracks PLAN.md phases. The legacy GUI (`legacy/gui/mod.rs`) is reference-only.
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

- [Plan-Aligned Rewrite Checklist (Legacy Reference)](#plan-aligned-rewrite-checklist-legacy-reference)
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
  - [Legacy Freeze Policy (Agreed)](#legacy-freeze-policy-agreed)
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
- [ ] Smart collections sidebar (Recent, Pinned, By Language, etc.) [Replace]
- [x] Keyboard navigation (up/down, enter)

## Phase 4: Editor + Autosave

- [x] Editable multiline editor
- [x] Read-only virtual preview mode behind `LOCALPASTE_VIRTUAL_PREVIEW`
- [x] Editable virtual rope editor is the default mode (`LOCALPASTE_VIRTUAL_EDITOR=0` keeps `TextEdit` fallback)
- [x] Dirty state tracking + save indicator
- [x] Autosave debounce (UI non-blocking) [Replace]
- [ ] Manual save (Ctrl/Cmd+S)
- [x] New paste (Ctrl/Cmd+N)
- [x] Smart paste creation when unfocused (Ctrl/Cmd+V)
- [x] Delete selected (Ctrl/Cmd+Delete)
- [ ] Export (file dialog + extension mapping)
- [x] Native GUI edit locks (open paste blocks API/CLI deletion)

### Virtual Editor Reliability Gates (Validated For Default Mode)

- [x] Clipboard reliability (`Ctrl/Cmd+C/X/V`) with external paste verification
- [x] Focus-gated virtual command routing: only `Copy` is selection-driven without focus; mutating/edit commands require focused virtual editor
- [x] Triple-click whole-line selection behavior (repeatable, non-intermittent)
- [x] Selection visuals: style-driven low-opacity fill from `ui.visuals().selection` (no custom multi-line left rail)
- [x] Highlight recovery: keep current render visible while async refresh is pending
- [x] Stale staged-highlight renders are dropped before apply (no unnecessary `highlight_version` bumps)
- [x] Trace protocol documented and validated with:
  - `LOCALPASTE_EDITOR_INPUT_TRACE=1`
  - `LOCALPASTE_HIGHLIGHT_TRACE=1`
  - Input trace expectation: `virtual input frame` logs show deterministic `immediate_focus/deferred_focus/deferred_copy` routing with pre/post focus booleans and `copied/cut/pasted` flags aligned with executed commands.
  - Highlight trace expectation: `queue -> worker_done -> apply` (or `apply_now/apply_idle`) with stale staged renders dropped via `drop_stale_staged`; `perf-scroll-5k-lines` post-warm `worker_done` spikes should stay below 2000ms.

Recommended validation command (PowerShell):

```powershell
.\scratch\virtualizedgui-perf-run.ps1 `
  -Profile Release `
  -VirtualMode Editor `
  -PerfLog `
  -InputTrace `
  -HighlightTrace `
  -KeepDb `
  -Port 38973
```

## Phase 5: Search + Command Palette

- [ ] Debounced search (150ms)
- [ ] Command palette (Ctrl+K)
- [ ] Result ranking
- [ ] Quick actions (pin, delete, copy)

## Phase 6: Polish + Intelligence

- [ ] Duplicate detection
- [ ] LLM output heuristic
- [ ] Optional folder tree (if kept)
- [ ] Drag-drop to folder (if kept)
- [ ] Copy as fenced code block
- [ ] Context menus

## Language + Highlighting

- [~] Auto-detect language on content (core detects on create; rewrite does not re-run yet)
- [ ] Manual language override + `language_is_manual`
- [x] Async syntect highlighting with staged apply and line-state reuse [Replace]
- [x] Large-paste fallback to plain text
- [x] Plain highlight threshold (aligned with perf budget)
- [x] Highlighting debounce while typing for performance

## Naming + Metadata

- [~] Auto-name generation on create (random name today; planned content-derived)
- [ ] Rename behavior (when/how) [Replace]
- [ ] Tags edit + persistence

## Folders

- [ ] Folder list + counts
- [ ] Create/rename/delete folders
- [ ] Cycle-safe parenting dialog
- [ ] Move paste between folders
- [ ] Folder delete migrates pastes to unfiled

## UX + Theme

- [x] Native theme consistent with palette direction (dark + accent) [Replace]
- [~] Status feedback (status bar; no toasts yet)
- [~] Shortcut hints in UI (some implicit, no dedicated help surface)

---

## Intentional Deviations (per PLAN.md)

- [Replace] Form header (Name/Language/Folder) -> inferred + status bar
- [Replace] Manual folders as primary nav -> Smart Collections + search
- [Replace] Export button as primary save -> autosave + subtle indicator
- [Replace] Highlight/layout path -> async syntect render + cache lifecycle keyed by editor revision and highlight epoch
- [Replace] Blocking DB calls in UI -> backend thread + channels
- [Drop] Any legacy-only UI quirks that fight the new model

---

## Legacy Freeze Policy (Agreed)

- Legacy GUI is reference-only.
- No new features in legacy except critical data-loss/security fixes.
- All new behavior goes into native rewrite.

## Removal Gate

Legacy GUI can be removed once:

- Phase 3 list performance is met (virtualized, 10k OK)
- Phase 4 editor + autosave UX is complete
- Phase 5 search + command palette is complete
- Folder operations + delete migration work end-to-end
- Large-paste handling + highlight strategy is stable
