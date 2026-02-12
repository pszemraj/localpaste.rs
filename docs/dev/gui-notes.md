# GUI Notes

## Sidebar

- Drag & drop behaviour reverted to the last stable version (pre-egui rewrite); no active work in progress.
- Paste rows use plain `selectable_label`, so hit targets now track the rendered text exactly. Any future styling changes should preserve that widget to keep click behaviour predictable.

## Highlight Profiling

- Rewrite perf telemetry uses `LOCALPASTE_EDITOR_PERF_LOG=1` for moving-average FPS and p95 frame timing.
- Detailed input/highlight traces use:
  - `LOCALPASTE_EDITOR_INPUT_TRACE=1`
  - `LOCALPASTE_HIGHLIGHT_TRACE=1`

## Virtualized Editor

- Detailed plan in [virtual-editor-plan.md](virtual-editor-plan.md).
- `LOCALPASTE_VIRTUAL_PREVIEW=1` keeps the read-only viewport renderer available for diagnostics.
- Editable virtual editor is now the default mode.
- `LOCALPASTE_VIRTUAL_EDITOR=1` explicitly forces editable virtual mode:
  - rope-backed text buffer
  - virtualized variable-height rendering (`show_viewport`)
  - operation-based undo/redo
  - command reducer for keyboard navigation/selection/edit operations
- `LOCALPASTE_VIRTUAL_EDITOR=0` forces the `TextEdit` fallback.
- Scope note: this cycle is English-first; multilingual/IME-specific UX design and validation are out of scope.

## Rewrite Highlighting

- Rewrite uses syntect directly with a plain-text fallback for large pastes (>=256KB).
- Buffers >=64KB use an async highlighter thread; the UI keeps the last render until the worker returns (avoids on/off flicker while typing).
- Highlight requests are debounced (150ms) so large edits don't clone/reparse every keystroke.
- Highlighting caches per-line syntect parse/highlight state to reuse unchanged lines after edits (both UI and worker).
- For large buffers, built-in egui double-click selection is disabled to avoid O(n^2) word boundary scans; the editor applies a local word-range selection instead.
- Language hint is derived from paste language metadata; missing metadata is shown as `(auto)` in the list/header.
- Virtual editor uses the same staged highlight flow; render cache keys include highlight epoch so stale galleys are not reused after highlight updates.

## Edit Locks

- When a paste is open in the GUI, it is locked against API/CLI deletion.
- Only the GUI instance editing the paste may delete it.
