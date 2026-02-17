# Docs

Documentation index for LocalPaste.
Use this page as the canonical map of where each concept is defined.
When two docs overlap, defer to the "Primary Source" listed below.

## Canonical Sources

| Concept | Primary Source |
| --- | --- |
| Product overview + quick start | [README.md](https://github.com/pszemraj/localpaste.rs/blob/main/README.md) |
| Build/run/validation workflow | [docs/dev/devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md) |
| System/runtime architecture | [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md) |
| Detection + normalization + highlighting | [docs/language-detection.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/language-detection.md) |
| Storage compatibility contract | [docs/storage.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/storage.md) |
| Security defaults + exposure policy | [docs/security.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/security.md) |
| Service operations + recovery | [docs/deployment.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/deployment.md) |
| Lock semantics (`db.owner.lock`, `423 Locked`) | [docs/dev/locking-model.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/locking-model.md) |
| GUI runtime behavior/flags | [docs/dev/gui-notes.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-notes.md) |
| GUI perf protocol + thresholds | [docs/dev/gui-perf-protocol.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-perf-protocol.md) |
| Engineering backlog | [docs/dev/backlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/backlog.md) |
| UI design tokens | [docs/dev/ui-palette.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/ui-palette.md) |
| API behavior contract (code) | [`crates/localpaste_server/src/handlers/paste.rs`](https://github.com/pszemraj/localpaste.rs/blob/main/crates/localpaste_server/src/handlers/paste.rs) |

## Supporting Indexes

- Dev docs index: [docs/dev/README.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/README.md)

## Historical Artifacts

- Historical deep-dive artifacts that are no longer active have been removed from the live docs tree.
- Use repository history (`git log -- docs/...`) when historical context is required.
