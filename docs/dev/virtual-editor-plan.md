# Virtualized Editor Plan

This document outlines the planned work for a virtualized editor that would replace the current `TextEdit` widget.

**Note:** The rewrite now ships three editor paths:
- `TextEdit` fallback (default editable path)
- `LOCALPASTE_VIRTUAL_PREVIEW=1` read-only viewport renderer
- `LOCALPASTE_VIRTUAL_EDITOR=1` editable rope-backed virtual editor

The virtual editor path is still feature-gated while parity/perf validation completes.

---

- [Virtualized Editor Plan](#virtualized-editor-plan)
  - [Current Editor Responsibilities](#current-editor-responsibilities)
  - [Virtualization Goals](#virtualization-goals)
  - [Data Model Changes Needed](#data-model-changes-needed)
  - [Staged Milestones](#staged-milestones)
  - [Immediate Next Steps](#immediate-next-steps)
  - [Progress](#progress)

---

## Current Editor Responsibilities

- Single `TextEdit::multiline` provides editing (cursor movement, selection, IME, clipboard) and renders the entire document each frame.
- Syntax highlighting drives a full document `LayoutJob`, cached per (content hash, language).
- Undo/redo and dirty tracking are managed via `EditorState`.
- Scroll behaviour relies on `ScrollArea::vertical' wrapping the text edit.

### Discovery

Original chat discussion as of 284a1f03a6c9529247e7a1d3945be1dbbfbed3ba:[^1]

[^1]: good call, tbh.

```text
- I started replacing the editor with a virtualized implementation but it’s a much larger rewrite than it may look at first glance. The current TextEdit widget handles everything—wrapping, cursor movement, selections, IME, clipboard, undo/redo, etc.—and also emits a single galley covering the entire document. To virtualize it we’d need to:

- Replace the widget outright with a custom viewported editor.
- Re‑implement text layout and painting per visible line (breaking that galley into cached per-line galleys so only what’s on screen is laid out).
- Rebuild all editing behaviour on top (cursor logic, selections, multi-line edits, undo stack, IME, clipboard, shortcuts).
- Integrate syntax highlighting per line or per chunk so multi-line constructs still render correctly and stay in sync.
- That means changing EditorState, ripping out the existing highlight cache, layering in new caches for per-line galleys, and writing a new event-processing pipeline. It’s doable, but it’s days of careful work—not something I can safely drop into the branch in one pass without a design doc and incremental rollout plan. One misstep and we lose core editing features.

Suggestion:
- First ship a scoped change that leaves TextEdit in place but limits re-highlighting costs (e.g., chunked highlighting or smarter caching).
- In parallel, design the custom virtualized editor (data structures, undo story, IME handling). We can stub a read-only virtualized view to prove out the rendering/caching path, then add editing features in controlled steps.
- Once we agree on the milestone breakdown, I can start landing pieces without freezing the branch for a giant rewrite.
```

- ## Observed Pain Points

- Highlight recalculation and 'TextEdit` layout cost scale with total characters; large pastes stutter.
- Trackpad scrolling suffers because the full galley is rebuilt/tessellated for every frame.
- Highlight cache invalidation currently flushes on every edit, forcing full recomputation.

## Virtualization Goals

- Only layout + tessellate the visible portion of the document while preserving full editing parity.
- Maintain syntax highlighting accuracy (multi-line constructs, string/brace scopes).
- Keep cursor, selection, IME, and keyboard interactions consistent with the current editor.
- Support undo/redo, dirty tracking, and language detection hooks.

## Data Model Changes Needed

- Replace single `String` buffer references with a structure that can provide slices by row.
- Maintain a per-line galley cache keyed by `(line index, syntax state, content hash)`.
- Store per-line syntax state to resume highlighting without reprocessing from the start.
- Track viewport/cursor state in a new `VirtualEditorState` (scroll offset, row height, cached IME data).

## Current Architecture (2026-02-11)

- `EditorBuffer` is now rope-backed internally (with a `String` mirror for `TextEdit` compatibility).
- `app/virtual_editor/` contains:
  - `buffer.rs`: rope buffer + char/line conversions + edit deltas
  - `state.rs`: cursor/selection/focus/IME state
  - `history.rs`: bounded undo/redo with typing coalescing
  - `layout.rs`: wrap metrics + prefix-height viewport range lookup
  - `input.rs`: egui event -> editor command reducer
- Editable virtual rendering uses `ScrollArea::show_viewport` and variable-height line layout.
- Highlight worker/staged apply flow is shared across `TextEdit` and virtual editor modes.

## Staged Milestones

1. **Highlight Cache Improvements (Short-term)**
   - Chunked highlighting (e.g., 2-4 KB windows) with rolling invalidation instead of full flush.
   - Reuse highlight results for unchanged chunks when editing inside large files.

2. **Rolling Layout Cache (Short-term)**
   - Extract line boundaries once per edit.
   - Cache `LayoutJob` per line; only recompute touched lines.
   - Still render through `TextEdit` (not yet virtualized) but reuse cached galleys to cut per-frame work.

3. **Read-Only Virtualized View (Mid-term)**
   - Build a `ScrollArea::show_rows` viewer using cached line galleys.
   - Confirm performance gains on the large paste scenario; keep existing editor for edits.

4. **Editable Virtualized Editor (Long-term)**
   - Implement custom widget handling keyboard/IME events, selections, cursor painting.
   - Integrate per-line highlight state, layout, and undo stacks.
   - Replace `TextEdit` once parity is confirmed via automated tests + manual QA.

## Immediate Next Steps

- Prioritize chunked highlighting and per-line layout cache (Milestones 1 & 2) to relieve current bottlenecks without retooling the entire editor.
- Draft API/structure for read-only virtualized view so we can begin milestone 3 in parallel when ready.

### Milestone 1 & 2 Detail

- Maintain a line index in EditorState (vector of char offsets) recomputed on edits.
- Introduce a HighlightChunkCache (map: chunk_id -> {start_byte, text_hash, layout_job, syntax_state_after}).
- When text changes, only invalidate chunks intersecting the edited byte range.
- Build the full LayoutJob by concatenating cached per-chunk layout jobs before handing it to the existing TextEdit layouter.
- Parallel change: store a per-line galley cache (line index + wrap-sensitive layout) so we can reuse layout for the same line if wrap width unchanged; fallback to recompute for lines in edited chunks.
- Logging hooks (already in place via LOCALPASTE_PROFILE_HIGHLIGHT) will help verify chunk cache hit rate and highlight/layout timing.

## Progress

- Chunked highlight caching plus chunk galley reuse now power the existing TextEdit workflow, delivering Milestones 1 & 2.
- Set `LOCALPASTE_VIRTUAL_PREVIEW=1` to render a read-only chunk-virtualized preview that exercises the new caches before replacing the editable widget.
- `LOCALPASTE_VIRTUAL_EDITOR=1` enables an editable virtual path with:
  - rope-backed buffer edits
  - keyboard navigation and selection commands
  - clipboard copy/cut/paste
  - undo/redo operation stack
  - IME event handling (`Enabled`/`Preedit`/`Commit`/`Disabled`)
  - soft-wrap-aware variable-height viewport rendering
- Remaining gate is runtime/manual parity verification and performance-gate sign-off before default flip.
