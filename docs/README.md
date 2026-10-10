# Documentation Map

## Using LocalPaste

| Topic | Reference |
| --- | --- |
| Product overview, quick start, and installing the server and CLI | [../README.md](../README.md) |
| Installing the GUI from a release | [release-gui.md](release-gui.md) |
| Using `lpaste` alongside the GUI | [cli-gui-workflows.md](cli-gui-workflows.md) |
| Running the server as a service, backups, and restore | [deployment.md](deployment.md) |
| Security defaults, exposure policy, and environment variables | [security.md](security.md) |
| Storage backend and the single-writer `DB_PATH` contract | [storage.md](storage.md) |
| Language detection, normalization, and highlighting | [language-detection.md](language-detection.md) |

## Developing

| Topic | Reference |
| --- | --- |
| System architecture, runtime topology, and endpoint discovery | [architecture.md](architecture.md) |
| Version history, diff, and metadata retrieval/search path | [architecture.md#5-read-and-write-paths](architecture.md#5-read-and-write-paths) |
| Lock semantics (`db.owner.lock`, paste edit locks, API `423`) | [dev/locking-model.md](dev/locking-model.md) |
| Build, run, and validation workflow | [dev/devlog.md](dev/devlog.md) |
| Server and CLI smoke test, including restart persistence | [dev/devlog.md#runtime-smoke-test-server-cli](dev/devlog.md#runtime-smoke-test-server-cli) |
| Tooling CLI contracts (`check-loc`, `check-ast-dupes`) | [dev/devlog.md#tooling-cli-contracts](dev/devlog.md#tooling-cli-contracts) |
| GUI runtime flags, navigation probe, and interaction behavior | [dev/gui-notes.md](dev/gui-notes.md) |
| GUI highlight pipeline (requests, worker, staging, Markdown grammar) | [dev/highlighting.md](dev/highlighting.md) |
| GUI performance protocol and thresholds | [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md) |
| GUI release workflows, packaging, and cutting a release | [dev/release-pipeline.md](dev/release-pipeline.md) |
| UI design tokens | [dev/ui-palette.md](dev/ui-palette.md) |
| Engineering backlog | [dev/backlog.md](dev/backlog.md) |

There is no separate HTTP API reference. The route table in [`crates/localpaste_server/src/lib.rs`](../crates/localpaste_server/src/lib.rs) lists every endpoint.
