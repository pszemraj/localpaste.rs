# GUI Notes

Use this document for rewrite GUI behavior notes and env flags.
For rollout status, use [virtual-editor-plan.md](virtual-editor-plan.md).
For perf validation steps/gates, use [gui-perf-protocol.md](gui-perf-protocol.md).

## Runtime Flags

- `LOCALPASTE_VIRTUAL_PREVIEW=1`: force read-only virtual preview mode.
- `LOCALPASTE_VIRTUAL_EDITOR=1`: force editable virtual mode (default behavior).
- `LOCALPASTE_VIRTUAL_EDITOR=0`: force `TextEdit` fallback kill-switch.
- `LOCALPASTE_EDITOR_PERF_LOG=1`: periodic local frame snapshots (`avg/p50/p95/p99/worst`) plus list/search and redo-cache counters.
- `LOCALPASTE_BACKEND_PERF_LOG=1`: local backend list/search cache hit/miss and latency logs.
- `LOCALPASTE_EDITOR_INPUT_TRACE=1`: virtual input routing trace.
- `LOCALPASTE_HIGHLIGHT_TRACE=1`: highlight request/apply/drop lifecycle trace.
- Boolean flags accept `1`, `true`, `yes`, `on` and `0`, `false`, `no`, `off` (case-insensitive, whitespace trimmed).
- Unrecognized flag values emit a warning and are treated as unset/false (shared parser behavior across core/server/gui env flags).

## Stable Behavior Notes

- Paste rows use `selectable_label`; keep this if adjusting row styling to preserve reliable click targets.
- Collections scope controls are rendered as smart filters in the sidebar (`All`, `Today`, `This Week`, `Recent`, `Unfiled`, `Code`, `Config`, `Logs`, `Links`) with compact chips and overflow under `...`.
- Language filtering is a secondary stackable filter in the bottom status bar (`Language: Any|...`) and applies on top of the active smart collection.
- Sidebar list refresh and sidebar search run on metadata projections (`name/tags/language/folder`) and do not deserialize full paste content.
- Large buffers (`>= 256KB`) intentionally use plain-text rendering.
- Highlight updates are debounced (150ms) and staged so existing render stays visible during async refresh.
- Language display can show `(auto)` when metadata language is unset.
- Metadata editing is intentionally compact in the editor header row; expanded metadata edits live in the Properties drawer.
- Folder create/edit/move controls are intentionally removed from the rewrite GUI; organization is smart-filter + search based.

## Edit Locks

- Opening a paste in GUI acquires a lock against API/CLI deletion.
- Only the GUI instance holding the lock can delete that paste.
