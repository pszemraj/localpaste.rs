# GUI Perf Test Protocol (Rewrite)

This is the canonical perf validation procedure for the rewrite GUI.
Use this protocol for release-gate evidence and regression checks.

## Scope

- English-first editor workflows only.
- Runtime topology for this protocol: the GUI owns the DB lock and runs the embedded API endpoint in-process.
- Do not run standalone `localpaste` concurrently against the same `DB_PATH` while running GUI perf checks.
- Primary perf scenario: `perf-scroll-5k-lines`.
- Manual release-gate thresholds:
  - average FPS `>= 45`
  - p95 frame time `<= 25 ms`
  - no multi-second plain fallback during newline-burst editing.

## Automated Test Budget (CI/Headless)

- Automated headless tests use a broad regression budget, not release gating:
  - list latency `< 5s`
  - search latency `< 5s`
- Source: `crates/localpaste_gui/tests/headless_workflows.rs` (`list_and_search_latency_stay_within_reasonable_headless_budget`).

## Prereqs

Use the canonical build matrix in [devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md).
Minimum binaries required for this protocol:

- `localpaste_tools` / `generate-test-data`
- `localpaste_gui` / `localpaste-gui`

## Canonical Runbook

Use this runbook as the canonical source for reproducible perf checks:
Flag behavior/meanings are canonical in [gui-notes.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-notes.md); this runbook only pins values used during perf validation.

```powershell
$env:DB_PATH = Join-Path $env:TEMP "lpaste-perf-$([guid]::NewGuid().ToString('N'))"
$env:PORT = "38973"
$env:LP_SERVER = "http://127.0.0.1:$env:PORT"
$env:LOCALPASTE_VIRTUAL_EDITOR = "1"
$env:LOCALPASTE_EDITOR_PERF_LOG = "1"
$env:LOCALPASTE_BACKEND_PERF_LOG = "1"
$env:LOCALPASTE_EDITOR_INPUT_TRACE = "1"
$env:LOCALPASTE_HIGHLIGHT_TRACE = "1"

cargo run -p localpaste_tools --bin generate-test-data -- --clear --count 10000 --folders 50
cargo run -p localpaste_gui --bin localpaste-gui --release
```

While GUI is running, use the API endpoint shown in the status bar (`API: http://...`) for CLI/API compatibility checks.
For standalone server-only smoke/perf validation, use the server+CLI CRUD smoke flow in [devlog.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/devlog.md) with `localpaste` + `lpaste`.

## Dataset Expectations

The canonical runbook seeds a large mixed dataset via `generate-test-data`:

- 10k pastes by default (configurable with `--count`)
- weighted content-size distribution (small/medium/large/very large)
- language-diverse snippets plus folder/tag metadata
- GUI sidebar list/search path reads metadata/index projections (content loads only on paste open)
- Sidebar list window is capped by `DEFAULT_LIST_PASTES_LIMIT` (`512`); command palette and search are the global discovery paths.

## Manual Verification Checklist

1. Medium (~1-10KB) code paste: typing at start/middle/end stays responsive.
2. Large (~10-50KB) code paste: highlight remains visible while edits debounce/refresh.
3. Very large (~50-256KB) code paste: plain fallback mode is active and scrolling remains smooth.
4. Long document paste (thousands of lines): rapid scroll and mid-document typing show no major hitching.
5. Window resize reflow: no long plain-text gaps after resize.
6. Shortcut sanity: `Ctrl/Cmd+N`, `Ctrl/Cmd+Delete`, unfocused `Ctrl/Cmd+V`.
7. Clipboard reliability: `Ctrl/Cmd+C/X/V` including unfocused mutation guard behavior.
8. Trace sanity (when enabled):
   - input trace: deterministic `virtual input frame` routing outcomes
   - highlight trace: deterministic `queue -> worker_done -> apply` (or `apply_now/apply_idle`) with stale staged renders dropped.
   - backend perf trace: list/search cache hit+miss counters and per-query latency logs (`localpaste_gui::backend_perf` target).

## Related Docs

- Editor flags and trace env vars: [gui-notes.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/gui-notes.md)
- Rewrite parity gate: [parity-checklist.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/dev/parity-checklist.md)
- System architecture context: [docs/architecture.md](https://github.com/pszemraj/localpaste.rs/blob/main/docs/architecture.md)
