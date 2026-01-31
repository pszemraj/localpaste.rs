# GUI Notes

## Sidebar

- Drag & drop behaviour reverted to the last stable version (pre-egui rewrite); no active work in progress.
- Paste rows use plain `selectable_label`, so hit targets now track the rendered text exactly. Any future styling changes should preserve that widget to keep click behaviour predictable.

## Highlight Profiling

- **Legacy only**: set `LOCALPASTE_PROFILE_HIGHLIGHT=1` before launching the legacy GUI to log highlight and text layout timings via `tracing::debug!`.
- Logged events:
  - `highlight_job`: duration, cache hit/miss, language token, paste id, character count.
  - `text_edit_layout`: the time spent laying out the multiline editor per frame and the current character count.
- These hooks are meant to guide the upcoming virtualized editor work; remove or downgrade them once we have a replacement metrics story.

## Virtualized Editor

- Detailed plan in docs/virtual-editor-plan.md; current focus is chunked highlighting + per-line layout cache before tackling full viewport editing.
- Legacy highlight recompute is debounced (75ms) and reuses prior galley; profile flag logs run durations.

## Rewrite Highlighting

- Rewrite uses `egui_extras::syntax_highlighting` with a plain-text fallback for large pastes (>=256KB).
- Highlighting is debounced while typing (150ms) to avoid stutter; the editor renders as plain text until the user pauses.
- Language hint is derived from paste language metadata; missing metadata is shown as `(auto)` in the list/header.

## Edit Locks

- When a paste is open in the GUI, it is locked against API/CLI deletion.
- Only the GUI instance editing the paste may delete it.
