# Phase 0 Performance Baseline (2026-01-29)

Historical snapshot from early rewrite work.
Current release/perf validation source of truth: [gui-perf-protocol.md](gui-perf-protocol.md).

## Environment

- OS: Windows (PowerShell)
- Build: release
- Binaries: `localpaste` + `lpaste`

## Dataset

Generated with:

```
DB_PATH=%TEMP%\localpaste-baseline
cargo run -p localpaste_tools --bin generate-test-data -- --count 10000 --folders 50
```

Notes:

- Size distribution from generator: 10% small, 70% medium, 15% large, 5% very_large.
- Folder count: 50.

## Headless API/CLI Timings (release)

Server:

```
PORT=3052
DB_PATH=%TEMP%\localpaste-baseline
```

CLI:

```
LP_SERVER=http://127.0.0.1:3052
lpaste --timing <command>
```

Measured results (single run):

- `new`: request 3.5 ms, parse 0.1 ms, total 3.6 ms
- `list` (limit 5): request 336.7 ms, parse 0.8 ms, total 337.4 ms
- `search` (query `baseline-test`): request 153.3 ms, parse 0.1 ms, total 153.4 ms
- `get`: request 2.2 ms, parse 0.1 ms, total 2.3 ms
- `delete`: request 2.4 ms

Interpretation:

- `list` and `search` are still O(n) over all pastes; expected to be higher at 10k.

## Historical GUI Notes

The original manual GUI checks from this phase are superseded by the scripted protocol in [gui-perf-protocol.md](gui-perf-protocol.md).
Keep this file for historical timing context only.

## Current Rewrite Gate

Use [gui-perf-protocol.md](gui-perf-protocol.md) for release-gate validation and thresholds.
