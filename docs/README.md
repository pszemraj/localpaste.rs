# Docs

This is the documentation ownership map.
For any concept, keep one canonical definition and replace duplicate prose elsewhere with a short link.

## Canonical Sources Of Truth

| Topic | Canonical doc |
| --- | --- |
| Product/project overview | [README.md](https://github.com/pszemraj/localpaste.rs/blob/main/README.md) |
| Security posture + public exposure | [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md) |
| Service/background operation + lock recovery | [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md) |
| Development workflow (build/run/test/debug) | [docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md) |
| Locking behavior (DB owner lock + paste edit locks) | [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md) |
| API route behavior/response shape | [crates/localpaste_server/src/handlers/paste.rs](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/handlers/paste.rs) |
| GUI runtime flags + behavior notes | [docs/dev/gui-notes.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-notes.md) |
| GUI perf validation protocol + thresholds | [docs/dev/gui-perf-protocol.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md) |
| Rewrite parity status + merge gate | [docs/dev/parity-checklist.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/parity-checklist.md) |
| Storage architecture follow-up plan | [docs/dev/storage-split-plan.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/storage-split-plan.md) |
| UI visual tokens | [docs/dev/ui-palette.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/ui-palette.md) |
| Folder invariants audit matrix | [docs/dev/folder-audit-matrix-2026-02-13.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/folder-audit-matrix-2026-02-13.md) |
| Folder invariants audit report | [docs/dev/folder-audit-report-2026-02-13.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/folder-audit-report-2026-02-13.md) |

## Documentation Rules

- Put normative procedures in the canonical doc for that topic.
- Keep non-canonical docs concise and link back to canonical docs.
- Do not duplicate command matrices, env-flag definitions, or merge-gate checklists.
- Mark historical/superseded docs clearly as non-normative.

## Historical/Superseded Docs

- [docs/dev/invariant-audit-matrix.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/invariant-audit-matrix.md)
- [docs/dev/invariant-audit-report-2026-02-13.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/invariant-audit-report-2026-02-13.md)
- [docs/dev/perf-baseline.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/perf-baseline.md)
- [docs/dev/virtual-editor-plan.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/virtual-editor-plan.md)
