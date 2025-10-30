# GUI Notes

## Sidebar

- Drag & drop behaviour reverted to the last stable version (pre-egui rewrite); no active work in progress.
- Paste rows use plain `selectable_label`, so hit targets now track the rendered text exactly. Any future styling changes should preserve that widget to keep click behaviour predictable.

## Highlight Profiling

- Set `LOCALPASTE_PROFILE_HIGHLIGHT=1` before launching the GUI to log highlight and text layout timings via `tracing::debug!`.
- Logged events:
  - `highlight_job`: duration, cache hit/miss, language token, paste id, character count.
  - `text_edit_layout`: the time spent laying out the multiline editor per frame and the current character count.
- These hooks are meant to guide the upcoming virtualized editor work; remove or downgrade them once we have a replacement metrics story.
\n## Virtualized Editor\n- Detailed plan in docs/virtual-editor-plan.md; current focus is chunked highlighting + per-line layout cache before tackling full viewport editing.
