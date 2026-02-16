# GUI Notes

Use this document for rewrite GUI behavior notes and env flags.
For detection/canonicalization/highlight pipeline semantics, use [docs/language-detection.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/language-detection.md).
For perf validation steps/gates, use [gui-perf-protocol.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md).

## Runtime Flags

- `LOCALPASTE_VIRTUAL_PREVIEW=1`: force read-only virtual preview mode.
- `LOCALPASTE_VIRTUAL_PREVIEW=0` (or empty): does not force preview and does not disable virtual editor by itself.
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
- When no explicit language filter is selected, the status bar label mirrors the selected paste language when known; it falls back to `Any` only when language is unknown.
- Sidebar list refresh and sidebar search run on metadata projections (`name/tags/language/folder`) and do not deserialize full paste content.
- Large buffers (`>= 256KB`) intentionally use plain-text rendering.
- Highlight updates are debounced (150ms) and staged so existing render stays visible during async refresh.
- Language display behavior is explicit: auto + unset -> `auto`; manual + unset -> `plain`.
- Metadata editing is intentionally compact in the editor header row; expanded metadata edits live in the Properties drawer.
- Folder create/edit/move controls are intentionally removed from the rewrite GUI; organization is smart-filter + search based.

## Language/Highlight QA (Magika + Fallback)

Use this checklist when touching detection/highlight/filter code.

1. Start GUI with default features (Magika enabled): `cargo run -p localpaste_gui --bin localpaste-gui`.
2. Create new pastes with representative snippets and confirm detected language chip (auto mode):
   - Rust: `fn main() { println!("hi"); }` -> `rust`
   - Python: `import os\nprint(os.getcwd())` -> `python`
   - Shell: `#!/bin/bash\necho hi` -> `shell`
   - JSON: `{\"key\":\"value\"}` -> `json`
3. Open Properties drawer, set language to `Plain text`, save, and verify chip reads `plain` (not `auto`).
4. With that same paste still manual plain, edit content into obvious Rust and verify language remains `plain`.
5. Switch language back to `Auto`, save, and verify content re-detects to `rust`.
6. Validate alias interoperability in UI filtering:
   - Set active language filter to `cs`; verify both `csharp` and `cs` pastes remain visible.
   - Set active language filter to `shell`; verify `bash`/`sh` labeled content matches.
7. Validate syntax resolver behavior:
   - `cs`, `shell`, `cpp`, `powershell` should highlight (non-plain grammar).
   - Unknown label and `text`/`txt` should render plain text.
   - High-priority fallback mappings when native grammar is unavailable:
     - `typescript` -> JavaScript grammar
     - `toml` -> Java Properties grammar (then YAML fallback)
     - `swift` -> Rust/Go grammar
     - `powershell` -> bash grammar
   - Languages currently kept as metadata/filter labels with plain rendering (no fallback grammar mapping):
     - `zig`, `scss`, `kotlin`, `elixir`, `dart`
8. Validate large-buffer guardrail:
   - Paste content >= 256KB and verify display is plain regardless of language metadata.
9. Re-run shortcut sanity checks after language UI edits:
   - `Ctrl/Cmd+S`, `Ctrl/Cmd+N`, `Ctrl/Cmd+Delete`, `Ctrl/Cmd+F`, `Ctrl/Cmd+K`.

## Edit Locks

Detailed lock semantics are canonical in [locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md).
GUI-specific behavior remains:

- Opening a paste in GUI acquires a paste edit lock for the app instance owner.
- API `PUT`/`DELETE` against that paste return `423 Locked` while the lock is held.
