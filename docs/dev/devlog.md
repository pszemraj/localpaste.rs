# Development Guide

[Architecture](../architecture.md#1-system-at-a-glance) maps each crate to its binaries and runtime role.

## Human-Language Support And Review Scope

LocalPaste is English-first, not English-only. The maintainer is currently its only known user, and the workflows guiding development are primarily in English. Until someone using the project requests broader support and the maintainer revisits this policy, keep implementation and validation proportional to that audience.

Non-English text is welcome, and text integrity and crash prevention remain core requirements regardless of language. Preserve working Unicode behavior, but do not add specialized matching, normalization, collation, word segmentation, input-method handling, or UI localization solely to cover hypothetical language-specific cases. This policy concerns human languages, not the programming languages and file formats LocalPaste detects and highlights.

For human and automated reviews, distinguish a bug in an established behavior or explicit contract from a request to broaden language support. Missing specialized language behavior is a feature request, not a release blocker by itself; do not require exhaustive multilingual coverage without a demonstrated user need. Issues and PRs with a concrete workflow, sample text, and expected behavior are welcome and give us a reason to revisit the scope.

## Build And Run

```bash
# Build all workspace binaries.
cargo build --workspace --release

# GUI
cargo run -p localpaste_gui --bin localpaste-gui

# Server
cargo run -p localpaste_server --bin localpaste --release

# CLI (built binary)
./target/release/lpaste --help
```

A build started without `DB_PATH` opens the default store, the same one an installed LocalPaste uses, and a pre-stable build may run [startup repair](../storage.md#startup-repair) on it. For experiments, export a temporary `DB_PATH`, an unused `PORT`, and a matching `LP_SERVER` as the [smoke test](#runtime-smoke-test-server-cli) does.

The `cargo install` commands for the server and CLI are in the [README](../../README.md#server-and-cli). GUI runtime flags and tracing variables are in [gui-notes.md](gui-notes.md#runtime-flags), and repeatable perf validation is in [gui-perf-protocol.md](gui-perf-protocol.md).

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

# 5) duplicate/dead-symbol audit (reports findings; --fail-on-findings makes them fail the run)
cargo run -p localpaste_tools --bin check-ast-dupes -- --root crates

# For broad refactors or test-suite consolidation, include tests in the audit.
cargo run -p localpaste_tools --bin check-ast-dupes -- --root crates --include-tests

# 6) targeted tests for touched areas
# cargo test -p <crate> --all-features   (localpaste_core runs its Magika tests only with the magika feature, which is off by default)

# 7) full build
cargo build --workspace --all-targets --all-features

# 8) docs contract check (rustdoc-checker is a separate tool, not part of this workspace)
rustdoc-checker crates --strict
```

After these checks, run the [server/CLI smoke test](#runtime-smoke-test-server-cli) and the [GUI checklist](gui-notes.md#manual-gui-human-step-checklist-comprehensive) as applicable, then commit the validated change. Resolve warnings or record a reason in the [backlog](backlog.md) or [LOC exceptions](loc-exceptions.toml).

Documentation-only changes need checks for the content changed, not the full Rust loop.

For changes to `.github/workflows/*` or `.github/scripts/*`, run these checks locally; [Workflows](release-pipeline.md#workflows) lists when CI runs them. They do not exercise `packaging/`, so rehearse packaging changes with a manual `release-gui` run, which packages and checks all three platforms without publishing.

```bash
# release helper regression tests
python -m unittest discover -s .github/scripts -p 'test_*.py'

# workflow YAML + shell/release invariant validation
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

# Wait until the server answers; fail if it exits first.
wait_ready() { until ./target/debug/lpaste list --limit 1 >/dev/null 2>&1; do kill -0 "$SERVER_PID" || return 1; sleep 0.2; done; }

./target/debug/localpaste &
SERVER_PID=$!
wait_ready

echo "smoke hello" | ./target/debug/lpaste new --name "smoke-test"
ID="$(./target/debug/lpaste list --limit 1 | awk '{print $1}')"
./target/debug/lpaste get "$ID"
./target/debug/lpaste search smoke

# Restart persistence check
kill "$SERVER_PID"
wait "$SERVER_PID" || true
./target/debug/localpaste &
SERVER_PID=$!
wait_ready
./target/debug/lpaste get "$ID"
./target/debug/lpaste delete "$ID"
if ./target/debug/lpaste get "$ID" >/dev/null 2>&1; then echo "deleted paste still exists" >&2; fi
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
do { Start-Sleep -Milliseconds 200; .\target\debug\lpaste.exe list --limit 1 *> $null } until ($LASTEXITCODE -eq 0 -or $proc.HasExited)

"smoke hello" | .\target\debug\lpaste.exe new --name "smoke-test"
$id = (.\target\debug\lpaste.exe list --limit 1) -split ' ' | Select-Object -First 1
.\target\debug\lpaste.exe get $id
.\target\debug\lpaste.exe search smoke

# Restart persistence check
Stop-Process -Id $proc.Id
$proc = Start-Process -FilePath .\target\debug\localpaste.exe -WindowStyle Hidden -PassThru
do { Start-Sleep -Milliseconds 200; .\target\debug\lpaste.exe list --limit 1 *> $null } until ($LASTEXITCODE -eq 0 -or $proc.HasExited)
.\target\debug\lpaste.exe get $id
.\target\debug\lpaste.exe delete $id
.\target\debug\lpaste.exe get $id; if ($LASTEXITCODE -eq 0) { throw "deleted paste still exists" }
.\target\debug\lpaste.exe list --limit 10

Stop-Process -Id $proc.Id
```

The isolated test database remains under `target/` for inspection. The final list must exclude the deleted paste. Use an unused `PORT` if 3055 is occupied. The binary paths assume Cargo's default `target/` directory; if a Cargo config redirects it, use `target_directory` from `cargo metadata --no-deps --format-version 1`.

## Tooling CLI Contracts

`localpaste_tools` command behavior:

### `generate-test-data`

- Database target policy:
  - requires explicit database intent via `--db-path` or `DB_PATH`; with neither set, it refuses to fall back to the platform default unless `--allow-default-db` is supplied
  - an explicit path that names the default store is not detected, so never point it at a real store
  - blank `DB_PATH` is rejected
- Destructive clear policy:
  - `--clear` and `--yes` must be passed together
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
