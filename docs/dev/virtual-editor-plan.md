# Virtualized Editor Plan

This document is a historical rollout log for the virtual editor.
Current operational definitions and merge-gate status are authoritative elsewhere:

- Runtime flags and editor behavior: [gui-notes.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-notes.md)
- Release/perf validation protocol: [gui-perf-protocol.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md)
- Merge readiness and parity status: [parity-checklist.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/parity-checklist.md)
- Canonical documentation map: [docs/README.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/README.md)

This file is retained as a historical design timeline and should not be used as a behavioral reference.

## Rollout Timeline (Condensed)

### 2026-02-11

- Landed rope-backed editor primitives (`buffer/state/history/layout/input` modules).
- Switched editable path to viewport-based virtual rendering with async highlighting.
- Added perf instrumentation controls (see [gui-notes.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-notes.md)).

### 2026-02-12

- Completed reliability hardening for selection, click semantics, staged highlight apply/drop, and focus-gated command routing.
- Completed module decomposition of GUI app runtime paths to keep files maintainable.
- Set virtual editor as default with explicit fallback switch.

## Post-Merge Follow-Up Themes

Active follow-up work should be tracked in [parity-checklist.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/parity-checklist.md), but expected themes remain:

- keep re-validating highlight behavior under newline-burst editing in large buffers
- continue periodic manual parity checks for selection/navigation/undo-redo workflows
- preserve unfocused paste non-regression (`Ctrl/Cmd+V` creates new paste when editor is unfocused)
- remove `TextEdit` fallback only after parity/perf confidence is established
