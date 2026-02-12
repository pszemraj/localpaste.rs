# Virtualized Editor Plan

This document tracks virtual-editor rollout status and implementation notes.

Canonical docs for validation status:

- release/perf gate procedure: [gui-perf-protocol.md](gui-perf-protocol.md)
- overall rewrite/merge gate status: [parity-checklist.md](parity-checklist.md)

## Current Modes

- `VirtualEditor` is the default editable path.
- `TextEdit` remains available as a fallback/diagnostic path.
- Runtime flag definitions and env matrix live in [gui-notes.md](gui-notes.md) (canonical).

## Scope Note

- Product scope for this cycle is English-only editor UX.
- Multilingual input design/validation (IME, i18n, locale-specific text workflows) is explicitly out of scope.
- If multilingual input works incidentally, treat it as best-effort and non-blocking.

## Status Snapshot (2026-02-12)

### Implemented (2026-02-11)

- `EditorBuffer` now keeps rope-backed state (with a `String` mirror for `TextEdit` compatibility).
- `app/virtual_editor/` now contains:
  - `buffer.rs` (`RopeBuffer`, edit deltas, char/line conversions)
  - `state.rs` (cursor/selection/focus editor state)
  - `history.rs` (bounded undo/redo with coalescing)
  - `layout.rs` (soft-wrap metrics + prefix-height index for viewport mapping)
  - `input.rs` (egui event -> virtual editor command reducer)
- Editable virtual editor rendering uses `ScrollArea::show_viewport` and variable-height line layout.
- Async syntect highlighting is shared with staged apply, and layout cache keys include highlight epoch/versioning.
- Frame metrics logging is available via `LOCALPASTE_EDITOR_PERF_LOG=1`.

### Implemented (2026-02-12)

- Reliability stabilization landed for virtual editor usage:
  - highlight cache alignment for line insert/delete in both UI-thread and worker-thread paths
  - exact snapshot matching for async highlight renders to avoid stale render application
  - stale staged renders now drop before apply when revision/text length no longer match active snapshot
  - style-driven selection overlay (`ui.visuals().selection`) with no custom multi-line rail
  - strict focus-gated input routing: only `Copy` can run selection-driven without focus; mutating/edit commands require focused virtual editor
  - virtual preview/editor click semantics now use one custom streak detector (no mixed egui double/triple overrides)
  - triple-click full-line selection in large-buffer editable paths
  - hardened clipboard routing (`Ctrl/Cmd+C/X`) with deferred apply after focus settles
  - drag-selection auto-scroll enabled at viewport edges for virtual preview and virtual editor (distance-scaled speed)
- Reliability validation protocol was updated and run with trace expectations:
  - `LOCALPASTE_EDITOR_INPUT_TRACE=1`
  - `LOCALPASTE_HIGHLIGHT_TRACE=1`
  - manual revalidation pass (2026-02-12):
    - drag auto-scroll upward works while selection extends
    - drag auto-scroll downward works while selection extends
    - unfocused `Ctrl/Cmd+V` still creates a new paste and does not mutate current editor text
- `crates/localpaste_gui/src/app.rs` monolith was refactored into focused modules:
  - `app/mod.rs`
  - `app/style.rs`
  - `app/highlight_flow.rs`
  - `app/virtual_ops.rs`
  - `app/state_ops.rs`
  - `app/ui/*`
  - `app/tests/mod.rs`
- Post-refactor constraint achieved for runtime modules: non-test files under
  `crates/localpaste_gui/src/app/**/*.rs` remain `< 1000` LoC.
- Default mode switched to editable virtual editor (`EditorMode::VirtualEditor`) with explicit `TextEdit` opt-out via `LOCALPASTE_VIRTUAL_EDITOR=0`.

## Deferred Follow-ups (Post-Merge)

These items are intentionally deferred to follow-up PRs so the rewrite work can keep moving.
They are not blockers for keeping `VirtualEditor` as the default path.
Current pre-merge focus remains the broader parity work tracked in [parity-checklist.md](parity-checklist.md).

- Re-verify previous highlight latency gap fix on newline bursts in `perf-scroll-5k-lines`:
  - previous symptom (now addressed in code path): repeated `Enter` in the middle could cause multi-second plain fallback before highlight returns
  - current action: keep re-running perf protocol and trace checks to verify the fix holds under newline-burst edits across releases
- Continue periodic manual parity passes in GUI for:
  - typing and edits at start/middle/end of large buffers
  - selection parity (mouse drag, shift-selection, word navigation)
  - navigation parity (Home/End, PageUp/PageDown, Ctrl/Cmd+arrows)
  - undo/redo parity (`Ctrl/Cmd+Z/Y`, `Shift+Ctrl/Cmd+Z`)
- Keep performance behavior within the release gate tracked in [gui-perf-protocol.md](gui-perf-protocol.md).
- Preserve unfocused paste behavior as a hard non-regression:
  - when LocalPaste window is active but editor is unfocused, `Ctrl/Cmd+V` must create a new paste and must not mutate the current editor

## Follow-up Sequencing

1. Finish remaining high-priority items in [parity-checklist.md](parity-checklist.md).
2. Keep running [gui-perf-protocol.md](gui-perf-protocol.md) for perf and interaction regression checks.
3. Keep `TextEdit` kill-switch (`LOCALPASTE_VIRTUAL_EDITOR=0`) available until parity/perf are stable.
4. Remove fallback after parity gate confidence is established.
