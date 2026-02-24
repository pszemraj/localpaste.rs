# Docs

Documentation index for LocalPaste.
Use this page as the canonical map of where each concept is defined.
When two docs overlap, defer to the "Primary Source" listed below.

## Canonical Sources

| Concept                                                | Primary Source                                                                                        |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------- |
| Product overview + quick start                         | [README.md](../README.md)                                                                             |
| Runtime topology + endpoint discovery/trust            | [docs/architecture.md#2-runtime-topologies](architecture.md#2-runtime-topologies)                     |
| Single-writer `DB_PATH` + on-disk storage contract     | [docs/storage.md](storage.md)                                                                         |
| Build/run/validation workflow                          | [docs/dev/devlog.md#validation-loop](dev/devlog.md#validation-loop)                                   |
| Server+CLI smoke test (includes restart persistence)   | [docs/dev/devlog.md#runtime-smoke-test-server-cli](dev/devlog.md#runtime-smoke-test-server-cli)      |
| Tooling CLI contracts (`check-loc`, `check-ast-dupes`) | [docs/dev/devlog.md#tooling-cli-contracts](dev/devlog.md#tooling-cli-contracts)                       |
| GUI release pipeline + artifact contract               | [docs/release-gui.md](release-gui.md)                                                                 |
| GUI packaging verification workflow                    | [`.github/workflows/verify-gui-packaging.yml`](../.github/workflows/verify-gui-packaging.yml)         |
| System/runtime architecture                            | [docs/architecture.md](architecture.md)                                                               |
| Detection + normalization + highlighting               | [docs/language-detection.md](language-detection.md)                                                   |
| Storage compatibility contract                         | [docs/storage.md](storage.md)                                                                         |
| Security defaults + exposure policy                    | [docs/security.md](security.md)                                                                       |
| Service operations + recovery                          | [docs/deployment.md](deployment.md)                                                                   |
| Lock semantics (`db.owner.lock`, `423 Locked`)         | [docs/dev/locking-model.md](dev/locking-model.md)                                                     |
| GUI keyboard/navigation interaction contract            | [docs/dev/gui-notes.md#keyboard-and-navigation-contract](dev/gui-notes.md#keyboard-and-navigation-contract) |
| GUI runtime behavior/flags                             | [docs/dev/gui-notes.md#runtime-flags](dev/gui-notes.md#runtime-flags)                                 |
| GUI perf protocol + thresholds                         | [docs/dev/gui-perf-protocol.md](dev/gui-perf-protocol.md)                                             |
| Engineering backlog                                    | [docs/dev/backlog.md](dev/backlog.md)                                                                 |
| UI design tokens                                       | [docs/dev/ui-palette.md](dev/ui-palette.md)                                                           |
| API behavior contract (code)                           | [`crates/localpaste_server/src/handlers/paste.rs`](../crates/localpaste_server/src/handlers/paste.rs) |

## Supporting Indexes

- Dev docs index: [docs/dev/README.md](dev/README.md)

## Historical Artifacts

- Historical deep-dive artifacts that are no longer active have been removed from the live docs tree.
- Use repository history (`git log -- docs/...`) when historical context is required.
