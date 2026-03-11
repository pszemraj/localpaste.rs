# GUI Perf Test Protocol (Rewrite)

Release-gate evidence and regression checks for GUI perf.

## Scope

- English-first editor workflows only.
- Runtime topology for this protocol: the GUI owns the DB lock and runs the embedded API endpoint in-process.
- Keep exactly one writer process per `DB_PATH` during perf runs
  (storage contract: [docs/storage.md](../storage.md)).
- Detection/highlight behavior definitions (including virtual-editor async debounce/staging policy) are maintained in [docs/language-detection.md](../language-detection.md).
- Primary perf scenario: `perf-scroll-5k-lines`.
- Manual release-gate thresholds:
  - average FPS `>= 45`
  - p95 frame time `<= 25 ms`
  - no multi-second plain fallback during newline-burst editing.
- Next gate target after virtual-editor Phase 1+2 perf changes:
  - p95 frame time `<= 16 ms` once post-change measurements are captured and reviewed.
  - Until that measurement evidence is captured, keep the current `<= 25 ms` release gate.

> [!IMPORTANT]
> Reuse of a shared `DB_PATH` with another writer invalidates perf results and can introduce lock contention artifacts.

## Automated Test Budget (CI/Headless)

- Automated headless tests use a broad regression budget, not release gating:
  - list latency `< 5s`
  - search latency `< 5s`
- Source: `crates/localpaste_gui/tests/headless_workflows.rs` (`list_and_search_latency_stay_within_reasonable_headless_budget`).

## Prereqs

Use the build matrix in [devlog.md](devlog.md).
Minimum binaries required for this protocol:

- `localpaste_tools` / `generate-test-data`
- `localpaste_gui` / `localpaste-gui`

## Runbook

Run this for reproducible perf checks:
Flag behavior/meanings are documented in [gui-notes.md](gui-notes.md); this runbook only pins values used during perf validation.

```powershell
$env:DB_PATH = Join-Path $env:TEMP "lpaste-perf-$([guid]::NewGuid().ToString('N'))"
$env:PORT = "38973"
$env:LP_SERVER = "http://127.0.0.1:$env:PORT"
$env:LOCALPASTE_EDITOR_PERF_LOG = "1"
$env:LOCALPASTE_BACKEND_PERF_LOG = "1"
$env:LOCALPASTE_EDITOR_INPUT_TRACE = "1"
$env:LOCALPASTE_HIGHLIGHT_TRACE = "1"

cargo run -p localpaste_tools --bin generate-test-data -- --clear --count 10000 --folders 50
cargo run -p localpaste_gui --bin localpaste-gui --release
```

While GUI is running, use the API endpoint shown in the status bar (`API: http://...`) for CLI/API compatibility checks.
For standalone server-only smoke/perf validation, use
[devlog.md#runtime-smoke-test-server-cli](devlog.md#runtime-smoke-test-server-cli).

## Dataset Expectations

This runbook seeds a large mixed dataset via `generate-test-data`:

- 10k pastes by default (configurable with `--count`)
- weighted content-size distribution (small/medium/large/very large)
- language-diverse snippets plus folder/tag metadata
- GUI sidebar list/search path reads metadata/index projections (content loads only on paste open)
- Sidebar list window is capped by `DEFAULT_LIST_PASTES_LIMIT` (`512`); command palette and search are the global discovery paths.

## Manual Verification Checklist

Run the full functional GUI checklist first:
[gui-notes.md#manual-gui-human-step-checklist-comprehensive](gui-notes.md#manual-gui-human-step-checklist-comprehensive).

Perf gating in this protocol is based on the checks below:

1. Medium paste (~1-10KB): typing at start/middle/end stays responsive.
2. Large paste (~10-50KB): highlighting stays visible while edits debounce/refresh.
3. Very large paste (~50-256KB): async/staged highlight stays stable; transient plain fallback during refresh is acceptable but should not stick.
4. Huge paste (`>= 256KB`): plain fallback is active by design and scrolling stays smooth.
5. Sustained typing: in a 5K-50K line document, hold a key for 3 seconds near the middle; no visible hitching and p95 stays within gate.
6. Long wrapped line: type near the middle of a minified JSON/log payload and verify no multi-frame stalls.
7. Idle baseline: open ~200KB content and verify CPU drops near idle between repaint intervals.
8. Window resize reflow: no long plain-text gaps after resize.
9. Trace sanity (when enabled): validate `virtual input`, `highlight`, and `editor/backend perf` logs using the runtime-flag behavior in [gui-notes.md#runtime-flags](gui-notes.md#runtime-flags).

## Related Docs

- Editor flags and trace env vars: [gui-notes.md](gui-notes.md)
- Detection/normalization/highlight behavior: [docs/language-detection.md](../language-detection.md)
- Open perf follow-ups: [backlog.md](backlog.md)
- System architecture context: [docs/architecture.md](../architecture.md)
