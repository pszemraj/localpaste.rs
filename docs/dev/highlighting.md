# GUI Highlight Pipeline

Notes on how the editor computes, stages, and displays syntax highlighting. User-visible behavior and the mapping from language labels to grammars are described in [Language Detection And Highlighting](../language-detection.md#gui-highlight-resolution).

Source map:

- [`highlight_flow.rs`](../../crates/localpaste_gui/src/app/highlight_flow.rs): request, staging, and apply lifecycle on the UI side.
- [`highlight/worker.rs`](../../crates/localpaste_gui/src/app/highlight/worker.rs): the background worker and its incremental passes.
- [`highlight/reuse.rs`](../../crates/localpaste_gui/src/app/highlight/reuse.rs): line hashing and cache alignment shared by the worker and its tests.
- [`virtual_ops_apply.rs`](../../crates/localpaste_gui/src/app/virtual_ops_apply.rs): edit hint capture.
- [`ui/editor_panel.rs`](../../crates/localpaste_gui/src/app/ui/editor_panel.rs): per-frame request dispatch and render selection.
- [`highlight/markdown.rs`](../../crates/localpaste_gui/src/app/highlight/markdown.rs): Markdown grammar loading and theme mappings.
- [`app/mod.rs`](../../crates/localpaste_gui/src/app/mod.rs): the `HIGHLIGHT_*` policy constants referenced below.

## Overview

Syntect runs on a worker thread, and the editor never waits for it. Each frame draws the best render available, and results arrive later as either a full render or a patch covering only the changed lines. Because results land asynchronously while edits continue, most of the pipeline decides whether a result is still valid and whether it is safe to show yet.

## Requests

Each frame, the editor panel decides whether to ask the worker for a new render. No request is sent when the buffer has reached `HIGHLIGHT_PLAIN_THRESHOLD`, when an identical request is already pending, or when a current or staged render already matches the buffer exactly. A request is also withheld during the debounce window after an edit, but only when a request is pending or a render already exists, so the first paint of a paste is never delayed.

The debounce window depends on buffer size and edit size. A small edit, at most `HIGHLIGHT_TINY_EDIT_MAX_CHARS` inserted plus deleted characters across at most two lines, in a buffer below `HIGHLIGHT_DEBOUNCE_LARGE_BYTES` uses the short `HIGHLIGHT_DEBOUNCE_TINY` window. Every other edit uses `HIGHLIGHT_DEBOUNCE_MEDIUM`, or `HIGHLIGHT_DEBOUNCE_LARGE` once the buffer reaches the large size, which avoids snapshotting a large buffer on every keystroke.

A request carries a cheap clone of the editor's rope and the identity the UI needs to judge the reply: paste id, buffer epoch, revision, byte length, language hint, and theme key. It also names the newest render the UI already holds for the same paste, language, and theme, which is the base a patch may be applied to, and an optional edit hint. The buffer epoch identifies one lifetime of the editor buffer. Revision numbers restart when a paste is reloaded, so the same paste can legitimately show the same revision and length twice, and only the epoch distinguishes the replies.

## Edit Hints

Each applied edit records a hint holding the start of the edit, the number of lines it touched, and the characters inserted and deleted. The start is a UTF-8 byte offset taken before the edit, not a character index. The worker maps it to a line through the rope, so line numbers agree with the editor's own lines: Ropey treats LF, CRLF, bare CR, VT, FF, NEL, and the Unicode line and paragraph separators as line breaks. The worker begins one byte before the offset, because an edit can move a CRLF boundary behind the edit position and the preceding line then needs to be rechecked.

Syntect grammars expect LF-terminated lines. The parser is given a copy of each line whose non-LF terminator is replaced with LF, and the resulting spans are mapped back onto the original bytes, including the terminator.

## Worker

The worker is a single thread that drains its queue and keeps only the newest request, so a burst of typing costs one pass. Grammars load lazily on the first request. The worker caches the last pass for one language and theme: for every line, the text hash, the byte length, the spans, and the parser and highlighter state at the end of the line. Changing language or theme discards the cache.

Cached lines are reused in one of two ways. The single-edit fast path applies when the request's base matches the cache, the line count is unchanged, the revision is exactly one past the cached revision, and an edit hint is present. Lines before the hint are taken from the cache without inspection, and highlighting restarts at the edit line. Once past the lines the edit can touch, the first line whose hash and starting parser state both match the cache lets the worker take the remaining tail from the cache. A matching line inside the edit span proves nothing about the lines after it: indenting a block, for example, can leave a blank line in the middle unchanged.

Every other request takes the general path. Cached lines are aligned to the new text by matching the common prefix and suffix of line hashes, and each reused line keeps its original index. A line is reused only when its hash matches and the parser state entering it equals the state its original predecessor left. Without that check, deleting a line could splice on a suffix carrying the state of the deleted line, such as the state inside a removed Markdown code fence.

The worker replies with a patch when the request's base matches the cache, the line count is unchanged, and the changed range is smaller than the whole buffer. Otherwise it sends a full render that carries a best-effort changed range for the UI to use. The invariant behind both paths is that an incremental result must equal a cold parse of the same text, which the worker tests assert across CR, CRLF, and the Unicode separators.

## Staging And Applying

A reply is first checked against the buffer epoch, and replies from a replaced buffer are dropped before they can touch pending or staged state. A reply that passes clears the matching pending marker even if validation then rejects it, so the same request can be retried rather than suppressed indefinitely.

A full render is staged unless it is no newer than the current or staged render for the same paste, language, and theme, or is older than the active text while a render is already displayed. A patch is merged into the staged render in place when that render is the patch's base, and otherwise into a copy of the displayed render when that is the base. A patch with an unknown base, a stale revision, or a mismatched line count is dropped, and the next request returns a full render.

A staged render is applied only while it still matches the active text by paste id, revision, and byte length, and is discarded otherwise. It is applied immediately when no render is displayed, and otherwise once the editor has been idle for `HIGHLIGHT_APPLY_IDLE`. This is what keeps colors from changing while typing continues.

Between applies, the editor draws the render that matches the buffer exactly if it has one, and otherwise the latest render for the same paste, language, and theme even when it is slightly stale. Plain text is used only when no such render exists or the buffer is above the plain-render threshold. Applying a render evicts cached text layouts only for the lines it changed, taken from the merged patch ranges or from the render's changed range when its base lines up with the displayed render, and evicts everything when the context or line count differs or the base is unknown.

## Large Buffers

The editor compares the buffer's byte length with `HIGHLIGHT_PLAIN_THRESHOLD`. At or above it, no requests are sent, any pending, staged, and current highlight state is cleared the moment a buffer crosses the threshold, and the buffer is drawn plain. The sidebar and paste picker apply the same threshold when choosing to display `plain` as a paste's language.

## Markdown Grammar

Markdown uses a project-owned grammar rather than syntect's bundled one. The static rules live in [`LocalPaste-Markdown.sublime-syntax`](../../crates/localpaste_gui/assets/LocalPaste-Markdown.sublime-syntax), and `settings()` in `highlight/markdown.rs` appends the list contexts when the grammar loads. They are generated because a backreference can retain an item's indentation but cannot turn a variable-width ordered marker into the matching number of spaces, so one set of contexts is produced for each CommonMark marker width.

The grammar follows CommonMark for the cases that matter to a paste. A fence closes on a delimiter of the same character that is at least as long as the opener. At the top level the closer may be indented by up to three spaces and may not carry a quote or list prefix, inside a list item that allowance is relative to the item's indentation, and a quoted fence ends where its blockquote ends. Fenced bodies take one code color with no embedded-language highlighting. Footnote markers end at the reference or definition boundary, inline code spans may cross lines within a paragraph or list item but end at a blank line or an interrupting block, and structural rules accept both LF and CRLF. The comments in the grammar and the tests in `markdown.rs` pin each of these rules.

At load time the module also appends scope mappings to every bundled theme so that Markdown structure stays readable: prose uses the theme's default foreground, code and link text use its string color, headings and fence or reference markers use its keyword color, and list and quote markers use its numeric-constant color.

## Tracing And Validation

`LOCALPASTE_HIGHLIGHT_TRACE=1` logs request, queue, drop, and apply events from the UI and a timing line for each worker pass, under the `localpaste_gui::highlight` target; see [gui-notes.md](gui-notes.md).

Tests for this pipeline are `localpaste_gui::app::tests::highlight_behaviors` for debounce, staging, and the plain threshold, `localpaste_gui::app::highlight::worker::resolver_tests` for the worker's incremental passes and grammar resolution, and the tests in `highlight/markdown.rs` for the grammar. Manual typing and large-paste checks are in [gui-perf-protocol.md](gui-perf-protocol.md).
