# Virtualized Editor Plan

This document tracks rollout of the rewrite editor from full-buffer `TextEdit` rendering to a rope-backed, viewport-virtualized editor.

## Current Modes

- `VirtualEditor` is the default editable path.
- `LOCALPASTE_VIRTUAL_PREVIEW=1` enables the read-only viewport renderer.
- `LOCALPASTE_VIRTUAL_EDITOR=0` forces `TextEdit` fallback for diagnostics/kill-switch use.
- `LOCALPASTE_VIRTUAL_EDITOR=1` explicitly forces editable virtual mode and wins over preview.

## Status Snapshot (2026-02-12)

### Implemented (2026-02-11)

- `EditorBuffer` now keeps rope-backed state (with a `String` mirror for `TextEdit` compatibility).
- `app/virtual_editor/` now contains:
  - `buffer.rs` (`RopeBuffer`, edit deltas, char/line conversions)
  - `state.rs` (cursor/selection/focus/IME state)
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
  - style-driven selection overlay (`ui.visuals().selection`) with no custom multi-line rail
  - triple-click full-line selection in large-buffer editable paths
  - hardened clipboard routing (`Ctrl/Cmd+C/X`) with deferred apply after focus settles
- Reliability validation protocol was updated and run with trace expectations:
  - `LOCALPASTE_EDITOR_INPUT_TRACE=1`
  - `LOCALPASTE_HIGHLIGHT_TRACE=1`
- `crates/localpaste_gui/src/app.rs` monolith was refactored into focused modules:
  - `app/mod.rs`
  - `app/style.rs`
  - `app/highlight_flow.rs`
  - `app/virtual_ops.rs`
  - `app/state_ops.rs`
  - `app/ui/*`
  - `app/tests/mod.rs`
- Post-refactor constraint achieved: all `crates/localpaste_gui/src/**/*.rs` files are now `< 1000` LoC.
- Default mode switched to editable virtual editor (`EditorMode::VirtualEditor`) with explicit `TextEdit` opt-out via `LOCALPASTE_VIRTUAL_EDITOR=0`.

## Post-Default Follow-ups

- Continue periodic manual parity passes in GUI for:
  - typing and edits at start/middle/end of large buffers
  - selection parity (mouse drag, shift-selection, word navigation)
  - navigation parity (Home/End, PageUp/PageDown, Ctrl/Cmd+arrows)
  - undo/redo parity (`Ctrl/Cmd+Z/Y`, `Shift+Ctrl/Cmd+Z`)
- Validate and re-check IME behavior on Windows end-to-end (`Enabled -> Preedit -> Commit -> Disabled`).
- Add or explicitly defer drag auto-scroll behavior when selecting beyond viewport edges.
- Keep performance gate stable in release mode for the 5k-line scenario:
  - average FPS `>= 45`
  - p95 frame time `<= 25 ms`
  - no visible hitching during rapid scroll + mid-document typing

## Rollout Plan

1. Keep running `docs/dev/gui-perf-protocol.md` for perf + interaction regression checks.
2. Keep `TextEdit` as a temporary kill-switch (`LOCALPASTE_VIRTUAL_EDITOR=0`) for at least one cycle.
3. Remove fallback only after parity checklist and perf gate remain stable across normal usage.
