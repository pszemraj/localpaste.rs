# GUI Notes

Use this document for rewrite GUI behavior notes and env flags.
Detection/normalization/highlight semantics are defined in [docs/language-detection.md](docs/language-detection.md) and should be treated as canonical.
For perf validation steps/gates, use [gui-perf-protocol.md](docs/dev/gui-perf-protocol.md).

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
- Virtual-editor highlight debounce/staging policy is defined in [docs/language-detection.md](docs/language-detection.md#virtual-editor-async-highlight-flow).
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
7. Validate syntax resolver behavior against the canonical matrix in [docs/language-detection.md](docs/language-detection.md#gui-highlight-resolution):
   - alias labels should resolve to non-plain grammars where expected,
   - unsupported labels should remain metadata-visible while rendering plain text.
8. Validate large-buffer guardrail:
   - Paste content >= 256KB and verify display is plain regardless of language metadata.
9. Re-run shortcut sanity checks after language UI edits:
   - `Ctrl/Cmd+S`, `Ctrl/Cmd+N`, `Ctrl/Cmd+Delete`, `Ctrl/Cmd+F`, `Ctrl/Cmd+K`.

## Manual GUI Human-Step Checklist (Comprehensive)

Use this when a change touches GUI interaction/state logic and you want an end-to-end manual pass.

### Preflight Commands

1. Build release binaries:
   - `cargo build -p localpaste_server --bin localpaste --release`
   - `cargo build -p localpaste_gui --bin localpaste-gui --release`
2. Run seeded virtual-editor GUI harness:
   - `.\scratch\virtualizedgui-perf-run.ps1 -Profile Release -VirtualMode Editor -PerfLog -InputTrace -HighlightTrace -SeedHighlightMatrix -SeedHighlightMatrixAutoDetect -KeepDb -Port 38973`
3. Expected script output before GUI launch:
   - seeded paste table printed
   - `Seed verification passed.`

### Script Status / Flags

- `scratch/virtualizedgui-perf-run.ps1` parameters are current:
  - `DbPath`, `Port`, `KeepDb`, `NoGui`, `Build`, `NoMagika`, `SeedHighlightMatrix`, `SeedHighlightMatrixAutoDetect`, `Profile`, `VirtualMode`, `PerfLog`, `InputTrace`, `HighlightTrace`
- `-SeedHighlightMatrixAutoDetect` is meaningful only when `-SeedHighlightMatrix` is also set.
- `-PerfLog` sets `LOCALPASTE_EDITOR_PERF_LOG=1`; backend perf logging is separate (`LOCALPASTE_BACKEND_PERF_LOG=1`).

### Manual Checklist

1. Launch sanity:
   - GUI opens without panic/crash, status bar shows API endpoint and expected mode (`virtual editor`).
2. Initial dataset sanity:
   - Sidebar includes seeded pastes such as `perf-medium-python`, `perf-100kb-python`, `perf-300kb-rust`, `perf-scroll-5k-lines`.
3. Focus behavior:
   - Click editor, type a character, caret remains visible and blinking.
4. Core shortcuts:
   - `Ctrl/Cmd+N`: creates/selects a new paste.
   - `Ctrl/Cmd+S`: save transitions status from dirty -> saved.
   - `Ctrl/Cmd+Delete`: deletes selected paste and list updates.
   - `Ctrl/Cmd+F`: focuses sidebar search input.
   - `Ctrl/Cmd+K`: opens command palette.
5. Command palette actions:
   - Open selected paste from palette.
   - Delete from palette and confirm list removal.
   - Copy raw/copy fenced commands complete and close/open behavior is correct.
6. Search and filters:
   - Sidebar query narrows results and clearing query restores list.
   - Smart collections (`All`, `Today`, `This Week`, `Recent`, `Unfiled`, `Code`, `Config`, `Logs`, `Links`) re-scope results.
   - Bottom language filter stacks with active collection (not replacing it).
7. Metadata/properties:
   - Open Properties drawer, edit name/tags/language, save, and confirm list projection updates.
8. Clipboard/editing baseline:
   - `Ctrl/Cmd+C`, `Ctrl/Cmd+X`, `Ctrl/Cmd+V`, `Ctrl/Cmd+Z`, `Ctrl/Cmd+Y` behave correctly in virtual editor mode.
9. Virtual editor selection:
   - Double-click selects word.
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

Detailed lock semantics are documented in [locking-model.md](docs/dev/locking-model.md).
GUI-specific behavior remains:

- Opening a paste in GUI acquires a paste edit lock for the app instance owner.
- API `PUT`/`DELETE` against that paste return `423 Locked` while the lock is held.
