# GUI Notes

GUI runtime flags and interaction contracts. Detection/normalization/highlight behavior: [language-detection.md](../language-detection.md). Perf validation steps and gates: [gui-perf-protocol.md](gui-perf-protocol.md).

## Runtime Flags

- `LOCALPASTE_EDITOR_PERF_LOG=1`: periodic local frame snapshots (`avg/p50/p95/p99/worst`) plus list/search and redo-cache counters.
- `LOCALPASTE_BACKEND_PERF_LOG=1`: local backend list/search cache hit/miss and latency logs.
- `LOCALPASTE_EDITOR_INPUT_TRACE=1`: virtual input routing trace.
- `LOCALPASTE_HIGHLIGHT_TRACE=1`: highlight request/apply/drop lifecycle trace.
- `LOCALPASTE_LOG_FILE=<path>`: append GUI tracing logs to a file (useful on Windows release builds where no console is shown).
- `LOCALPASTE_LINUX_DESKTOP_ENTRY=force|off`: Linux-only desktop-entry setup override. `force` writes the managed user entry; `off` (also `0`, `false`, `no`, `skip`, `disable`, `disabled`) disables desktop-entry setup for isolated probe or test launches.
- `LOCALPASTE_NAV_PROBE_LOG=<path>`: enables per-frame NDJSON navigation probe logging.
- `LOCALPASTE_NAV_PROBE_SCENARIO=<id>`: labels probe frames for runner assertions.
- `LOCALPASTE_NAV_PROBE_SEED_TEXT=<text>`, `LOCALPASTE_NAV_PROBE_SEED_NAME=<name>`, `LOCALPASTE_NAV_PROBE_SEED_CURSOR=<char|line:col>`: seed the disposable in-memory probe paste and initial caret.
- `LOCALPASTE_NAV_PROBE_FOCUS_EDITOR=1`: requests initial virtual-editor focus until the active native window acquires it; later focus changes follow normal app behavior. Only `1` and `true` enable it.
- `LOCALPASTE_NAV_PROBE_PYTHON=<path>`: runner/assertion Python executable override.
- The perf and trace flags (`LOCALPASTE_EDITOR_PERF_LOG`, `LOCALPASTE_BACKEND_PERF_LOG`, `LOCALPASTE_EDITOR_INPUT_TRACE`, `LOCALPASTE_HIGHLIGHT_TRACE`) accept `1`, `true`, `yes`, `on` and `0`, `false`, `no`, `off` (case-insensitive, whitespace trimmed). An unrecognized value logs a warning and counts as false.

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
- `Ctrl/Cmd+V`: pastes into the focused editor or text field. With nothing focused, the clipboard becomes a new paste. Switching away from the app releases editor focus, so the first paste after switching back creates a new paste.
- `Ctrl/Cmd+Shift+V`: pastes into the open paste at its last caret or selection, whatever has focus, then focuses the editor. With no paste open, the text is appended to the top sidebar paste once it loads; with no pastes, it becomes a new one. The palette, picker, and help keep the text for their own query field. The palette's Paste as New action is the explicit way to create a paste.

egui-winit delivers these chords as a Paste event with no key event, so routing reads the chord from the frame's modifier state. If the modifiers are released before the frame runs, under CPU load or with automated key injection, `Ctrl/Cmd+Shift+V` is indistinguishable from `Ctrl/Cmd+V` and follows that rule. [Issue #35](https://github.com/pszemraj/localpaste.rs/issues/35) tracks preserving the pressed chord.

Focus ownership: the virtual editor keeps the keyboard until the window loses native focus, a text field or discovery surface takes focus, or a click lands outside it (editor toolbar actions that preserve focus excepted). Native deactivation first applies the text and paste events that arrived earlier in the same frame, then releases editor focus while keeping the selection. When a frame carries several focus events the last one decides, so a deactivate-and-reactivate pair keeps editing. While unfocused, `Ctrl/Cmd+C` still copies the editor selection if no other text input has focus, `Ctrl/Cmd+V` creates a new paste, and a click on the editor resumes editing. Uncommitted IME composition is cancelled on deactivation and when a discovery surface takes focus, restoring the displaced text and selection; a later commit is one undoable replacement of the original selection.

