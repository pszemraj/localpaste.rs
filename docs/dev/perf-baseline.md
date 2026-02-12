# Phase 0 Performance Baseline (2026-01-29)

This is a historical baseline snapshot for early rewrite work. Use `docs/dev/gui-perf-protocol.md` for current virtual-editor performance verification.

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

## Manual GUI Baseline (Historical Snapshot)

These checks were used before the editable virtual editor path became default:

1) **Scroll FPS (10k snippets)**
   - Run: `cargo run -p localpaste_gui --bin localpaste-gui --features debug-tools,profile`
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

## Current Rewrite Gate

Use `docs/dev/gui-perf-protocol.md` for release-gate validation.
Virtual editor is now default; set `LOCALPASTE_VIRTUAL_EDITOR=1` only when you want to force it explicitly.

Required gate on `perf-scroll-5k-lines`:

- Average FPS `>= 45`
- p95 frame time `<= 25 ms`
- No visible hitching during rapid scroll + mid-document typing
