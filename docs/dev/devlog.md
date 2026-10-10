# Development Guide

Build and run the workspace binaries with the commands below. See [runtime architecture](../architecture.md) for process topology.

## Binary Map

| Crate under `crates/` | Binaries | Purpose |
| --- | --- | --- |
| `localpaste_core` | Library only | Config, models, storage, shared operations |
| `localpaste_gui` | `localpaste-gui` | Native desktop app |
| `localpaste_server` | `localpaste` | Headless HTTP API |
| `localpaste_cli` | `lpaste` | HTTP client and endpoint discovery |
| `localpaste_tools` | `generate-test-data`, `check-loc`, `check-ast-dupes` | Fixtures, line-count policy, duplicate/dead-symbol audit |

## Build Matrix

```bash
# Build all workspace binaries.
cargo build --workspace --release
```

For service and shell use, install the server and CLI into Cargo's binary directory:

```bash
cargo install --path crates/localpaste_server --bin localpaste
cargo install --path crates/localpaste_cli --bin lpaste
```

## Run Matrix

```bash
# GUI
cargo run -p localpaste_gui --bin localpaste-gui

# Server
cargo run -p localpaste_server --bin localpaste --release

# CLI (built binary)
./target/release/lpaste --help
```

For editor-mode flags and tracing env vars, see [gui-notes.md](gui-notes.md).

For repeatable GUI perf validation, see [gui-perf-protocol.md](gui-perf-protocol.md).

## Validation Loop

Run this loop when touching Rust/runtime behavior.

```bash
# 1) format
cargo fmt --all

# 2) lint
cargo clippy --workspace --all-targets --all-features

# 3) full compile check
cargo check --workspace --all-targets --all-features

# 4) LoC policy check
cargo run -p localpaste_tools --bin check-loc -- --max-lines 1000 --warn-lines 900

# 5) duplicate/dead-symbol audit
cargo run -p localpaste_tools --bin check-ast-dupes -- --root crates

# For broad refactors or test-suite consolidation, include tests in the audit.
cargo run -p localpaste_tools --bin check-ast-dupes -- --root crates --include-tests

# 6) targeted tests for touched areas
# cargo test -p <crate>

# 7) full build
cargo build --workspace --all-targets --all-features

# 8) docs contract check
rustdoc-checker crates --strict
```

After these checks, run the [server/CLI smoke test](#runtime-smoke-test-server-cli) and the [GUI checklist](gui-notes.md#manual-gui-human-step-checklist-comprehensive) as applicable, then commit the validated change. Resolve warnings or record a reason in the [backlog](backlog.md) or [LOC exceptions](loc-exceptions.toml).

Documentation-only changes need checks for the content changed, not the full Rust loop.

For changes to `.github/workflows/*`, `.github/scripts/*`, or GUI packaging, run these checks locally. See [workflow triggers](release-pipeline.md#workflow-triggers) for automated runs.

```bash
# release helper regression tests
python -m unittest discover -s .github/scripts -p 'test_*.py'

# workflow YAML + shell/release invariant validation
# requires yamllint in PATH
python .github/scripts/validate_workflow.py .github/workflows
```

The workflow validator needs PyYAML, `yamllint` on `PATH`, and a working Bash for shell syntax checks.

## Runtime Smoke Test (Server CLI)

Run this API/core smoke test. It validates CRUD behavior and persistence across process restart.

### Bash

```bash
export PORT=3055
mkdir -p target
export DB_PATH="$(mktemp -d "$PWD/target/lpaste-smoke-XXXXXX")"
export LP_SERVER="http://127.0.0.1:$PORT"

cargo build -p localpaste_server --bin localpaste
cargo build -p localpaste_cli --bin lpaste

./target/debug/localpaste &
SERVER_PID=$!
sleep 1

echo "smoke hello" | ./target/debug/lpaste new --name "smoke-test"
ID="$(./target/debug/lpaste list --limit 1 | awk '{print $1}')"
./target/debug/lpaste get "$ID"
./target/debug/lpaste search smoke

# Restart persistence check
kill "$SERVER_PID"
wait "$SERVER_PID" || true
./target/debug/localpaste &
SERVER_PID=$!
sleep 1
./target/debug/lpaste get "$ID"
./target/debug/lpaste delete "$ID"
! ./target/debug/lpaste get "$ID"
./target/debug/lpaste list --limit 10

kill "$SERVER_PID"
```

### PowerShell

```powershell
$env:PORT = "3055"
$env:DB_PATH = Join-Path (Get-Location) "target/lpaste-smoke-$([guid]::NewGuid().ToString('N'))"
$env:LP_SERVER = "http://127.0.0.1:$env:PORT"

cargo build -p localpaste_server --bin localpaste
cargo build -p localpaste_cli --bin lpaste

$proc = Start-Process -FilePath .\target\debug\localpaste.exe -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 1

"smoke hello" | .\target\debug\lpaste.exe new --name "smoke-test"
$id = (.\target\debug\lpaste.exe list --limit 1) -split ' ' | Select-Object -First 1
.\target\debug\lpaste.exe get $id
.\target\debug\lpaste.exe search smoke

# Restart persistence check
Stop-Process -Id $proc.Id
$proc = Start-Process -FilePath .\target\debug\localpaste.exe -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 1
.\target\debug\lpaste.exe get $id
.\target\debug\lpaste.exe delete $id
.\target\debug\lpaste.exe get $id; if ($LASTEXITCODE -eq 0) { throw "deleted paste still exists" }
.\target\debug\lpaste.exe list --limit 10

Stop-Process -Id $proc.Id
```

The isolated test database remains under `target/` for inspection. The final list must exclude the deleted paste. Use an unused `PORT` if 3055 is occupied.

## Tooling CLI Contracts

`localpaste_tools` command behavior:

### `generate-test-data`

- Database target policy:
  - requires explicit database intent via `--db-path` or `DB_PATH`
  - platform-default `DB_PATH` use is rejected unless `--allow-default-db` is supplied
  - blank `DB_PATH` is rejected
- Destructive clear policy:
  - `--clear` requires `--yes`
- Side effects:
  - opens the chosen database path as a writer and mutates paste/folder data

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

- Audit scope:
  - default scans production bodies; `--include-tests` also compares test bodies
  - test context follows enclosing `cfg(test)` modules, including out-of-line modules and `#[path]` declarations
  - module links are discovered before standalone roots, independent of source-file ordering; declared `mod.rs` files inherit context, and `src/bin/*.rs` entrypoints resolve child modules beside the entrypoint
  - files shared by test and production declarations remain in the production audit; disconnected or unresolved files are audited conservatively without Cargo target metadata
  - test-only helpers are excluded from dead-symbol and visibility findings
  - `tests/` path segments are relative to `--root`; an ancestor directory named `tests` does not hide production files
  - recognized callback paths in attribute arguments (for example, Clap `value_parser` and Serde callback values) count as usage; the audit does not expand procedural macros
- Parse-time validation:
  - `--threshold` in `[0.0, 1.0]`
  - `--near-miss-threshold` in `[0.0, 1.0]`
  - `--k > 0`
  - `--min-nodes > 0`
- Runtime validation:
  - `--near-miss-threshold <= --threshold`
  - `--root` must exist and be a directory
- Parse-error policy:
  - default: parse errors fail the run, including files whose test bodies are excluded
  - override: `--allow-parse-errors` allows continued reporting with partial coverage
- `--fail-on-findings` policy:
  - fails on any reported finding category (duplicates, near-misses, likely-dead, visibility-tighten candidates)
