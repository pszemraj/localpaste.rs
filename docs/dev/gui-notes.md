# GUI Notes

GUI runtime flags and interaction contracts.
Detection/normalization/highlight behavior: [language-detection.md](../language-detection.md).
Perf validation steps and gates: [gui-perf-protocol.md](gui-perf-protocol.md).

## Runtime Flags

- `LOCALPASTE_EDITOR_PERF_LOG=1`: periodic local frame snapshots (`avg/p50/p95/p99/worst`) plus list/search and redo-cache counters.
- `LOCALPASTE_BACKEND_PERF_LOG=1`: local backend list/search cache hit/miss and latency logs.
- `LOCALPASTE_EDITOR_INPUT_TRACE=1`: virtual input routing trace.
- `LOCALPASTE_HIGHLIGHT_TRACE=1`: highlight request/apply/drop lifecycle trace.
- `LOCALPASTE_LOG_FILE=<path>`: append GUI tracing logs to a file (useful on Windows release builds where no console is shown).
- `LOCALPASTE_LINUX_DESKTOP_ENTRY=force|off`: Linux-only desktop-entry setup override. `force` writes the managed user entry; `off`/`skip`/`disabled` disables desktop-entry setup for isolated probe or test launches.
- `LOCALPASTE_NAV_PROBE_LOG=<path>`: enables per-frame NDJSON navigation probe logging.
- `LOCALPASTE_NAV_PROBE_SCENARIO=<id>`: labels probe frames for runner assertions.
- `LOCALPASTE_NAV_PROBE_SEED_TEXT=<text>`, `LOCALPASTE_NAV_PROBE_SEED_NAME=<name>`, `LOCALPASTE_NAV_PROBE_SEED_CURSOR=<char|line:col>`: seed the disposable in-memory probe paste and initial caret.
- `LOCALPASTE_NAV_PROBE_FOCUS_EDITOR=1`: requests initial virtual-editor focus until the active native window acquires it; later focus changes follow normal app behavior.
- `LOCALPASTE_NAV_PROBE_PYTHON=<path>`: runner/assertion Python executable override.
- Shared boolean flags above accept `1`, `true`, `yes`, `on` and `0`, `false`, `no`, `off` (case-insensitive, whitespace trimmed).
- Unrecognized shared boolean values emit a warning and are treated as unset/false.

## Keyboard And Navigation Contract

Shortcut contract:

