# GUI Notes

Use this document for rewrite GUI behavior notes and env flags.
Detection/normalization/highlight semantics are defined in
[docs/language-detection.md](../language-detection.md)
and should be treated as canonical.
For perf validation steps/gates, use
[docs/dev/gui-perf-protocol.md](gui-perf-protocol.md).

## Runtime Flags

- `LOCALPASTE_VIRTUAL_PREVIEW=1`: force read-only virtual preview mode.
- `LOCALPASTE_VIRTUAL_PREVIEW=0` (or empty): does not force preview and does not disable virtual editor by itself.
- `LOCALPASTE_VIRTUAL_EDITOR=1`: force editable virtual mode (default behavior).
- `LOCALPASTE_VIRTUAL_EDITOR=0`: force `TextEdit` fallback kill-switch.
- `LOCALPASTE_EDITOR_PERF_LOG=1`: periodic local frame snapshots (`avg/p50/p95/p99/worst`) plus list/search and redo-cache counters.
- `LOCALPASTE_BACKEND_PERF_LOG=1`: local backend list/search cache hit/miss and latency logs.
- `LOCALPASTE_EDITOR_INPUT_TRACE=1`: virtual input routing trace.
- `LOCALPASTE_HIGHLIGHT_TRACE=1`: highlight request/apply/drop lifecycle trace.
- `LOCALPASTE_LOG_FILE=<path>`: append GUI tracing logs to a file (useful on Windows release builds where no console is shown).
- Boolean flags accept `1`, `true`, `yes`, `on` and `0`, `false`, `no`, `off` (case-insensitive, whitespace trimmed).
- Unrecognized flag values emit a warning and are treated as unset/false (shared parser behavior across core/server/gui env flags).

## Stable Behavior Notes