Navigation/selection contract:

- `Up`/`Down` move the sidebar selection only as bare arrows while neither the editor nor a text input owns the keyboard and no overlay is open; if the selected paste is not in the visible list, the first press selects the first visible row. Arrows with `Ctrl`/`Alt`/`Shift`/`Cmd` stay with editor-selection semantics.
- `Tab` indents each selected line that has non-whitespace content by four spaces, and inserts four spaces at a single caret. `Shift+Tab` removes one leading tab or up to four spaces from each affected line. A partial single-line selection indents the whole line, directional selections survive indentation and undo/redo, and each indentation is one undo step.
- Shift-click and Shift-drag extend the editor selection from its existing anchor using the modifiers captured at mouse press, even if Shift is released before the mouse. Clicks on floating windows, including their edges, leave the underlying caret and selection untouched.
- Virtual wrapped-row navigation preserves wrap-boundary intent across vertical movement (boundary affinity handling).
- CRLF is one navigation boundary: Left/Right and shifted selection cross both characters without placing the caret between them. Edits and undo/redo keep the caret on a visible position and preserve unedited line endings.
- Over-wide glyph wrapping (emoji/CJK in very narrow viewports) consumes at least one glyph per row to avoid blank visual rows.

## Navigation Probe

The navigation probe writes per-frame NDJSON for native keyboard focus and caret checks: actual viewport and caret bounds, visible-caret status, Find focus, and scroll offset, so contracts assert visibility as well as cursor indices. The OS-specific runner scripts set the probe environment variables listed in [Runtime Flags](#runtime-flags); set them directly only for local debugging or manual Wayland checks.

- Scenario contract: [nav_contract.json](nav_contract.json)
- Assertion checker: [../../tools/nav_probe_assert.py](../../tools/nav_probe_assert.py)
- Linux X11 runner: [../../tools/nav_probe_run_linux_x11.sh](../../tools/nav_probe_run_linux_x11.sh)
- macOS runner: [../../tools/nav_probe_run_macos.sh](../../tools/nav_probe_run_macos.sh)
- Windows runner: [../../tools/nav_probe_run_windows.ps1](../../tools/nav_probe_run_windows.ps1)

Each runner launches a disposable probe DB under `target/`, seeds the editor, waits for a probe frame showing virtual-editor keyboard focus, injects native key chords, and writes NDJSON evidence. The GUI process is terminated after the evidence frame by default (Windows `-GracefulShutdown` opts out), so normal window-close shutdown races cannot fail a probe run. Run a runner with `--help` (`Get-Help tools\nav_probe_run_windows.ps1` on Windows) for scenario selection, timing, and repeat options; `--list` (`-List` on Windows) prints the scenarios that would run. Windows runs also write a `*.manifest.json` beside the log, which `-Assert` verifies; `python tools/nav_probe_assert.py --help` lists the options for re-verifying a saved bundle.

Linux automation is X11-only and requires `xdotool`; Wayland must be checked manually with the same probe environment variables because compositor policy restricts synthetic input. The runner sets `LOCALPASTE_LINUX_DESKTOP_ENTRY=off` so contract runs do not touch user desktop-integration paths, and defaults to `LIBGL_ALWAYS_SOFTWARE=1` and `WGPU_BACKEND=gl` to avoid host GPU/EGL startup noise; set either variable beforehand to override.

```bash
tools/nav_probe_run_linux_x11.sh --build --assert --only ctrl_home_from_middle
tools/nav_probe_run_linux_x11.sh --assert --summary
```

macOS automation requires Accessibility permission for the terminal running the script, because native key injection goes through the Swift/CoreGraphics helper built from `tools/nav_probe_macos_driver.swift`.

```bash
tools/nav_probe_run_macos.sh --build --assert --only cmd_up_from_middle --summary
tools/nav_probe_run_macos.sh --build --assert --summary
```

macOS `paste_*` scenarios use persisted fixtures and the `LocalPasteReview` test
bundle, with a fresh temporary `DB_PATH`, unused port, and explicit `LP_SERVER`.
The helper types and pastes through native events, reads bodies and metadata
through the embedded API, and checks them again after restarting the GUI. App
switch scenarios use `Cmd+Tab` and copy synthetic text from a new Safari window;
Safari must already be running. Only that window is closed. Every clipboard item
and representation is privately backed up and restored byte for byte, unless
outside clipboard activity has superseded the test copy.

Use `--key-delay-ms 80` for a normal held chord or `--key-delay-ms 0` for rapid
release. `--capture` pauses each paste result for a Computer screenshot: the
runner writes `LOG.with_suffix('.capture.json')` with the scenario and isolated
endpoint, and resumes when the controller creates `LOG.with_suffix('.continue')`.
No human input is needed. Completed paste results are recorded alongside native
frames in NDJSON, including full synthetic API readback and restart equality.
An unmet native precondition remains a failed assertion; it is never silently
replaced by an in-memory test.

Windows automation runs from Windows PowerShell 5.1 or PowerShell 7 and sends navigation chords through low-level `SendInput`; `-UseSendKeys` is a fallback for debugging the driver itself.

```powershell
tools\nav_probe_run_windows.ps1 -Build -Assert -Only ctrl_home_from_middle
tools\nav_probe_run_windows.ps1 -Build -Assert -Summary
```

Use `python tools/nav_probe_assert.py --check-spec docs/dev/nav_contract.json --windows-runner tools/nav_probe_run_windows.ps1 --self-test` to lint scenario ids, driver chord syntax, macOS key-code entries, Windows runner key support, and manifest-completeness checks without launching the GUI.

## Stable Behavior Notes

### Sidebar and filters

Sidebar rows are virtualized and grouped under Today, Yesterday, This Week, and Earlier headers by last update; the headers are omitted while a search query is active. Each row is a single full-width click target showing the title and a language label, and the label follows the large-buffer plain-rendering rule. The list is a metadata projection capped at `DEFAULT_LIST_PASTES_LIMIT` (`512`), and scoped sidebar and paste-picker searches likewise return metadata summaries rather than full bodies.

Smart filters (All, Today, This Week, Recent (30d), Unfiled, Documents, Code, Config, Logs, Links) render as compact chips with the overflow collapsed under a `...` menu. The language filter beneath them always offers an explicit `All languages` entry and stacks with the active smart filter instead of replacing it. Classification, including the Documents grouping of Markdown, reStructuredText, LaTeX, and prose notes, follows [filter semantics](../language-detection.md#filter-and-search-semantics).

Empty filter results never discard work in progress: unsaved content, unsaved metadata, and pending saves keep their paste and edit lock until saving completes. Selecting the already-active paste cancels a queued switch to another, and a repeated load reply cannot overwrite an initialized editor draft.

### Editor

Title edits commit on `Enter` and on title-field blur. The editor header row stays compact (title and language); the Properties drawer holds the expanded metadata fields and is non-modal, so opening it does not disable typing, caret movement, or editor shortcuts. A language label displays as `auto` when automatic detection has not resolved one and as `plain` when the language is manually pinned without a value. The GUI has no folder create, edit, or move controls; organization is by smart filters and search. Virtual-editor highlight debounce and staging policy is defined in [highlighting.md](highlighting.md).

Typing and paste reveal the caret with minimal scrolling, including the inserted tail of a multiline paste, and manual scrolling stays where it is left until the next edit or navigation action. Document jumps and Find results center the caret instead, independent of editor focus. Loading another paste resets the viewport to the first line before any search-match reveal, even if the previous paste was scrolled to its end. Virtual rows use zero vertical item spacing so hit testing and scrolling share the rendered row height.

An editor-owned drag keeps extending the selection and autoscrolling outside the viewport or window, foreground overlays retain their pointer ownership, and dragging the scrollbar preserves the selection.

In a paste whose current language is Markdown (including the `md` alias), pasting a single HTTP(S) URL over a non-empty selection inserts `[selection](URL)` as one undoable edit; escaping, inline code, and line-break handling live in `format_markdown_link`. Other languages, empty selections, selections that are themselves URLs, and non-URL clipboard text paste normally, so selecting the destination of an existing link and pasting a URL swaps it rather than nesting a new link.

### Search and paste picker

The sidebar and the paste picker each keep their own query and field scope for the session: All fields (default), Title, Metadata, or Body. The [search read paths](../architecture.md#5-read-and-write-paths) determine which fields are loaded and ranked. Responses and backend cache keys carry the scope and collection, so delayed results cannot leak between contexts, and collection rules are applied in the backend before the result limit, so a matching row older than the first 512 unfiltered results still appears. Debounced searches schedule their own repaint deadline, so a typed query dispatches without further input or focus changes, while pending or failed requests do not create a repaint loop.

Changing the sidebar scope keeps the existing rows, the open document, and its reading position until replacement results arrive, even when nothing matches. A failed sidebar search keeps those rows and shows `Retry search`; retrying or changing the query or a filter resumes requests, and a stale failure cannot replace the current query's status.

The paste picker is separate from the command palette: the palette lists actions only (for example Export, Duplicate, Copy, Copy link, Find, Properties, History, and Diff), and the picker lists paste rows. A result opens on click or `Enter` and offers Copy, Copy Fenced, and Delete. While a scoped request is in flight the picker shows `Searching...`; a failure stays visible with a Retry button, requests resume only on Retry or a query or scope change, and a successful response clears the error. Changing the query or scope clears old results immediately. Closing the picker discards its results and resets selection while retaining the query and scope; opening it selects the retained query for replacement and re-dispatches the search, so responses discarded while it was closed cannot leave it empty. Once rows arrive after a reopen or a query or scope change, selection resets to the first result and the list scrolls to the top, after which manual scrolling is preserved. All fields and Body results carry a compact excerpt around the first literal body match; Title and Metadata results are metadata only.

Opening a result focuses its editor after the paste loads. Text and paste input received while the previous draft saves or the chosen body loads is held for the accepted selection and replayed into the editor; a pointer press, `Escape`, another app shortcut, or a failed open discards it. A result outside the current sidebar results stays selected through background refreshes until the sidebar selection, search query, or collection or language filter changes.

Copying a picker result leaves the editor selection and draft intact. Only the latest copy request is honored: older loaded, missing, or failed replies cannot consume a newer request, even one for the same paste, and a completed copy from the editor, toolbar, a text field, or a selectable label supersedes an older pending picker copy (a copy with no selected text does not).

Deleting a different paste from the picker restores the originating input and keeps the editor draft. Deleting the open paste keeps the picker as keyboard owner, including for typing and paste into its query, and blocks dismissal, discovery toggles, and unrelated selection changes until the delete reply and the replacement paste load settle. A selection queued before the delete is preferred over the adjacent fallback, a save, delete, or load failure releases the block while keeping the picker, and after a successful replacement load `Escape` returns to the originating input.

### Find

Editor-toolbar `Find` searches the open paste body, selects the active match in the virtual editor, and scrolls it into view. Switching pastes with Find open selects the first match of the retained query. Opening a paste from a sidebar or picker All fields or Body search primes the find bar with that query when it appears in the body; picker opens carry their originating query through loading and save-before-switch, including when reopening the active draft, and metadata-only picker hits leave the existing Find query alone.

Growing the query refines the current match from its start. Clicking Prev, Next, or Case and pressing `Enter`/`Shift+Enter` keep focus in the query field, with `Enter` and `Shift+Enter` moving to the next and previous match. `Escape` in the query field or `Close` returns focus to the editor, preserving the matched selection and the query for reopening.

### Discovery overlays and help

The command palette, paste picker, and shortcut help are mutually exclusive: opening one closes the other two so its query owns keyboard input. `Escape` or the toggle shortcut returns focus to the input that opened discovery, including across switches between surfaces. Input batched around a transition stays in order, with earlier text belonging to the previous input and later text to the new destination, and `Enter` on a palette command or picker result processes the preceding query before routing later text. Commands that open another workflow keep their intended focus destination.

While any of the three is open, background New, Paste into Editor, Save, Delete, sidebar-search focus, and Properties shortcuts are blocked, including delayed Paste as New clipboard replies; actions chosen inside the palette remain available. [Diff and History](#diff-and-history-workflows) use separate workflow fences. The palette keeps the selected command visible during keyboard navigation and query changes.

App-level shortcut dispatch, palette hints, and shortcut help share one runtime shortcut registry, and dispatch preserves native event order, including repeated chords. Help lists the app bindings plus selected editor combinations and omits basic navigation and the standard select/copy/cut/undo/redo keys. Palette query terms such as `diff` and `history` are palette discoverability rather than keyboard shortcuts, so help intentionally excludes them.

The `F1` window has a stable scrollable layout with key labels in the native platform's spelling. Search matches descriptions and key combinations, omits empty sections, reports when nothing matches, and returns to the first result when edited. Clear keeps search focus; `Escape`, `F1`, and Close return focus to the input that opened help, unless a different discovery surface opened in its place. Opening help over History or Diff hides that window without disturbing its workflow; closing help brings it back, and `Escape` dismisses only help.

### Startup and styling

The app has a single dark palette. It selects the dark theme before installing custom fonts and spacing, so a light system theme cannot replace it, and editor geometry uses the resolved font if the named Editor text style is missing.

## Diff And History Workflows

- Editor toolbar exposes `Diff` and `History` for the selected paste.
- Command palette exposes `Open diff modal` and `Open history modal` when a paste is selected.
- Opening either window closes discovery surfaces and prevents command-palette and paste-picker opens. Escape closes the version window.
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
  - create/delete/paste-into-editor and other destructive workflow shortcuts are blocked while a version window is open; New and Delete report why,
  - autosave and explicit save still persist already-dirty content/metadata while a version window is open,
  - selection changes and automatic reselection are blocked while a version window is open; a refused sidebar selection preserves the picker-opened document and its pending input,
  - the selected paste stays pinned during a queued hard reset,
  - the selected paste is temporarily read-only until reset success/error arrives.
- Diff preview generation runs on the backend worker against frozen left/right text snapshots; the UI only renders cached results.
- Reset and snapshot loading clear their in-flight UI state only for matching version-load/reset failures so unrelated backend errors cannot tear down the modal context.

## Language/Highlight QA (Magika + Fallback)

Run this checklist when touching detection/highlight/filter code.

1. Start GUI with default features (Magika enabled): `cargo run -p localpaste_gui --bin localpaste-gui`.
2. Create new pastes with representative snippets and confirm the detected language chip (auto mode). Use actual line breaks where `\n` is shown:
   - Rust: `fn main() { println!("hi"); }` -> `rust`
   - Python: `import os\nprint(os.getcwd())` -> `python`
   - Shell: `#!/bin/bash\necho hi` -> `shell`
   - JSON: `{"key":"value"}` -> `json`
3. Open Properties drawer, set language to `Plain text`, save, and verify chip reads `plain` (not `auto`).
4. With that same paste still manual plain, edit content into obvious Rust and verify language remains `plain`.
5. Switch language back to `Auto`, save, and verify the language chip shows unresolved auto state.
6. Make a content edit and verify auto-detection resolves and locks to `rust`.
7. Validate alias interoperability in UI filtering:
   - Set active language filter to `cs`; verify both `csharp` and `cs` pastes remain visible.
   - Set active language filter to `shell`; verify `bash`/`sh` labeled content matches.
8. Validate syntax resolver behavior against the matrix in [language-detection.md#gui-highlight-resolution](../language-detection.md#gui-highlight-resolution):
   - alias labels should resolve to non-plain grammars where expected,
   - unsupported labels should remain metadata-visible while rendering plain text.
   - In Markdown, put a Rust fence after a `- ` list marker, add an indented body, then an unindented `ordinary prose` line. The final line returns to prose colors; repeat with an ordered marker, tildes, and a following sibling list item.
9. Validate large-buffer guardrail:
   - Use content over the [plain-rendering threshold](highlighting.md#large-buffers) and verify display is plain regardless of language metadata.
10. Re-run keyboard/navigation sanity checks listed in [Keyboard And Navigation Contract](#keyboard-and-navigation-contract) after language UI edits.
11. Repeat with heuristic detection using the [build option](../language-detection.md#feature-topology).

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
   - Focus blurs when clicking outside the editor, when another text field or overlay takes focus, or when the app window loses focus.
4. Core shortcuts and navigation:
   - Verify all contracts in [Keyboard And Navigation Contract](#keyboard-and-navigation-contract).
   - Confirm save transitions dirty -> saved after `Ctrl/Cmd+S`.
5. Commands and paste discovery:
   - `Ctrl/Cmd+K` lists commands; a paste-body query does not produce paste rows.
   - `Ctrl/Cmd+Shift+K` opens the paste picker. With an empty query, open a different paste and confirm its body starts at the first line. With a Body query near the end of a long paste, confirm the result shows a matching excerpt; open it and confirm Find selects and reveals that passage.
   - Copy and Copy Fenced work from picker results without changing the active editor or its unsaved draft; deleting a disposable result removes its row.
   - With a dirty draft, open each of the picker, palette, and help, then press `Ctrl/Cmd+Shift+V`. Clipboard text belongs to that surface; the editor behind it is unchanged and no new paste appears. Choosing the palette's explicit Paste as New action still creates a paste.
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
   - Set the language to Markdown, select a phrase, and paste `https://example.com/docs`: expected `[phrase](https://example.com/docs)`, with the caret after the link. Undo restores the phrase and its selection; Redo restores the link. Save and restart to confirm persistence. Repeat with a non-URL or a plain-text paste and confirm ordinary replacement; a URL pasted without selection remains literal. Select the URL inside `[phrase](https://example.com/docs)` and paste another URL: expected the destination is replaced, not wrapped in a second link.
   - Type in the middle of a paste, switch to another app with `Alt+Tab`/`Cmd+Tab`, copy text there, switch back, and press `Ctrl/Cmd+V`: expected a new paste containing that text, with the previous paste unchanged.
   - Repeat, but press `Ctrl/Cmd+Shift+V` after switching back: expected the text inserted at the previous caret, with the editor focused. Repeat from sidebar search and from the title field; the text lands in the editor, not the focused field.
   - With no paste open, press `Ctrl/Cmd+Shift+V`: the top sidebar paste opens with the text appended on a new last line. Confirm the command palette's Paste as New action still creates a paste.
   - Start Paste as New, open help with `F1`, close it with `Escape`, then paste fresh text with `Ctrl/Cmd+V`. The fresh paste reaches the editor; an older canceled clipboard reply must not overwrite or prepend it.
   - Modified arrow movement/selection (`Ctrl`/`Alt`/`Shift`/`Cmd` + arrows) affects editor selection/caret movement and does not change the sidebar selection.
9. Virtual editor selection:
   - Double-click selects word.
   - On a line longer than 10,000 characters, `End` reaches the true end of the line, and double-click selects the word under the pointer even beyond column 10,000.
   - Triple-click selects line.
   - Drag selection across lines keeps expected range and autoscroll direction.
   - Load `a\rb\n# c\n` (escapes denote actual line separators), move to `b`, and Delete it. Check the visible caret after deletion, undo, and redo; typing `x` after redo produces `ax\r\n# c\n`. Right/Left and Shift+Right/Left cross the CRLF pair without an invisible caret or changing the body.
   - Load Rust `// comment let b = 2; let c = 3;` with actual Unicode separators. Both `let` rows retain Rust colors; `Ctrl/Cmd+Home`, `End`, `Shift+Home`, Copy selects only `// comment`, excluding its separator.
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
