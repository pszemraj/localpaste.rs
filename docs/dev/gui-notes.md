# GUI Notes

## Sidebar

- Drag & drop behaviour reverted to the last stable version (pre-egui rewrite); no active work in progress.
- Paste rows use plain `selectable_label`, so hit targets now track the rendered text exactly. Any future styling changes should preserve that widget to keep click behaviour predictable.

## Highlight Profiling

- **Legacy only**: set `LOCALPASTE_PROFILE_HIGHLIGHT=1` before launching the legacy GUI to log highlight and text layout timings via `tracing::debug!`.
- Logged events:
  - `highlight_job`: duration, cache hit/miss, language token, paste id, character count.
  - `text_edit_layout`: the time spent laying out the multiline editor per frame and the current character count.
- These hooks are retained for legacy diagnostics. Rewrite perf telemetry now uses `LOCALPASTE_EDITOR_PERF_LOG=1`.

## Virtualized Editor

- Detailed plan in [virtual-editor-plan.md](virtual-editor-plan.md).
- `LOCALPASTE_VIRTUAL_PREVIEW=1` keeps the read-only viewport renderer available for diagnostics.
- `LOCALPASTE_VIRTUAL_EDITOR=1` enables the editable virtual editor:
  - rope-backed text buffer
  - virtualized variable-height rendering (`show_viewport`)
  - operation-based undo/redo
  - IME composition event handling
  - command reducer for keyboard navigation/selection/edit operations
- Default editable mode is still `TextEdit` while parity/perf validation completes.

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
