# Docs

This file is the documentation index and ownership map.
If two docs repeat the same procedures, keep one canonical and replace the other with a short link.

## Source Of Truth Map

Use these as canonical references. Other docs should link here instead of restating procedures.

| Topic | Canonical doc |
| --- | --- |
| Product/project overview | [../README.md](../README.md) |
| Security posture + public exposure | [security.md](security.md) |
| Service/background operation + lock recovery procedure | [deployment.md](deployment.md) |
| Development workflow (build/run/test/debug) | [dev/devlog.md](dev/devlog.md) |
| API route behavior/response shape | [../crates/localpaste_server/src/handlers/paste.rs](../crates/localpaste_server/src/handlers/paste.rs) |
| GUI runtime flags + behavior notes | [dev/gui-notes.md](dev/gui-notes.md) |
| Rewrite parity status + merge gate | [dev/parity-checklist.md](dev/parity-checklist.md) |
| GUI perf validation protocol + thresholds | [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md) |
| Virtual editor rollout/status | [dev/virtual-editor-plan.md](dev/virtual-editor-plan.md) |
| Storage architecture follow-up plan | [dev/storage-split-plan.md](dev/storage-split-plan.md) |
| UI visual tokens | [dev/ui-palette.md](dev/ui-palette.md) |
| Folder invariants audit | [dev/folder-audit-matrix-2026-02-13.md](dev/folder-audit-matrix-2026-02-13.md) + [dev/folder-audit-report-2026-02-13.md](dev/folder-audit-report-2026-02-13.md) |

## Canonical Rules

When updating docs:

- Put normative procedures in the canonical doc for that topic.
- Keep non-canonical docs concise and link back to the canonical source.
- Avoid copying command matrices, env-flag definitions, or merge-gate checklists across files.
- If context is needed in a non-canonical doc, summarize briefly and link to the canonical source.
- Keep historical docs clearly labeled as historical and non-normative.

## Overview

- [Deployment](deployment.md) - How to run LocalPaste headlessly and manage it as a background service across OSes.
- [Security](security.md) - Default security posture, environment knobs, and guidance for (discouraged) public exposure.

## Development

- [Devlog](dev/devlog.md) - Canonical build/run/test/debug workflow for contributors.
- [GUI notes](dev/gui-notes.md) - Rewrite GUI flags and behavior-specific implementation notes.
- [GUI perf protocol](dev/gui-perf-protocol.md) - Canonical GUI perf validation procedure and thresholds.
- [Parity checklist](dev/parity-checklist.md) - Canonical rewrite parity/merge-gate tracking.
- [Folder audit matrix (2026-02-13)](dev/folder-audit-matrix-2026-02-13.md) - Canonical folder mutation-path inventory with guard/error-contract coverage.
- [Folder audit report (2026-02-13)](dev/folder-audit-report-2026-02-13.md) - Canonical severity-ranked findings and remediation evidence.
- [Invariant audit matrix (superseded)](dev/invariant-audit-matrix.md) - Historical pointer retained for backward links.
- [Invariant audit report (superseded)](dev/invariant-audit-report-2026-02-13.md) - Historical pointer retained for backward links.
- [Performance baseline](dev/perf-baseline.md) - Historical baseline snapshot (superseded by the perf protocol; do not use as a gate).
- [UI palette](dev/ui-palette.md) - Canonical color and typography tokens for the rewrite UI.
- [Virtual editor plan](dev/virtual-editor-plan.md) - Historical rollout timeline (behavior/gates are tracked in GUI notes + parity checklist).
- [Storage split plan](dev/storage-split-plan.md) - Design-complete follow-up plan for metadata/content split storage.
