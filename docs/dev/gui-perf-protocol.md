# GUI Performance Checks

The GUI must stay responsive while scrolling, editing, and highlighting large pastes.

## Scope

- English-first editor workflows only.
- Use the [GUI-primary topology](../architecture.md#gui-primary-topology) and [DB ownership rules](../storage.md#operational-expectations).
- Detection behavior: [language-detection.md](../language-detection.md). Virtual-editor highlight debounce and staging policy: [highlighting.md](highlighting.md).
- Primary perf scenario: `perf-scroll-5k-lines`.
- Manual release-gate thresholds:
  - average FPS `>= 45`
  - p95 frame time `<= 25 ms`
  - no multi-second plain fallback during newline-burst editing.
- Potential tighter gate: p95 frame time `<= 16 ms` after newline-burst measurements are captured and reviewed.

Perf runs require an isolated `DB_PATH`; shared writers invalidate the measurements.

## Headless Checks

The [headless list/search test](../../crates/localpaste_gui/tests/headless_workflows.rs) (`list_and_search_latency_stay_within_reasonable_headless_budget`) checks result shape and requires command submission to finish within five seconds. Its timers stop before receiving worker results; it does not measure backend round-trip latency or GUI frame performance. Use the manual frame budgets above for responsiveness checks.

For list Markdown containing repeated inline code, run the real-parser scaling probe:

```powershell
cargo test -p localpaste_gui --lib list_inline_span_scaling_probe -- --ignored --nocapture
```

It reports timings and rendered span counts at 1,000, 5,000, and 10,000 spans. Compare growth across sizes; elapsed times depend on the machine and build profile.

## Prereqs

Use the build matrix in [devlog.md](devlog.md). Minimum binaries required for this protocol:

- `localpaste_tools` / `generate-test-data`
- `localpaste_gui` / `localpaste-gui`

## Runbook

Run with the [GUI trace flags](gui-notes.md#runtime-flags):

```powershell
$env:DB_PATH = Join-Path (Get-Location) "target/lpaste-perf-$([guid]::NewGuid().ToString('N'))"
$env:PORT = "38973"
$env:LP_SERVER = "http://127.0.0.1:$env:PORT"
$env:LOCALPASTE_EDITOR_PERF_LOG = "1"
$env:LOCALPASTE_BACKEND_PERF_LOG = "1"
$env:LOCALPASTE_EDITOR_INPUT_TRACE = "1"
$env:LOCALPASTE_HIGHLIGHT_TRACE = "1"

cargo run -p localpaste_tools --bin generate-test-data -- --clear --yes --count 10000 --folders 50
cargo run -p localpaste_gui --bin localpaste-gui --release
```

While GUI is running, use the API endpoint shown in the status bar (`API: http://...`) for CLI/API compatibility checks. In another PowerShell terminal, seed the named cases used below through that endpoint:

```powershell
$env:LP_SERVER = "http://127.0.0.1:38973" # Replace with the GUI status-bar endpoint.
$fixtures = @{
    "perf-medium-python" = "print('fixture')`n" * 100
    "perf-100kb-python" = "print('fixture')`n" * 6400
    "perf-300kb-rust" = "fn sample() {}`n" * 21000
    "perf-scroll-5k-lines" = "fn sample() {}`n" * 5000
}
foreach ($fixture in $fixtures.GetEnumerator()) {
    $fixture.Value | cargo run --quiet -p localpaste_cli --bin lpaste -- new --name $fixture.Key
}
```

Run this seed block once per fresh dataset; the random generator does not create these fixed names.

For standalone server-only smoke/perf validation, use [devlog.md#runtime-smoke-test-server-cli](devlog.md#runtime-smoke-test-server-cli).

## Dataset Expectations

This runbook seeds a large mixed dataset via `generate-test-data`:

- 10k random pastes with `--count 10000`, plus the four named fixtures
- weighted content-size distribution (small/medium/large/very large)
- language-diverse snippets plus folder/tag metadata

Check the [GUI list limits and search scopes](gui-notes.md#stable-behavior-notes).

## Manual Verification Checklist

Run the full functional GUI checklist first: [gui-notes.md#manual-gui-human-step-checklist-comprehensive](gui-notes.md#manual-gui-human-step-checklist-comprehensive).

Perf gating in this protocol is based on the checks below:

1. Medium paste (~1-10 KiB): typing at start/middle/end stays responsive.
2. Large paste (~10-50 KiB): highlighting stays visible while edits debounce/refresh.
3. Very large paste (~50-256 KiB): async/staged highlight stays stable; transient plain fallback during refresh is acceptable but should not stick.
4. Huge paste: verify the [plain-rendering threshold](../language-detection.md#virtual-editor-async-highlight-flow) and smooth scrolling.
5. Sustained typing: in a 5K-50K line document, hold a key for 3 seconds near the middle; no visible hitching and p95 stays within gate.
6. Long wrapped line: type near the middle of a minified JSON/log payload and verify no multi-frame stalls.
7. Idle baseline: open ~200 KiB content and verify CPU drops near idle between repaint intervals.
8. Window resize reflow: no long plain-text gaps after resize.
9. Trace sanity (when enabled): validate `virtual input`, `highlight`, and `editor/backend perf` logs using the runtime-flag behavior in [gui-notes.md#runtime-flags](gui-notes.md#runtime-flags).

## Related Docs

- Open perf follow-ups: [backlog.md](backlog.md)
