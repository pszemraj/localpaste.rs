# Virtualized Editor Plan

This document is the historical rollout log for the virtual editor.
Normative current-state definitions live in canonical docs:

- runtime flags and behavior notes: [gui-notes.md](gui-notes.md)
- release/perf validation gate: [gui-perf-protocol.md](gui-perf-protocol.md)
- merge readiness and parity status: [parity-checklist.md](parity-checklist.md)

## Current Product Position

- `VirtualEditor` is the default editable path.
- `TextEdit` remains available as a fallback via `LOCALPASTE_VIRTUAL_EDITOR=0`.
- Scope is English-first editor UX for this cycle; multilingual/IME behavior is best-effort and non-blocking.

## Rollout Timeline (Condensed)

### 2026-02-11

- Landed rope-backed editor primitives (`buffer/state/history/layout/input` modules).
- Switched editable path to viewport-based virtual rendering with async highlighting.
- Added perf instrumentation controls (see [gui-notes.md](gui-notes.md)).

### 2026-02-12

- Completed reliability hardening for selection, click semantics, staged highlight apply/drop, and focus-gated command routing.
- Completed module decomposition of GUI app runtime paths to keep files maintainable.
- Set virtual editor as default with explicit fallback switch.

Detailed gate-level status is intentionally tracked only in [parity-checklist.md](parity-checklist.md).

## Post-Merge Follow-Up Themes

Active follow-up work should be tracked in [parity-checklist.md](parity-checklist.md), but expected themes remain:

- keep re-validating highlight behavior under newline-burst editing in large buffers
- continue periodic manual parity checks for selection/navigation/undo-redo workflows
- preserve unfocused paste non-regression (`Ctrl/Cmd+V` creates new paste when editor is unfocused)
- remove `TextEdit` fallback only after parity/perf confidence is established
