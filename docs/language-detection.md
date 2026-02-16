# Language Detection And Highlighting

This document describes language detection, normalization, and syntax
highlighting behavior in LocalPaste.

Implementation roots:

- Core detection entrypoint: [`crates/localpaste_core/src/detection/mod.rs`](crates/localpaste_core/src/detection/mod.rs)
- GUI highlight pipeline entrypoints:
  - [`crates/localpaste_gui/src/app/highlight/mod.rs`](crates/localpaste_gui/src/app/highlight/mod.rs)
  - [`crates/localpaste_gui/src/app/highlight/worker.rs`](crates/localpaste_gui/src/app/highlight/worker.rs)

## Feature Topology

- `localpaste_core` keeps `magika` as opt-in (`default = []`).
- `localpaste_gui` and `localpaste_server` enable `magika` by default.
- `localpaste_cli` uses `localpaste_core` defaults, so it is heuristic-only unless explicitly feature-enabled in downstream builds.

This keeps GUI/server detection broad by default while preserving portability for core/CLI users.

## Detection Flow

For auto-detected language (`language_is_manual == false`):

1. If `magika` feature is enabled:
   - run Magika detection,
   - reject non-text results,
   - reject generic labels (`txt`, `randomtxt`, `unknown`, `empty`, `undefined`),
   - normalize and return if non-empty and not `text`.
2. Otherwise (or if Magika is unavailable/fails/generic), run heuristic fallback.
3. Normalize heuristic label and return unless empty/`text`.

For manual language (`language_is_manual == true`), content edits do not re-run auto detection.

> [!IMPORTANT]
> Manual language selection disables automatic re-detection on edits until you switch back to auto mode.

Magika session lifecycle:

- lazy singleton (`OnceLock<Result<Mutex<magika::Session>, String>>`),
- guarded with `Mutex` because Magika identify calls require `&mut self`,
- `prewarm()` is called in GUI/server startup paths to avoid first-save load latency.

## Normalization Contract

Normalization maps legacy aliases and user-entered variants to stable labels (examples):

- `csharp`, `c#` -> `cs`
- `c++` -> `cpp`
- `bash`, `sh`, `zsh` -> `shell`
- `pwsh`, `ps1` -> `powershell`
- `yml` -> `yaml`
- `js` -> `javascript`
- `ts` -> `typescript`
- `md` -> `markdown`
- `plaintext`, `plain text`, `plain`, `txt` -> `text`

Unknown values pass through in lowercase.

Manual language picker values are defined centrally in `MANUAL_LANGUAGE_OPTIONS` and stored as normalized values.

## Filter And Search Semantics

Language filter matching normalizes both:

- stored language metadata,
- incoming filter value.

This preserves interoperability across legacy and current labels (for example, `csharp` and `cs`).

Search ranking also checks normalized language values to avoid losing metadata relevance as stored labels evolve.

## GUI Highlight Resolution

GUI highlight resolution uses a multi-step strategy instead of a fixed name table:

1. exact syntax name
2. exact extension
3. case-insensitive name
4. normalized-name match (alphanumeric only)
5. case-insensitive extension scan
6. explicit fallback candidates for known mismatches/high-priority labels
7. plain text

Policy:

- Keep explicit fallback mapping narrow and intentional.
- Preserve unsupported-language visibility by keeping their metadata labels even when rendering falls back to plain text.

Current high-priority fallback labels:

- `typescript`
- `toml`
- `swift`
- `powershell`

Currently metadata-only (plain rendering) labels:

- `zig`, `scss`, `kotlin`, `elixir`, `dart`

> [!NOTE]
> These labels remain useful for metadata/search/filtering even when rendering is plain text.

## Virtual Editor Async Highlight Flow

Virtual-editor highlight behavior is async and staged to avoid mid-burst visual churn while typing.

Flow:

1. UI sends a highlight request keyed by paste/context (`paste_id`, `revision`, `text_len`, `language_hint`, `theme_key`).
2. Worker coalesces queued requests and computes either:
   - full render (`HighlightRender`), or
   - changed-range patch (`HighlightPatch`) when the UI base snapshot matches the worker cache base.
3. UI merges matching patches into staged/current highlight state.
4. Staged highlight applies:
   - immediately only when there is no current render,
   - otherwise only after idle threshold.

Current policy constants (virtual editor):

- idle apply threshold: `200ms`
- adaptive debounce windows:
  - tiny edits (`<=4` changed chars, `<=2` touched lines): `15ms`
  - medium edits: `35ms`
  - larger supported buffers (`>=64KB`): `50ms`
  - async highlighting disabled: `0ms` (synchronous/no debounce path)
- plain rendering guardrail: `>=256KB` content

Primary implementation:

- request/stage/apply lifecycle: [`crates/localpaste_gui/src/app/highlight_flow.rs`](crates/localpaste_gui/src/app/highlight_flow.rs)
- virtual edit hint capture: [`crates/localpaste_gui/src/app/virtual_ops.rs`](crates/localpaste_gui/src/app/virtual_ops.rs)
- editor dispatch and debounce usage: [`crates/localpaste_gui/src/app/ui/editor_panel.rs`](crates/localpaste_gui/src/app/ui/editor_panel.rs)

## Runtime Provider Default (Magika)

When Magika is enabled, runtime defaults to CPU execution provider:

- env var: `MAGIKA_FORCE_CPU`
- default: `true`
- falsey values (`0`, `false`, `no`, `off`) allow runtime/provider defaults

Reference: [`.env.example`](.env.example)

## Validation Targets

When touching detection/highlight behavior, validate:

- core detection tests (`localpaste_core::detection::tests`),
- GUI resolver/worker tests (`localpaste_gui::app::highlight::worker::resolver_tests`),
- GUI manual checks in [docs/dev/gui-notes.md](docs/dev/gui-notes.md),
- GUI perf checks in [docs/dev/gui-perf-protocol.md](docs/dev/gui-perf-protocol.md).
