# Docs

This is the documentation ownership map.
For any concept, keep one canonical definition and replace duplicate prose elsewhere with a short link.

## Canonical Sources Of Truth

| Topic | Canonical doc |
| --- | --- |
| Product/project overview | [README.md](https://github.com/pszemraj/localpaste.rs/blob/main/README.md) |
| System architecture walkthrough | [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md) |
| Language detection + canonicalization + highlight resolution | [docs/language-detection.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/language-detection.md) |
| Storage backend + compatibility contract | [docs/storage.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/storage.md) |
| Security posture + public exposure | [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md) |
| Service/background operation + lock recovery | [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md) |
| Dev docs index | [docs/dev/README.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/README.md) |
| Development workflow (build/run/test/debug) | [docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md) |
| Locking behavior (DB owner lock + paste edit locks) | [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md) |
| API route behavior/response shape | [crates/localpaste_server/src/handlers/paste.rs](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/handlers/paste.rs) |
| GUI runtime flags + behavior notes | [docs/dev/gui-notes.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-notes.md) |
| GUI perf validation protocol + thresholds | [docs/dev/gui-perf-protocol.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md) |
| Engineering backlog | [docs/dev/backlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/backlog.md) |
| UI visual tokens | [docs/dev/ui-palette.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/ui-palette.md) |

## Documentation Rules

- Put normative procedures in the canonical doc for that topic.
- Keep non-canonical docs concise and link back to canonical docs.
- Do not duplicate command matrices, env-flag definitions, or merge-gate checklists.
- Mark historical/superseded docs clearly as non-normative.

## Historical Artifacts

- Historical deep-dive artifacts that are no longer active have been removed from the live docs tree.
- Use repository history (`git log -- docs/...`) when historical context is required.
