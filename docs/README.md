# Docs

Documentation index for LocalPaste.
Use this page as the canonical map of where each concept is defined.
When two docs overlap, defer to the "Primary Source" listed below.

## Canonical Sources

| Concept | Primary Source |
| --- | --- |
| Product overview + quick start | [README.md](../README.md) |
| Build/run/validation workflow | [docs/dev/devlog.md](dev/devlog.md) |
| Tooling CLI contracts (`check-loc`, `check-ast-dupes`) | [docs/dev/devlog.md#tooling-cli-contracts](dev/devlog.md#tooling-cli-contracts) |
| GUI release pipeline + artifact contract | [docs/release-gui.md](release-gui.md) |
| System/runtime architecture | [docs/architecture.md](architecture.md) |
| Detection + normalization + highlighting | [docs/language-detection.md](language-detection.md) |
| Storage compatibility contract | [docs/storage.md](storage.md) |
| Security defaults + exposure policy | [docs/security.md](security.md) |
| Service operations + recovery | [docs/deployment.md](deployment.md) |
| Lock semantics (`db.owner.lock`, `423 Locked`) | [docs/dev/locking-model.md](dev/locking-model.md) |
| GUI runtime behavior/flags | [docs/dev/gui-notes.md](dev/gui-notes.md) |
| GUI perf protocol + thresholds | [docs/dev/gui-perf-protocol.md](dev/gui-perf-protocol.md) |
| Engineering backlog | [docs/dev/backlog.md](dev/backlog.md) |
| UI design tokens | [docs/dev/ui-palette.md](dev/ui-palette.md) |
| API behavior contract (code) | [`crates/localpaste_server/src/handlers/paste.rs`](../crates/localpaste_server/src/handlers/paste.rs) |

## Supporting Indexes

- Dev docs index: [docs/dev/README.md](dev/README.md)

## Historical Artifacts

- Historical deep-dive artifacts that are no longer active have been removed from the live docs tree.
- Use repository history (`git log -- docs/...`) when historical context is required.
