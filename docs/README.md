# Documentation Map

## Reference Map

| Concept | Reference |
| --- | --- |
| Product overview and quick start | [../README.md](../README.md) |
| Practical `lpaste` workflows alongside the GUI | [cli-gui-workflows.md](cli-gui-workflows.md) |
| System architecture, runtime topology, and discovery | [architecture.md](architecture.md) |
| Storage backend and single-writer `DB_PATH` contract | [storage.md](storage.md) |
| Security defaults and exposure policy | [security.md](security.md) |
| Service operations and lock recovery | [deployment.md](deployment.md) |
| Detection, normalization, and highlighting | [language-detection.md](language-detection.md) |
| Lock semantics (`db.owner.lock`, paste edit locks, API `423`) | [dev/locking-model.md](dev/locking-model.md) |
| Version history, diff, and metadata retrieval/search path | [architecture.md#5-read-and-write-paths](architecture.md#5-read-and-write-paths) |
| GUI runtime flags and interaction behavior | [dev/gui-notes.md](dev/gui-notes.md) |
| GUI perf protocol and thresholds | [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md) |
| Build/run/validation workflow | [dev/devlog.md](dev/devlog.md) |
| Server+CLI smoke test (restart persistence included) | [dev/devlog.md#runtime-smoke-test-server-cli](dev/devlog.md#runtime-smoke-test-server-cli) |
| Tooling CLI contracts (`check-loc`, `check-ast-dupes`) | [dev/devlog.md#tooling-cli-contracts](dev/devlog.md#tooling-cli-contracts) |
| GUI release pipeline and artifact contract | [release-gui.md](release-gui.md) |
| GUI packaging verification workflow | [../.github/workflows/verify-gui-packaging.yml](../.github/workflows/verify-gui-packaging.yml) |
| Engineering backlog | [dev/backlog.md](dev/backlog.md) |
| UI design tokens | [dev/ui-palette.md](dev/ui-palette.md) |
| API handler behavior (code) | [../crates/localpaste_server/src/handlers/paste.rs](../crates/localpaste_server/src/handlers/paste.rs) |