- Paste rows use `selectable_label`; keep this if adjusting row styling to preserve reliable click targets.
- Collections scope controls are rendered as smart filters in the sidebar (`All`, `Today`, `This Week`, `Recent`, `Unfiled`, `Code`, `Config`, `Logs`, `Links`) with compact chips and overflow under `...`.
- Language filtering is rendered in the sidebar under smart filters and always includes an explicit `All languages` clear option.
- Language filtering stacks with the active smart collection instead of replacing it.
- Sidebar list refresh and sidebar search run on metadata projections (`name/tags/language/folder`) and do not deserialize full paste content.
- Command palette is action-first (`Commands` section first; `Pastes` section is secondary search/open context).
- Large buffers (`>= 256KB`) intentionally use plain-text rendering.
- Virtual-editor highlight debounce/staging policy is defined in
  [docs/language-detection.md#virtual-editor-async-highlight-flow](../language-detection.md#virtual-editor-async-highlight-flow).
- Language display behavior is explicit: auto + unset -> `auto`; manual + unset -> `plain`.
- Rename/title edits commit on `Enter` and on title-field blur.
- Metadata editing is intentionally compact in the editor header row; expanded metadata edits live in the Properties drawer.
- Folder create/edit/move controls are intentionally removed from the rewrite GUI; organization is smart-filter + search based.
- Global sidebar navigation via `Up`/`Down` is bare-arrow only; modified arrows (`Ctrl`/`Alt`/`Shift`/`Cmd`) stay in editor-selection semantics.
- Virtual wrapped-row navigation preserves wrap-boundary intent across vertical movement (boundary affinity handling).
- Over-wide glyph wrapping (emoji/CJK in very narrow viewports) consumes at least one glyph per row to avoid blank visual rows.
- Virtual preview triple-click selects the full logical line, including terminal long lines even when rendering is capped.
- Virtual editor double-click word selection is clamped to the render cap so hidden post-cap content is never selected/mutated implicitly.

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
7. Validate syntax resolver behavior against the canonical matrix in
   [docs/language-detection.md#gui-highlight-resolution](../language-detection.md#gui-highlight-resolution):
   - alias labels should resolve to non-plain grammars where expected,
   - unsupported labels should remain metadata-visible while rendering plain text.
8. Validate large-buffer guardrail:
   - Paste content >= 256KB and verify display is plain regardless of language metadata.
9. Re-run shortcut sanity checks after language UI edits:
   - `Ctrl/Cmd+S`, `Ctrl/Cmd+N`, `Ctrl/Cmd+Delete`, `Ctrl/Cmd+F`, `Ctrl/Cmd+Shift+P`.

## Manual GUI Human-Step Checklist (Comprehensive)

Use this when a change touches GUI interaction/state logic and you want an end-to-end manual pass.

### Preflight Commands

- Build/run commands: [docs/dev/devlog.md](devlog.md).
- Perf-oriented dataset + trace runbook: [docs/dev/gui-perf-protocol.md#runbook](gui-perf-protocol.md#runbook).
- Use virtual editor mode (`LOCALPASTE_VIRTUAL_EDITOR=1`) when executing this checklist.

### Manual Checklist

1. Launch sanity:
   - GUI opens without panic/crash and status bar shows API endpoint.
2. Initial dataset sanity:
   - Sidebar includes seeded pastes such as `perf-medium-python`, `perf-100kb-python`, `perf-300kb-rust`, `perf-scroll-5k-lines`.
3. Focus behavior:
   - Click editor, type a character, caret remains visible and blinking.
4. Core shortcuts:
   - `Ctrl/Cmd+N`: creates/selects a new paste.
   - `Ctrl/Cmd+S`: save transitions status from dirty -> saved.
   - `Ctrl/Cmd+Delete`: deletes selected paste and list updates.
   - `Ctrl/Cmd+F`: focuses sidebar search input.
   - `Ctrl/Cmd+Shift+P`: opens command palette.
   - `Ctrl/Cmd+K`: toggles command palette (legacy alias).
   - `Ctrl/Cmd+I`: toggles Properties drawer.
5. Command palette actions:
   - Open selected paste from palette.
   - Delete from palette and confirm list removal.
   - Copy raw/copy fenced commands complete and close/open behavior is correct.
6. Search and filters:
   - Sidebar query narrows results and clearing query restores list.
   - Smart collections (`All`, `Today`, `This Week`, `Recent`, `Unfiled`, `Code`, `Config`, `Logs`, `Links`) re-scope results.
   - Sidebar language filter (`All languages` + detected languages) stacks with active collection (not replacing it).
7. Metadata/properties:
   - Open Properties drawer, edit name/tags/language, save, and confirm list projection updates.
   - Rename in the editor header applies on `Enter` and on blur (without requiring Apply click).
8. Clipboard/editing baseline:
   - `Ctrl/Cmd+C`, `Ctrl/Cmd+X`, `Ctrl/Cmd+V`, `Ctrl/Cmd+Z`, `Ctrl/Cmd+Y` behave correctly in virtual editor mode.
   - Modified arrow movement/selection (`Ctrl`/`Alt`/`Shift`/`Cmd` + arrows) affects editor selection/caret movement and does not switch sidebar filters.
9. Virtual editor selection:
   - Double-click selects word.
   - On a render-capped long line, double-click does not extend selection beyond the visible cap.
   - Triple-click selects line.
   - Drag selection across lines keeps expected range and autoscroll direction.
10. Wrap-boundary regression: down-move boundary intent:
    - Paste content `abcd\nab\n`.
    - Make editor narrow enough to wrap at ~4 columns.
    - Put caret at end of `abcd` and press `Down`.
    - Expected: caret lands at end of short row (`ab`), not column 0.
11. Wrap-boundary regression: repeated up from exact boundary:
    - Paste content `wxyz\nabcdefgh\n`.
    - Keep wrap at ~4 columns.
    - Place caret at end of `abcdefgh`, press `Up` twice.
    - Expected: second `Up` continues movement to previous physical line end (`wxyz`), not stuck on internal boundary.
12. Wide-glyph wrapping regression:
    - Paste `🦀` (or `你好`) and make viewport very narrow (`wrap_cols` effectively 1).
    - Expected: no blank first visual row; glyph remains visible; caret/selection maps to glyph correctly.
13. Highlight matrix auto-detect sanity:
    - Open several `lang-*` seeded pastes and verify language chip/highlighting is plausible for each.
14. Manual plain override:
    - Set language to `Plain text` in Properties, save, confirm chip shows `plain`.
    - Edit into obvious code; expected: remains plain until switched back to auto.
15. Large buffer fallback:
    - Open `perf-300kb-rust`; expected: plain rendering by design (`>=256KB`) with smooth scrolling.
16. Mid-size perf sanity:
    - Open `perf-scroll-5k-lines`, scroll rapidly, type near middle, no major hitching.
17. Window reflow:
    - Resize window repeatedly; expected: no persistent plain-text gap artifacts and caret remains aligned.
18. Lock behavior sanity:
    - While GUI is open on a paste, verify external API mutation attempts against same paste are lock-gated (423 behavior per lock model).
19. Trace sanity (if enabled):
    - Input trace logs show deterministic virtual input routing.
    - Highlight trace logs show queue/worker/apply flow with stale drops when applicable.
    - Perf logs emit frame percentiles (`avg/p50/p95/p99/worst`) periodically.
20. Persistence check:
    - Close GUI and relaunch with same `DB_PATH` (kept by `-KeepDb`), verify seeded/edited content persists.

## Edit Locks

Detailed lock semantics are documented in [locking-model.md](locking-model.md).
GUI-specific behavior remains:

- Opening a paste in GUI acquires a paste edit lock for the app instance owner.
- API `PUT`/`DELETE` against that paste return `423 Locked` while the lock is held.
