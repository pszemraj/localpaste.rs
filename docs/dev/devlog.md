# Development Guide

This guide covers the development workflow.
Topic-specific ownership references are listed in
[docs/README.md](../README.md).
This guide includes the binary/build/run command matrix used in day-to-day development.
System architecture context lives in
[docs/architecture.md](../architecture.md).

## Workspace Layout

```text
localpaste.rs/
|-- Cargo.toml
|-- crates/
|   |-- localpaste_core/    # config, db, models, naming, errors
|   |-- localpaste_server/  # axum API + embedded server
|   |-- localpaste_gui/     # native rewrite desktop app
|   |-- localpaste_cli/     # lpaste binary
|   `-- localpaste_tools/   # dataset generators / utilities
|-- docs/
|-- assets/
`-- target/
```

## Binary Map

- `localpaste-gui` - rewrite desktop app (`crates/localpaste_gui`)
- `localpaste` - headless API server (`crates/localpaste_server`)
- `lpaste` - CLI client (`crates/localpaste_cli`)
- `generate-test-data` - synthetic dataset tool (`crates/localpaste_tools`)
- `check-loc` - line-count policy checker (`crates/localpaste_tools`)
- `check-ast-dupes` - AST-normalized duplicate/dead-symbol audit (`crates/localpaste_tools`)

## Build Matrix

```bash
# GUI
cargo build -p localpaste_gui --bin localpaste-gui --release

# Server
cargo build -p localpaste_server --bin localpaste --release

# CLI
cargo build -p localpaste_cli --bin lpaste --release

# Tooling
cargo build -p localpaste_tools --bin generate-test-data --release
cargo build -p localpaste_tools --bin check-loc --release
cargo build -p localpaste_tools --bin check-ast-dupes --release
```

## Run Matrix

```bash
# Rewrite GUI
cargo run -p localpaste_gui --bin localpaste-gui

# Server
cargo run -p localpaste_server --bin localpaste --release

# CLI (built binary)
./target/release/lpaste --help
```

Runtime contract references:

