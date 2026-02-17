# Docs

Documentation index for LocalPaste.
Use this page as the canonical map of where each concept is defined.

## Key Docs

| Topic                                                     | Doc                                                                                                                                                  |
| --------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Product/project overview                                  | [../README.md](../README.md)                                                                           |
| Validation workflow + mandatory quality gates             | [dev/devlog.md](dev/devlog.md)                                                       |
| System architecture walkthrough                           | [architecture.md](architecture.md)                                                     |
| Language detection + normalization + highlight resolution | [language-detection.md](language-detection.md)                                         |
| Storage backend + compatibility contract                  | [storage.md](storage.md)                                                               |
| Security posture + public exposure                        | [security.md](security.md)                                                             |
| Service/background operation + lock recovery              | [deployment.md](deployment.md)                                                         |
| Dev docs index                                            | [dev/README.md](dev/README.md)                                                         |
| Development workflow (build/run/test/debug)               | [dev/devlog.md](dev/devlog.md)                                                       |
| Locking behavior (DB owner lock + paste edit locks)       | [dev/locking-model.md](dev/locking-model.md)                                           |
| API route behavior/response shape                         | [../crates/localpaste_server/src/handlers/paste.rs](../crates/localpaste_server/src/handlers/paste.rs) |
| GUI runtime flags + behavior notes                        | [dev/gui-notes.md](dev/gui-notes.md)                                                   |
| GUI perf validation protocol + thresholds                 | [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md)                                   |
| Engineering backlog                                       | [dev/backlog.md](dev/backlog.md)                                                       |
| UI visual tokens                                          | [dev/ui-palette.md](dev/ui-palette.md)                                                 |

## Notes

- Build/run/test command matrix: [dev/devlog.md](dev/devlog.md)
- Runtime architecture: [architecture.md](architecture.md)
- Detection/highlighting behavior: [language-detection.md](language-detection.md)

## Historical Artifacts

- Historical deep-dive artifacts that are no longer active have been removed from the live docs tree.
- Use repository history (`git log -- docs/...`) when historical context is required.
