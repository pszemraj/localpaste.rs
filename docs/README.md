# Docs

## Source Of Truth Map

Use these as canonical references. Other docs should link here instead of restating procedures.

| Topic | Canonical doc |
| --- | --- |
| Security posture + public exposure | [security.md](security.md) |
| Service/background operation | [deployment.md](deployment.md) |
| Development workflow (build/run/test/debug) | [dev/devlog.md](dev/devlog.md) |
| GUI runtime flags + behavior notes | [dev/gui-notes.md](dev/gui-notes.md) |
| Rewrite parity status + merge gate | [dev/parity-checklist.md](dev/parity-checklist.md) |
| GUI perf validation protocol + thresholds | [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md) |
| Virtual editor rollout/status | [dev/virtual-editor-plan.md](dev/virtual-editor-plan.md) |
| UI visual tokens | [dev/ui-palette.md](dev/ui-palette.md) |

When updating docs:

- Put normative procedures in the canonical doc for that topic.
- Keep non-canonical docs concise and link back to the canonical source.
- Avoid copying command matrices/flag definitions across multiple files.

## Overview

- [Deployment](deployment.md) - How to run LocalPaste headlessly and manage it as a background service across OSes.
- [Security](security.md) - Default security posture, environment knobs, and guidance for (discouraged) public exposure.

## Development

- [Devlog](dev/devlog.md) - Canonical build/run/test/debug workflow for contributors.
- [GUI notes](dev/gui-notes.md) - Rewrite GUI flags and behavior-specific implementation notes.
- [GUI perf protocol](dev/gui-perf-protocol.md) - Canonical GUI perf validation procedure and thresholds.
- [Parity checklist](dev/parity-checklist.md) - Canonical rewrite parity/merge-gate tracking.
- [Performance baseline](dev/perf-baseline.md) - Historical baseline snapshot (superseded by the perf protocol).
- [UI palette](dev/ui-palette.md) - Canonical color and typography tokens for the rewrite UI.
- [Virtual editor plan](dev/virtual-editor-plan.md) - Virtual-editor rollout history and remaining follow-up items.
