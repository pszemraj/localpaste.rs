# GUI Perf Test Protocol (Rewrite)

This is the canonical perf validation procedure for the rewrite GUI.
Use this protocol for release-gate evidence and regression checks.

## Scope

- English-first editor workflows only.
- Primary perf scenario: `perf-scroll-5k-lines`.
- Gate thresholds:
  - average FPS `>= 45`
  - p95 frame time `<= 25 ms`
  - no multi-second plain fallback during newline-burst editing.

## Prereqs

```powershell
cargo build -p localpaste_server --bin localpaste --release
cargo build -p localpaste_gui --bin localpaste-gui --release
```

## Canonical Runner

Use the script in `scratch/virtualizedgui-perf-run.ps1` as the single source of truth for:

- isolated temp DB creation
- deterministic perf paste seeding
- server restart verification
- optional GUI launch with perf/input/highlight tracing flags

Example:

```powershell
.\scratch\virtualizedgui-perf-run.ps1 `
  -Profile Release `
  -VirtualMode Editor `
  -PerfLog `
  -InputTrace `
  -HighlightTrace `
  -KeepDb `
  -Port 38973
```

If you only need seed+verification without launching GUI, add `-NoGui`.

## Dataset Expectations

The canonical runner seeds these named pastes:

- `perf-medium-python`
- `perf-100kb-python`
- `perf-300kb-rust`
- `perf-scroll-5k-lines`

## Manual Verification Checklist

1. `perf-medium-python`: typing at start/middle/end stays responsive.
2. `perf-100kb-python`: highlight remains visible while edits debounce/refresh.
3. `perf-300kb-rust`: plain fallback mode is active and scrolling remains smooth.
4. `perf-scroll-5k-lines`: rapid scroll and mid-document typing show no major hitching.
5. Window resize reflow: no long plain-text gaps after resize.
6. Shortcut sanity: `Ctrl/Cmd+N`, `Ctrl/Cmd+Delete`, unfocused `Ctrl/Cmd+V`.
7. Clipboard reliability: `Ctrl/Cmd+C/X/V` including unfocused mutation guard behavior.
8. Trace sanity (when enabled):
   - input trace: deterministic `virtual input frame` routing outcomes
   - highlight trace: deterministic `queue -> worker_done -> apply` (or `apply_now/apply_idle`) with stale staged renders dropped.

## Related Docs

- Editor flags and trace env vars: [gui-notes.md](gui-notes.md)
- Rewrite parity gate: [parity-checklist.md](parity-checklist.md)
- Virtual editor rollout context: [virtual-editor-plan.md](virtual-editor-plan.md)
