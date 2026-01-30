# Legacy → Native Parity Checklist

This is the authoritative checklist for porting behavior from the legacy GUI (`src/gui/mod.rs`) to the native rewrite (`crates/localpaste_native`).

Status key:
- [x] Done
- [~] Partial
- [ ] Not started

---

## Phase 0/1: Core + API
- [x] Core extracted to `localpaste_core`
- [x] API/CLI build still works (core is shared)
- [x] Default port updated + documented
- [x] API lock enforcement (delete blocked when paste open in GUI)

## Phase 2: Native App Skeleton (Current)
- [x] Native app launches (eframe)
- [x] Backend thread + command/event channel
- [x] List pastes (basic)
- [x] Select → fetch content (read-only)
- [~] Missing paste handling (list refreshes; selection cleared)

## Editor Behavior (Legacy parity)
- [ ] Editable multiline editor
- [ ] Dirty state tracking + save indicator
- [ ] Autosave debounce (UI non-blocking)
- [ ] Manual save (Ctrl/Cmd+S)
- [ ] New paste (Ctrl/Cmd+N)
- [ ] Delete selected (Ctrl/Cmd+Delete)
- [ ] Export (file dialog + extension mapping)
- [ ] Locking in native GUI (open paste must lock API deletes)

## Language & Highlighting
- [ ] Auto-detect language on content
- [ ] Manual language override + `language_is_manual`
- [ ] Highlighting via `egui_extras`
- [ ] Large-paste fallback to plain text
- [ ] Plain highlight threshold (aligned with legacy)

## Naming & Metadata
- [ ] Auto-name generation on create
- [ ] Rename behavior (when, how)
- [ ] Tags edit + persistence

## Folders
- [ ] Folder list + counts
- [ ] Create/rename/delete folders
- [ ] Cycle-safe parenting dialog
- [ ] Move paste between folders
- [ ] Folder delete migrates pastes to unfiled

## Search / Filter
- [ ] Filter/search in sidebar
- [ ] Keyboard focus (Ctrl/Cmd+F or Ctrl/Cmd+K)
- [ ] Filter counts + folder-aware counts

## UX / Theme
- [ ] Legacy palette parity
- [ ] Status toasts for actions/errors
- [ ] Shortcut hints in UI (as needed)

## Performance & Diagnostics
- [ ] Non-blocking I/O in `App::update`
- [ ] Virtualized list (10k target)
- [ ] Headless perf baseline updates
- [ ] Debug/perf overlay (manual profiler panel)

---

## Legacy Freeze Policy (Agreed)
- Legacy GUI is now **reference-only**.
- No new features in legacy except critical data-loss/security fixes.
- All new behavior goes into native rewrite.

## Removal Gate
Legacy GUI can be removed once the following are done:
- Editor behavior + autosave parity
- Folder CRUD + migration parity
- Search/filter parity
- Export + keyboard shortcuts parity
- Large-paste handling parity
- Non-blocking I/O + list virtualization
