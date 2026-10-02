# Language Detection And Highlighting

Implementation roots:

- Core detection entrypoint: [`../crates/localpaste_core/src/detection/mod.rs`](../crates/localpaste_core/src/detection/mod.rs)
- GUI highlight pipeline entrypoints:
  - [`../crates/localpaste_gui/src/app/highlight/mod.rs`](../crates/localpaste_gui/src/app/highlight/mod.rs)
  - [`../crates/localpaste_gui/src/app/highlight/worker.rs`](../crates/localpaste_gui/src/app/highlight/worker.rs)

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

Auto mode is intentionally "pending detection":

- switching to auto clears the resolved language label,
- the next content edit re-runs detection,
- if detection resolves a concrete language, that value is locked (`language_is_manual = true`) until explicitly switched back to auto.
- API create requests that omit `language_is_manual` detect immediately and lock when detection resolves; pass `language_is_manual: false` to start unresolved auto mode and defer detection until a later edit.

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
- `tex` -> `latex`
- `restructuredtext`, `restructured text` -> `rst`
- `plaintext`, `plain text`, `plain`, `txt` -> `text`

Unknown values pass through in lowercase.

Manual language picker values are defined centrally in `MANUAL_LANGUAGE_OPTIONS` and stored as normalized values.

## Filter And Search Semantics

Language filter matching normalizes both:

- stored language metadata,
- incoming filter value.

This preserves interoperability across legacy and current labels (for example, `csharp` and `cs`).

Search ranking also checks normalized language values to avoid losing metadata relevance as stored labels evolve.

The Documents collection includes Markdown, reStructuredText, LaTeX, and prose notes without stronger code/config/log/link signals. Explicit document languages take precedence over embedded code and misleading titles/tags. A fallback prose classification yields to filename and tag collection rules, such as `deploy.log` or a `logs` tag. Untyped text is a document only when it appears prose-like; delimited records, hexadecimal blobs, recognized commands, and log-level lines stay out of Documents. Markdown is excluded from Code. The derived `Document` kind is rebuilt through the [storage projection repair policy](storage.md#compatibility-policy).

## Text Export Extensions

[`preferred_extension`](../crates/localpaste_core/src/detection/extensions.rs) centrally maps recognized text formats to conventional extensions, independently of grammar support. CSV and TSV export as `.csv` and `.tsv`; document aliases resolve to `.md`, `.rst`, or `.tex`. Unsupported highlighting does not force `.txt`; unknown formats still use `.txt`. Export writes the current editor content unchanged.

## GUI Highlight Resolution

Markdown uses the project-owned [LocalPaste Markdown grammar](../crates/localpaste_gui/assets/LocalPaste-Markdown.sublime-syntax). Footnote markers end at the reference/definition boundary so body text remains readable. Indented, list, and blockquote fence prefixes are recognized; matching or longer closing fences terminate the block. Escaped punctuation stays literal in prose; backslashes inside code do not escape delimiters. Inline-code spans retain their matching delimiter across content lines, but an unmatched span ends at a blank paragraph boundary. Fenced bodies use one existing string color, with no embedded-language highlighting. Scope mappings use the current theme's foreground, string, and keyword colors.

Other GUI highlight resolution uses a multi-step strategy instead of a fixed name table:

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

Fallback candidate mapping lives in:

- [`../crates/localpaste_gui/src/app/highlight/syntax.rs`](../crates/localpaste_gui/src/app/highlight/syntax.rs) (`syntax_fallback_candidates`)

Fallback coverage tests live in:

- [`../crates/localpaste_gui/src/app/highlight/worker.rs`](../crates/localpaste_gui/src/app/highlight/worker.rs) (`resolver_tests`)

Examples covered by the resolver tests include:

- fallback-to-grammar labels (for example: `typescript`, `powershell`, `sass`)
- metadata-only/plain-render labels (for example: `zig`, `kotlin`, `dart`)

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
- plain rendering guardrail: `>=256KB` content

Primary implementation:

- request/stage/apply lifecycle: [`../crates/localpaste_gui/src/app/highlight_flow.rs`](../crates/localpaste_gui/src/app/highlight_flow.rs)
- virtual edit hint capture: [`../crates/localpaste_gui/src/app/virtual_ops.rs`](../crates/localpaste_gui/src/app/virtual_ops.rs)
- editor dispatch and debounce usage: [`../crates/localpaste_gui/src/app/ui/editor_panel.rs`](../crates/localpaste_gui/src/app/ui/editor_panel.rs)

## Runtime Provider Default (Magika)

When Magika is enabled, runtime defaults to CPU execution provider:

- env var: `MAGIKA_FORCE_CPU`
- default: `true`
- falsey values (`0`, `false`, `no`, `off`) allow runtime/provider defaults

Reference: [`../.env.example`](../.env.example)

## YAML Refinement Guardrail

YAML auto-detection requires YAML-distinctive structure before accepting a mapping-heavy sample.
Single-line and prose-like flat mappings remain ambiguous because they also match notes,
logs, and email or HTTP headers. YAML is accepted when the sample has a document marker,
nested indentation, flow collections, block scalars, anchors, structured sequence items,
or multiple config-shaped flat mapping lines.

Primary implementation:

- [`../crates/localpaste_core/src/detection/mod.rs`](../crates/localpaste_core/src/detection/mod.rs)

## Validation Targets

When touching detection/highlight behavior, validate:

- core detection tests (`localpaste_core::detection::tests`),
- GUI resolver/worker tests (`localpaste_gui::app::highlight::worker::resolver_tests`),
- GUI manual checks in [dev/gui-notes.md](dev/gui-notes.md),
- GUI perf checks in [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md).
