# Phase 0 Performance Baseline (2026-01-29)

This baseline captures headless API/CLI timings and documents the manual GUI measurements still required for Phase 0.

## Environment

- OS: Windows (PowerShell)
- Build: release
- Binaries: `localpaste` + `lpaste`

## Dataset

Generated with:

```
DB_PATH=%TEMP%\localpaste-baseline
cargo run --bin generate-test-data --features cli -- --count 10000 --folders 50
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

## Manual GUI Baseline (Required)

These metrics require a human run of the GUI:

1) **Scroll FPS (10k snippets)**
   - Run: `cargo run --bin localpaste-gui --features gui,debug-tools,profile`
   - Load the 10k dataset.
   - Scroll the sidebar list continuously for ~10 seconds.
   - Record: average FPS and any stutters.

2) **Typing latency**
   - Open a large paste (50-250 KB) and type at speed for ~10 seconds.
   - Record: perceived latency and any frame spikes in the Debug panel.

3) **Search first results**
   - Use filter/search with a query that matches multiple entries.
   - Record: time-to-first-result and any frame time spikes.

4) **Profiler snapshot**
   - Open the profiler panel (Ctrl+Shift+P) and capture a screenshot or note hot paths.

Record results here once measured.