- Runtime topologies + endpoint discovery/trust checks:
  [docs/architecture.md#2-runtime-topologies](../architecture.md#2-runtime-topologies)
  and
  [docs/architecture.md#10-discovery-and-trust](../architecture.md#10-discovery-and-trust)
- Single-writer `DB_PATH` + on-disk contract: [docs/storage.md](../storage.md)
- Lock semantics and API `423 Locked` behavior:
  [docs/dev/locking-model.md](locking-model.md)

Day-to-day rule:

- Keep exactly one writer process per `DB_PATH` during local development and validation.

For editor-mode flags and tracing env vars, see
[docs/dev/gui-notes.md](gui-notes.md).
For repeatable GUI perf validation, see
[docs/dev/gui-perf-protocol.md](gui-perf-protocol.md).

## Validation Loop

Policy reference:
This document and linked `docs/dev/*` references define mandatory validation gates and when smoke/manual GUI checks are required.
This section is the quick command reference used during active development.

```bash
# 1) format
cargo fmt --all

# 2) lint
cargo clippy --workspace --all-targets --all-features

# 3) full compile check
cargo check --workspace --all-targets --all-features

# 4) LoC policy check
cargo run -p localpaste_tools --bin check-loc -- --max-lines 1000 --warn-lines 900

# 5) duplicate/dead-symbol audit (required on refactors)
cargo run -p localpaste_tools --bin check-ast-dupes -- --root crates

# 6) targeted tests for touched areas
# cargo test -p <crate>

# 7) runtime smoke (server + CLI + restart persistence)
# run the smoke runbook:
# docs/dev/devlog.md#runtime-smoke-test-server-cli

# 8) docs contract check
rustdoc-checker crates --strict
```

- Manual GUI checklist:
  [docs/dev/gui-notes.md#manual-gui-human-step-checklist-comprehensive](gui-notes.md#manual-gui-human-step-checklist-comprehensive)

Language detection/normalization/highlight behavior is tracked in
[docs/language-detection.md](../language-detection.md).

## Runtime Smoke Test (Server CLI)

Use this API/core smoke runbook.
It validates CRUD behavior and persistence across process restart.

### Bash

```bash
export PORT=3055
export DB_PATH="$(mktemp -d)/lpaste-smoke"
export LP_SERVER="http://127.0.0.1:$PORT"

cargo build -p localpaste_server --bin localpaste
cargo build -p localpaste_cli --bin lpaste

./target/debug/localpaste &
SERVER_PID=$!
sleep 1

echo "smoke hello" | ./target/debug/lpaste new --name "smoke-test"
ID="$(./target/debug/lpaste list --limit 1 | awk '{print $1}')"
./target/debug/lpaste get "$ID"

# Restart persistence check
kill "$SERVER_PID"
./target/debug/localpaste &
SERVER_PID=$!
sleep 1
./target/debug/lpaste get "$ID"
./target/debug/lpaste delete "$ID"

kill "$SERVER_PID"
rm -rf "$DB_PATH"
```

### PowerShell

```powershell
$env:PORT = "3055"
$env:DB_PATH = Join-Path $env:TEMP "lpaste-smoke-$([guid]::NewGuid().ToString('N'))"
$env:LP_SERVER = "http://127.0.0.1:$env:PORT"

cargo build -p localpaste_server --bin localpaste
cargo build -p localpaste_cli --bin lpaste

$proc = Start-Process -FilePath .\target\debug\localpaste.exe -NoNewWindow -PassThru
Start-Sleep -Seconds 1

"smoke hello" | .\target\debug\lpaste.exe new --name "smoke-test"
$id = (.\target\debug\lpaste.exe list --limit 1) -split ' ' | Select-Object -First 1
.\target\debug\lpaste.exe get $id

# Restart persistence check
Stop-Process -Id $proc.Id
$proc = Start-Process -FilePath .\target\debug\localpaste.exe -NoNewWindow -PassThru
Start-Sleep -Seconds 1
.\target\debug\lpaste.exe get $id
.\target\debug\lpaste.exe delete $id

Stop-Process -Id $proc.Id
Remove-Item -Recurse -Force $env:DB_PATH
```

## Tooling CLI Contracts

This section documents `localpaste_tools` CLI behavior
used in automation/CI contracts.

### `check-loc`

- Parse-time validation:
  - `--max-lines > 0`
  - `--warn-lines > 0`
- Runtime validation:
  - `--warn-lines <= --max-lines` (reject contradictory thresholds)
  - `--root` must exist and be a directory
- Exit behavior:
  - exits non-zero on line-count policy violations
  - exits non-zero on malformed exception registries or stale exception paths

### `check-ast-dupes`

- Parse-time validation:
  - `--threshold` in `[0.0, 1.0]`
  - `--near-miss-threshold` in `[0.0, 1.0]`
  - `--k > 0`
  - `--min-nodes > 0`
- Runtime validation:
  - `--near-miss-threshold <= --threshold`
  - `--root` must exist and be a directory
- Parse-error policy:
  - default: parse errors fail the run
  - override: `--allow-parse-errors` allows continued reporting with partial coverage
- `--fail-on-findings` policy:
  - fails on any reported finding category (duplicates, near-misses, likely-dead, visibility-tighten candidates)

## GUI Release Pipeline

Release contract details:
[docs/release-gui.md](../release-gui.md).

Dev-facing quick links:

- Release workflow: [`.github/workflows/release-gui.yml`](../../.github/workflows/release-gui.yml)
- Packaging verification workflow: [`.github/workflows/verify-gui-packaging.yml`](../../.github/workflows/verify-gui-packaging.yml)
- Packaging config roots:
  - [`../../packaging/windows/packager.json`](../../packaging/windows/packager.json)
  - [`../../packaging/linux/packager.json`](../../packaging/linux/packager.json)
  - [`../../packaging/macos/packager.json`](../../packaging/macos/packager.json)

Release workflows intentionally keep validation scoped to packaging/runtime gates rather than running the full local development loop.

## Behavior Contracts

This file is intentionally command/workflow-focused. For runtime behavior contracts, use:

- System/runtime architecture: [docs/architecture.md](../architecture.md)
- Security defaults and env policy: [docs/security.md](../security.md)
- Service operation and lock recovery: [docs/deployment.md](../deployment.md)
- Lock semantics and API `423 Locked` behavior: [docs/dev/locking-model.md](locking-model.md)
- Detection/normalization/highlight behavior: [docs/language-detection.md](../language-detection.md)
- API wiring + handler behavior in code:
  - [`../../crates/localpaste_server/src/lib.rs`](../../crates/localpaste_server/src/lib.rs)
  - [`../../crates/localpaste_server/src/handlers/paste.rs`](../../crates/localpaste_server/src/handlers/paste.rs)
- Engineering backlog: [docs/dev/backlog.md](backlog.md)