- `Ctrl/Cmd+N`: create/select new paste.
- `Ctrl/Cmd+S`: save content + metadata.
- `Ctrl/Cmd+Delete`: delete selected paste when text input does not own focus. `Delete` means forward Delete; on a MacBook keyboard use `Fn+Cmd+Delete`.
- `Ctrl/Cmd+F`: focus sidebar search.
- `Ctrl/Cmd+K`: toggle the commands-only palette (also `Ctrl/Cmd+Shift+P`).
- `Ctrl/Cmd+Shift+K`: toggle the separate paste picker.
- `Ctrl/Cmd+I`: toggle Properties drawer.
- `F1`: toggle shortcut help; search descriptions or key combinations (for example `picker` or `Cmd+Shift+K`).
- `Ctrl/Cmd+A/C/X/Z/Y`: standard virtual-editor select-all/copy/cut/undo/redo when editor owns focus.
- `Ctrl/Cmd+Shift+Z`: redo editor edit when editor owns focus.
- Windows `Shift+Delete`, including `Ctrl+Shift+Delete`, cuts the editor selection; with no selection it leaves the buffer unchanged. `Ctrl+Delete` deletes the next word. The pinned [egui-winit 0.33.3 clipboard mapping](https://github.com/emilk/egui/blob/0.33.3/crates/egui-winit/src/lib.rs#L1008-L1012) consumes the shifted Delete press as a Cut event; Linux retains forward word deletion for `Ctrl+Shift+Delete`.
- `Ctrl/Cmd+V`: insert when editor is focused; create new paste from clipboard when editor is not focused.
- `Ctrl/Cmd+Shift+V`: explicit "force paste as new" fallback.

Native paste routing recognizes egui-winit's Paste-only events as well as key-plus-payload input. Under CPU load, the pinned backend can deliver the payload after discarding all shortcut modifiers; `Ctrl/Cmd+Shift+V` then inserts into the current editor. The [inspection and native paste migration (#35)](https://github.com/pszemraj/localpaste.rs/issues/35) must preserve the press chord to resolve that case.

Navigation/selection contract:

- Global sidebar navigation via `Up`/`Down` is bare-arrow only; modified arrows (`Ctrl`/`Alt`/`Shift`/`Cmd`) stay in editor-selection semantics.
- `Tab` indents selected nonblank lines; without a selection it inserts four spaces. `Shift+Tab` removes one leading tab or up to four spaces from each affected line. This uses the buffer's line boundaries, including CR and Unicode separators, and preserves original line endings. Even a partial single-line selection indents that entire line. Directional selections survive indentation and undo/redo; each indentation is one undo step.
- Window deactivation releases editor keyboard ownership after applying earlier text and paste events. When multiple native focus events arrive in one frame, the final event determines whether to blur. Uncommitted IME preedit is cancelled on deactivation or discovery focus transfer, restoring displaced text and selection so typing can resume. Committing composition creates one undoable replacement of the original selection. `Ctrl/Cmd+C` still copies that selection when no other text input owns the keyboard; `Ctrl/Cmd+V` creates a new paste. Click the editor to resume editing.
- Shift-click and Shift-drag extend the editor selection from its existing anchor using modifiers captured at mouse press, even if Shift is released before the mouse. Floating-window clicks, including window edges, leave the underlying caret and selection untouched.
- Virtual wrapped-row navigation preserves wrap-boundary intent across vertical movement (boundary affinity handling).
- CRLF is one navigation boundary: Left/Right and shifted selection cross both characters without placing the caret between them. Edits and undo/redo keep the caret on a visible position and preserve unedited line endings.
- Over-wide glyph wrapping (emoji/CJK in very narrow viewports) consumes at least one glyph per row to avoid blank visual rows.
- Virtual editor double-click word selection is clamped to the render cap so hidden post-cap content is never selected/mutated implicitly.

## Navigation Probe

The navigation probe writes per-frame NDJSON for native keyboard focus and caret checks. The OS-specific runner scripts set the probe environment variables listed in [Runtime Flags](#runtime-flags); set them directly only for local debugging or manual Wayland checks.

Probe contract and tooling:

- Scenario contract: [nav_contract.json](nav_contract.json)
- Assertion checker: [../../tools/nav_probe_assert.py](../../tools/nav_probe_assert.py)
- Linux X11 runner: [../../tools/nav_probe_run_linux_x11.sh](../../tools/nav_probe_run_linux_x11.sh)
- macOS runner: [../../tools/nav_probe_run_macos.sh](../../tools/nav_probe_run_macos.sh)
- Windows runner: [../../tools/nav_probe_run_windows.ps1](../../tools/nav_probe_run_windows.ps1)

Probe snapshots include actual viewport/caret bounds, visible-caret status, Find focus, and scroll offset. Native contracts assert visibility as well as cursor indices.

All automated runners launch a disposable probe DB under `target/`, seed the editor, wait for a probe frame showing virtual-editor keyboard focus, inject native key chords, and write NDJSON evidence. They terminate the child GUI process after the evidence frame by default so disposable probe runs do not fail on normal window-close shutdown races. Use `--help` / `Get-Help` on the runner for timing and filtering options.

Linux automation is X11-only and requires `xdotool`; Wayland must be checked manually with the same probe environment variables because compositor policy restricts synthetic input.

```bash
tools/nav_probe_run_linux_x11.sh --build --assert --only ctrl_home_from_middle
tools/nav_probe_run_linux_x11.sh --assert --ctrl-only --summary
tools/nav_probe_run_linux_x11.sh --assert
```

The Linux runner sets `LOCALPASTE_LINUX_DESKTOP_ENTRY=off` for probe launches so contract runs do not touch user desktop-integration paths.
It also defaults probe launches to `LIBGL_ALWAYS_SOFTWARE=1` and `WGPU_BACKEND=gl` to avoid host GPU/EGL startup noise; set either variable before running the script to override that default.

macOS automation uses `tools/nav_probe_run_macos.sh` and requires Accessibility permission for the terminal running the script, because native key injection goes through the Swift/CoreGraphics helper built from `tools/nav_probe_macos_driver.swift`:

```bash
tools/nav_probe_run_macos.sh --build --assert --only cmd_up_from_middle --summary
tools/nav_probe_run_macos.sh --build --assert --summary
tools/nav_probe_run_macos.sh --list
```

Windows automation uses `tools/nav_probe_run_windows.ps1` from Windows PowerShell 5.1 or PowerShell 7 and defaults to the low-level `SendInput` path for navigation chords. Use `-UseSendKeys` only as a fallback when debugging the driver itself:

```powershell
tools\nav_probe_run_windows.ps1 -Build -Assert -Only ctrl_home_from_middle
tools\nav_probe_run_windows.ps1 -Build -Assert -CtrlOnly -Summary
tools\nav_probe_run_windows.ps1 -Build -Assert -CtrlOnly -Summary -RepeatCount 3
tools\nav_probe_run_windows.ps1 -Assert
```

Use `-RepeatCount` for flake hunting. When assertions are enabled, repeated runs log each repetition under a unique scenario label and assert it immediately against the base contract so a later passing run cannot hide an earlier failed chord.
Windows runs also write a manifest next to the NDJSON log by default (`*.manifest.json`) with scenario selection, repeat labels, completed runs, input driver, shutdown mode, log path, and final assertion status. Use `-Manifest <path>` to override it.
Runs with `-Assert` self-verify the final manifest before exiting. New manifests record `-Only` selections, so a `-CtrlOnly` manifest without `-Only` must cover every Windows ctrl scenario in the current contract for every repeat.
Re-verify a completed Windows artifact bundle with `python tools/nav_probe_assert.py --manifest target/<run>.manifest.json --summary`; pass explicit `LOG SPEC` positional paths before `--manifest` if the manifest was copied from another checkout.
For the full Windows ctrl-navigation proof, re-verify the final artifact with `python tools/nav_probe_assert.py --manifest target/<run>.manifest.json --require-full-windows-ctrl --min-repeat-count 3 --summary`.

Use `python tools/nav_probe_assert.py --check-spec docs/dev/nav_contract.json --windows-runner tools/nav_probe_run_windows.ps1 --self-test` to lint scenario ids, driver chord syntax, macOS key-code entries, Windows runner key support, and manifest-completeness checks without launching the GUI.

## Stable Behavior Notes

- Paste rows use `selectable_label`; keep this if adjusting row styling to preserve reliable click targets.
- Collections scope controls are rendered as smart filters in the sidebar (`All`, `Today`, `This Week`, `Recent`, `Unfiled`, `Documents`, `Code`, `Config`, `Logs`, `Links`) with compact chips and overflow under `...`.
- Language filtering is rendered in the sidebar under smart filters and always includes an explicit `All languages` clear option.
- Language filtering stacks with the active smart collection instead of replacing it.
- Sidebar list refresh reads metadata projections; scoped sidebar and paste-picker searches return metadata summaries.
- Empty filtered results retain unsaved content, metadata, and pending saves with the edit lock until saving completes. Selecting the active paste cancels a queued switch, and repeated load replies cannot replace an initialized editor draft.
- The Documents smart filter groups Markdown, prose notes, reStructuredText, and LaTeX. Document classification, Markdown scopes, and text-export extensions follow [Language Detection And Highlighting](../language-detection.md).
- The app selects its dark theme before installing custom fonts and spacing, including on systems using a light theme. Editor geometry uses the resolved font even if a later style change removes the named Editor text style. Startup, first creation, and populated restart have regression coverage with light-system input and without test-only style registration.
- The command palette searches actions only, including Export, Duplicate, Copy, Copy Link, Find, Properties, History, and Diff. The paste picker searches paste rows and retains Open, Copy, Copy Fenced, and Delete actions.
- Command palette, paste picker, and shortcut help are mutually exclusive: opening one closes the other two so its query owns keyboard input. Escape or the toggle shortcut restores the input that opened discovery, including across switches between these surfaces. Input batched around these transitions stays in order: earlier text belongs to the previous input, and later text belongs to the new destination. Accepting a palette command or picker result with Enter processes the preceding query before routing subsequent text. Commands that open another workflow retain their intended focus destination.
- Opening History or Diff closes discovery surfaces and prevents command-palette and paste-picker opens. Version windows allow Save for the current draft; New and Delete report why they are blocked. Discovery overlays block background New, Paste as New, Save, Delete, sidebar-search focus, and Properties shortcuts, including delayed clipboard replies; actions chosen within the palette remain available. Escape closes either version window. A refused sidebar selection preserves the picker-opened document and its pending input.
- Command-palette keyboard navigation and query changes reveal the selected command within the scroll area.
- Sidebar and picker each retain their own session query and field scope: All fields (default), Title, Metadata, or Body. Metadata searches the existing title/tag/language/derived-term projection. Title and Metadata avoid loading bodies; every scope searches the full store before applying the result limit. HTTP and CLI search are unchanged.
- Debounced sidebar and picker searches schedule their own repaint deadline; typing a query needs no further input or focus change to dispatch it. Pending or failed requests do not create a repaint loop.
- Picker scope changes clear old results immediately; changing only sidebar scope retains the open document and reading position even if no rows match. Responses and backend cache keys carry the scope and collection so delayed results cannot leak between contexts. Collection rules apply in the backend before the search result limit, including when a matching collection row is older than the first 512 unfiltered results.
- Opening the paste picker from its shortcut or the command palette refreshes its retained query and scope and selects the old query for replacement; responses discarded while it was closed cannot leave it stuck with empty results. Reopening or changing query/scope resets selection and scrolls to the first result once rows arrive; subsequent manual scrolling is preserved.
- Picker Delete restores the originating input after deleting a different paste and preserves the editor draft through the delete reply. Deleting the open paste keeps the picker as the keyboard owner, including for typing and paste into its query, and blocks dismissal, discovery toggles, and unrelated selection changes until the delete reply and replacement paste load settle. A selection already queued before deletion is preferred over the adjacent fallback. A save, delete, or replacement-load failure releases the fence while preserving the picker; after a successful replacement load, Escape returns to the originating input.
- Arrow navigation from a hidden selection starts at the first visible row.
- The paste picker shows `Searching...` while a scoped request is in flight. Search failures remain visible in the picker with a Retry button. Requests resume only when Retry is clicked or query/scope changes; a successful response clears the error. Closing it discards displayed results and resets selection while retaining its query and scope. Language labels follow the sidebar's large-buffer plain-rendering rule.
- Sidebar scope changes retain existing rows until replacement results arrive. A failed search keeps those rows and shows `Retry search`; retry or a query/filter change resumes requests. Stale failures cannot replace the current query's status.
- Picker All fields and Body results show a compact excerpt around the first literal body match. Title and Metadata results use metadata alone and have no body excerpts.
- Opening a picker result focuses its editor after loading. Text and paste received while the previous draft saves or the chosen body loads wait for that accepted selection; failed or cancelled opens discard their queued input. Native deactivation preserves earlier queued edits and leaves the editor blurred. Results outside current sidebar results stay selected through background refreshes until sidebar navigation, the search query, or collection/language filters change. Copying a picker result leaves the current editor selection and draft intact.
- Repeated picker copies use the latest request's snapshot and format. Older loaded, missing, or failed replies cannot consume a newer copy action, including another copy of the same paste.
- A completed editor, toolbar, text-field, or selectable-label copy supersedes an older pending picker copy. A copy with no selected text leaves the pending request intact.
- Editor toolbar `Find` searches the currently open paste body, selects the active match in the virtual editor, and scrolls it into view. Switching pastes with Find open selects the retained query's first match. Opening a paste from a sidebar or picker All fields or Body search primes this in-paste find bar when that search query appears in the paste body. Picker opens retain their originating query through loading and save-before-switch, including when reopening the active draft. Metadata-only picker hits preserve the existing Find query.
- Growing the Find query refines the current selected match from its start. Find keeps query focus after clicking Prev, Next, or Case and on `Enter`/`Shift+Enter` and advances to the next/previous match. `Escape` from its query or `Close` returns focus to the editor while preserving the matched selection and the query for reopening. Buttons and document jumps center the caret independently of editor focus.
- Typing and paste reveal the caret with minimal scrolling; manual scrolling stays where you leave it until another edit or navigation action. Virtual rows use zero vertical item spacing so hit testing and scrolling share the rendered row height.
- An editor-owned drag continues selection and autoscroll outside the viewport or window; foreground overlays retain their pointer ownership. Dragging the scrollbar preserves the editor selection.
- Loading another paste resets the previous viewport to the first line before applying any search-match reveal, including when the previous paste was scrolled to its end.
- Virtual-editor paste follows the post-paste cursor: when a multiline paste extends past the current viewport, the editor scrolls so the inserted tail/caret is visible instead of leaving the paste off-screen.
- In a Markdown-labelled paste, pasting one HTTP(S) URL over selected text creates `[selected text](URL)` in one undoable edit. Inline code is retained; label line breaks use character references so blank lines cannot split the link. URL recognition uses the current language choice, including `md`; other languages, empty selections, and non-URL clipboard text retain ordinary paste behavior.
- App-level shortcut dispatch, command-palette hints, and keyboard shortcut help share the runtime shortcut registry. Dispatch preserves native event order, including repeated chords. The shortcut help intentionally excludes command-palette query terms such as `diff` and `history`; those remain command-palette discoverability, not keyboard shortcuts.
- Help lists app bindings and selected editor combinations. Basic navigation and standard select/copy/cut/undo/redo instructions are omitted.
- F1 help uses a stable, scrollable layout with aligned native-platform key labels. Search omits empty sections, reports no matches, and returns to the first result when edited. Clear keeps search focus; Escape, F1, and Close return focus to the input that opened help. Opening a different discovery surface transfers focus to that surface instead.
- F1 help temporarily hides History or Diff while keeping its version workflow intact. Closing help returns to that dialog; Escape dismisses only help.
- Virtual-editor highlight debounce/staging policy is defined in
  [language-detection.md#virtual-editor-async-highlight-flow](../language-detection.md#virtual-editor-async-highlight-flow).
- Language display behavior is explicit: auto + unset -> `auto`; manual + unset -> `plain`.
- Rename/title edits commit on `Enter` and on title-field blur.
- Metadata editing is intentionally compact in the editor header row; expanded metadata edits live in the Properties drawer.
- Properties drawer is non-modal; opening it does not disable virtual-editor typing, caret movement, or editor shortcuts.
- Folder create/edit/move controls are absent from the GUI; organize with smart filters and search.

## Diff And History Workflows

- Editor toolbar exposes `Diff` and `History` for the selected paste.
- Command palette exposes `Open diff modal` and `Open history modal` when a paste is selected.
- Opening Diff focuses its candidate query once; later focus changes remain under user control. Floating-window query focus waits for a visible render pass and retains its one-shot request through egui's sizing pass, so accessibility focus always refers to a node in the published tree.
- Diff is detached from main editor state:
  - left side uses the active snapshot (`active_snapshot`) so unsaved edits are included,
  - loading a comparison target does not change current selection or paste locks.
- History is detached and read-only:
  - `Current working copy` is index `0`,
  - stored snapshots are normally older-only entries,
  - reset restores the selected snapshot and archives the outgoing head as a recoverable snapshot.
  - dirty save-and-reset saves local edits before reset so the just-saved outgoing head is recoverable.
- History, Diff, and reset-confirm windows fence background mutations:
  - create/delete/paste-as-new and other destructive workflow shortcuts are blocked while a version window is open,
  - autosave and explicit save still persist already-dirty content/metadata while a version window is open,
  - selection changes and automatic reselection are blocked while a version window is open,
  - the selected paste stays pinned during a queued hard reset,
  - the selected paste is temporarily read-only until reset success/error arrives.
- Diff preview generation runs on the backend worker against frozen left/right text snapshots; the UI only renders cached results.
- Reset and snapshot loading clear their in-flight UI state only for matching version-load/reset failures so unrelated backend errors cannot tear down the modal context.

## Language/Highlight QA (Magika + Fallback)

Run this checklist when touching detection/highlight/filter code.

1. Start GUI with default features (Magika enabled): `cargo run -p localpaste_gui --bin localpaste-gui`.
2. Create new pastes with representative snippets and confirm detected language chip (auto mode):
   - Rust: `fn main() { println!("hi"); }` -> `rust`
   - Python: `import os\nprint(os.getcwd())` -> `python`
   - Shell: `#!/bin/bash\necho hi` -> `shell`
   - JSON: `{\"key\":\"value\"}` -> `json`
3. Open Properties drawer, set language to `Plain text`, save, and verify chip reads `plain` (not `auto`).
4. With that same paste still manual plain, edit content into obvious Rust and verify language remains `plain`.
5. Switch language back to `Auto`, save, and verify the language chip shows unresolved auto state.
6. Make a content edit and verify auto-detection resolves and locks to `rust`.
7. Validate alias interoperability in UI filtering:
   - Set active language filter to `cs`; verify both `csharp` and `cs` pastes remain visible.
   - Set active language filter to `shell`; verify `bash`/`sh` labeled content matches.
8. Validate syntax resolver behavior against the matrix in
   [language-detection.md#gui-highlight-resolution](../language-detection.md#gui-highlight-resolution):
   - alias labels should resolve to non-plain grammars where expected,
   - unsupported labels should remain metadata-visible while rendering plain text.
   - In Markdown, put a Rust fence after a `- ` list marker, add an indented body, then an unindented `ordinary prose` line. The final line returns to prose colors; repeat with an ordered marker, tildes, and a following sibling list item.
9. Validate large-buffer guardrail:
   - Use content over the [plain-rendering threshold](../language-detection.md#virtual-editor-async-highlight-flow) and verify display is plain regardless of language metadata.
10. Re-run keyboard/navigation sanity checks listed in
    [Keyboard And Navigation Contract](#keyboard-and-navigation-contract)
    after language UI edits.

## Manual GUI Human-Step Checklist (Comprehensive)

Run this end-to-end pass when a change touches GUI interaction or state logic.

### Preflight Commands

- Build/run commands: [devlog.md](devlog.md).
- Perf-oriented dataset + trace runbook: [gui-perf-protocol.md#runbook](gui-perf-protocol.md#runbook).
- Virtual editor mode is the default editable path; no separate kill-switch flag is supported.

### Manual Checklist

1. Launch sanity:
   - GUI opens without panic/crash and status bar shows API endpoint.
2. Initial dataset sanity:
   - Confirm the named cases from the [perf runbook](gui-perf-protocol.md#runbook) appear in All, including `perf-scroll-5k-lines`.
3. Focus behavior:
   - Click editor, type a character, caret remains visible and blinking.
   - Focus stays in editor during in-editor interaction.
   - Focus blurs only when clicking outside editor or when the app window loses focus.
4. Core shortcuts and navigation:
   - Verify all contracts in
     [Keyboard And Navigation Contract](#keyboard-and-navigation-contract).
   - Confirm save transitions dirty -> saved after `Ctrl/Cmd+S`.
5. Commands and paste discovery:
   - `Ctrl/Cmd+K` lists commands; a paste-body query does not produce paste rows.
   - `Ctrl/Cmd+Shift+K` opens the paste picker. With an empty query, open a different paste and confirm its body starts at the first line. With a Body query near the end of a long paste, confirm the result shows a matching excerpt; open it and confirm Find selects and reveals that passage.
   - Copy and Copy Fenced work from picker results without changing the active editor or its unsaved draft; deleting a disposable result removes its row.
   - With a dirty draft, open each of the picker, palette, and help, then press `Ctrl/Cmd+Shift+V`. Clipboard text belongs to that surface; no new paste appears behind it. Choosing the palette's explicit Paste as New action still creates a paste.
   - Open history and diff modals from palette queries (`history`, `diff`) when a paste is selected.
   - `F1` help shows native platform key names and finds shortcuts by description and by key combination, including `Cmd+Shift+K` on macOS or `Ctrl+Shift+K` on Windows/Linux. Try an unmatched query and confirm the no-match message appears without resizing the window; Clear restores all shortcuts. Open help from the editor, then close with `Esc`, `F1`, or Close and confirm typing resumes at the same selection. Repeat from sidebar search and confirm typing resumes in its query.
   - Open History, press `F1`, then `Esc`: help closes and History returns at the same snapshot. Repeat from Diff and confirm its comparison is retained.
6. Search and filters:
   - Sidebar query narrows results and clearing query restores list.
   - Check Title, Metadata, and Body with field-specific fixtures; sidebar and picker retain independent queries and scopes.
   - Opening a paste from a sidebar body-text search selects and scrolls to the first matching substring in the editor.
   - Editor toolbar `Find` locates substrings within the selected paste; `Next`/`Prev` wrap through all matches and the `Case` toggle narrows matching.
   - Leave Find on match two, switch to another paste containing the query, and confirm match one is visibly selected; Next selects match two.
   - Close Find with `Escape` or `Close`, then type; the matched selection is replaced in the editor without another click.
   - Smart collections re-scope results; verify prose with embedded fences and standalone technical fences against [filter semantics](../language-detection.md#filter-and-search-semantics).
   - Sidebar language filter (`All languages` + detected languages) stacks with active collection (not replacing it).
7. Metadata/properties:
   - Open Properties drawer, edit name/tags/language, save, and confirm list projection updates.
   - Rename in the editor header applies on `Enter` and on blur (without requiring Apply click).
8. Clipboard/editing baseline:
   - `Ctrl/Cmd+C`, `Ctrl/Cmd+X`, `Ctrl/Cmd+V`, `Ctrl/Cmd+Z`, `Ctrl/Cmd+Y` behave correctly in virtual editor mode.
   - Paste a 20-30 line block near the bottom of the visible editor; expected: the inserted tail/caret scrolls into view.
   - Set the language to Markdown, select a phrase, and paste `https://example.com/docs`: expected `[phrase](https://example.com/docs)`, with the caret after the link. Undo restores the phrase and its selection; Redo restores the link. Save and restart to confirm persistence. Repeat with a non-URL or a plain-text paste and confirm ordinary replacement; a URL pasted without selection remains literal.
   - `Ctrl/Cmd+V` outside editor focus creates a new paste from clipboard.
   - `Ctrl/Cmd+Shift+V` requests a new paste when native modifiers survive; the known backend modifier-loss case is tracked in [#35](https://github.com/pszemraj/localpaste.rs/issues/35). The command palette's Paste as New action remains available.
   - Start Paste as New, open help with `F1`, close it with `Escape`, then paste fresh text with `Ctrl/Cmd+V`. The fresh paste reaches the editor; an older canceled clipboard reply must not overwrite or prepend it.
   - Modified arrow movement/selection (`Ctrl`/`Alt`/`Shift`/`Cmd` + arrows) affects editor selection/caret movement and does not switch sidebar filters.
9. Virtual editor selection:
   - Double-click selects word.
   - On a render-capped long line, double-click does not extend selection beyond the visible cap.
   - Triple-click selects line.
   - Drag selection across lines keeps expected range and autoscroll direction.
   - Load `a\rb\n# c\n` (escapes denote actual line separators), move to `b`, and Delete it. Check the visible caret after deletion, undo, and redo; typing `x` after redo produces `ax\r\n# c\n`. Right/Left and Shift+Right/Left cross the CRLF pair without an invisible caret or changing the body.
   - Load Rust `// comment\u2028let b = 2;\u2029let c = 3;` with actual Unicode separators. Both `let` rows retain Rust colors; `Ctrl/Cmd+Home`, `End`, `Shift+Home`, Copy selects only `// comment`, excluding its separator.
   - Replace several Rust rows separated by Unicode line breaks, keeping a middle row unchanged and making the last replaced row a comment. Its colors must update immediately, including after Undo/Redo.
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
13. Run [Language/Highlight QA](#languagehighlight-qa-magika--fallback), using its representative snippets and smooth scrolling in `perf-300kb-rust`.
14. Mid-size perf sanity:
    - Open `perf-scroll-5k-lines`, scroll rapidly, type near middle, no major hitching.
15. Window reflow:
    - Resize window repeatedly; expected: no persistent plain-text gap artifacts and caret remains aligned.
16. Lock behavior sanity:
    - While GUI is open on a paste, verify external API mutation attempts against same paste are lock-gated (423 behavior per lock model).
17. Version workflow sanity:
    - Open `Diff`, select another paste, and verify current unsaved edits appear on the left side.
    - Open `History`, navigate with `Older/Newer`, duplicate a historical snapshot, and verify a new paste is created.
    - With Documents active, open a Code paste from the picker, then History or Diff. Collapse or resize the window to expose a sidebar row, click it, then press Escape. The refused click must leave the Code paste open.
    - Trigger reset-to-version and verify current paste updates to the selected snapshot.
18. Trace sanity (if enabled):
    - Input trace logs show deterministic virtual input routing.
    - Highlight trace logs show queue/worker/apply flow with stale drops when applicable.
    - Perf logs emit frame percentiles (`avg/p50/p95/p99/worst`) periodically.
19. Persistence check:
    - Close GUI and relaunch with the same `DB_PATH`; verify seeded/edited content persists.

## Edit Locks

See [GUI lock ownership](locking-model.md#gui-ownership) and the [API error contract](locking-model.md#error-surface-contract).
