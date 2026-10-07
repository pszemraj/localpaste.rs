# Language Detection And Highlighting

Implementation roots:

- Core detection entrypoint: [`../crates/localpaste_core/src/detection/mod.rs`](../crates/localpaste_core/src/detection/mod.rs)
- GUI highlight pipeline entrypoints:
  - [`../crates/localpaste_gui/src/app/highlight/mod.rs`](../crates/localpaste_gui/src/app/highlight/mod.rs)
  - [`../crates/localpaste_gui/src/app/highlight/worker.rs`](../crates/localpaste_gui/src/app/highlight/worker.rs)

## Feature Topology

- `localpaste_core` keeps `magika` as opt-in (`default = []`).
- `localpaste_gui` and `localpaste_server` enable `magika` by default.
- `localpaste_cli` sends content to the API; detection runs in the receiving GUI/server, not the CLI.

This keeps GUI/server detection broad by default while preserving portability for core/CLI users.

## Detection Flow

For auto-detected language (`language_is_manual == false`):

1. Recognize a standalone Markdown fence, a shell command sequence (including one command after a leading shell comment or prompt), or a Rust runtime panic header optionally following Cargo build/run status or an entered `cargo run` command before statistical detection.
2. If `magika` feature is enabled:
   - run Magika detection,
   - reject non-text results,
   - reject generic labels (`txt`, `randomtxt`, `unknown`, `empty`, `undefined`),
   - normalize and return if non-empty and not `text`.
3. Otherwise (or if Magika is unavailable/fails/generic), run heuristic fallback.
4. Normalize heuristic label and return unless empty/`text`.

Auto mode is intentionally "pending detection":

- switching to auto clears the resolved language label,
- the next content edit re-runs detection,
- if detection resolves a concrete language, that value is locked (`language_is_manual = true`) until explicitly switched back to auto.
- API create requests that omit `language_is_manual` detect immediately and lock when detection resolves; pass `language_is_manual: false` to start unresolved auto mode and defer detection until a later edit.

For manual language (`language_is_manual == true`), content edits do not re-run auto detection.

An inferred Markdown label is refined using [whole-body technical structure](#documents-and-fenced-content). This can replace or reject weak Markdown without changing other statistical labels. Ordinary documents retain their language behavior.

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

### Documents And Fenced Content

The Documents collection uses the derived Document kind, independently of the highlighting label. It includes Markdown, reStructuredText, LaTeX, and prose notes without stronger code/config/log/link signals. Embedded examples and misleading titles/tags do not override a document language. Empty document-language bodies derive Document with no handle or terms; other empty bodies derive Other.

Whole-body technical structure overrides a stored Markdown label: commands derive Code or Other, comment-prefixed Python imports/source derive Code, and runtime output derives Log. Automatically detected labels become locked, so the stored flag cannot distinguish them from user-selected labels; both receive the same content-derived exception. Language and lock fields remain unchanged.

A paste containing only one fence is classified by its info word or body:

- Python/shell fences belong to Code; JSON/YAML fences belong to Config.
- Structurally recognized runtime output belongs to Logs, including in a `text` fence.
- Other explicit `text` fences and unlabeled prose fences remain Documents.
- Unlabeled bodies use structural heuristics without statistical detection or a language lock. Structural data that remains Other does not become prose merely because it is fenced.
- Fences containing a document language, or embedded in surrounding prose, remain Documents.

Retrieval derivation leaves the stored Markdown highlighting/export label intact. Fallback prose needs at least three whitespace-separated words, at least 70% letters, and at most 20% symbols among non-whitespace characters. Single tokens, hexadecimal blobs, and recognized commands remain outside Documents.

Fallback prose yields to explicit filename suffixes, lowercase executable prefixes, URL-shaped names, languages, and whole-word tags (`deploy.log`, a `logs` tag). Substrings such as `log` in `holographic` or `script` in `transcript` do not exclude prose. Ambiguous `make`/`just` title prefixes need a command-derived kind.

### Commands

Command handles use the first command after optional leading shell comments and `$`, `%`, or `>` prompts. A command mentioned later in an ordinary note cannot reclassify that note. Executable names are case-sensitive; known commands include `rustup`, `conda`, `mkdir`, `sudo`, `echo`, and `make`/`just` target lists. `git` requires a known subcommand (including `blame`, `rev-parse`, and `submodule`) unless shell syntax makes the command explicit. Determiners, pronouns, prepositions, and verb inflection keep sentence-shaped `make`/`just` prose in Documents.

Environment-prefixed commands and unknown lowercase executables with flags or path arguments are excluded from prose when they have command structure. Notes mentioning options or paths retain prose classification.

Setup commands require specific arguments:

- `export`: leading options or assignment operands.
- `set`: an option, an assignment (including CMD names/values with spaces and quoted assignments), or a single-variable query.
- `source`: a path, including a quoted path with spaces.
- `cd`: leading options, a path, a quoted path, or a CMD drive-switch/path form.

Closed leading quoted arguments and shell control operators after command-shaped arguments establish setup commands; punctuation in a prose URL does not. Compact bare setup arguments may introduce a sequence only when immediately followed by a genuine command after optional blanks/comments. Thus `cd repo` or bare `cd Program Files` needs that following command. Prose copulas/prepositions, digits, incidental apostrophes, and slashes do not establish a sequence.

### Logs And Delimited Data

Delimited records require consistent field counts. Comma/semicolon rows also need compact/quoted fields or numeric data, so comma-heavy sentences remain prose. A stored TSV label protects a one-row header from log-level interpretation.

Uppercase spaced levels and lower/uppercase levels with colons or brackets are single-line log signals. Lowercase spaced prose, shell function definitions, and tabular headers are excluded. Two leading strong machine-level rows establish a log even when tab-delimited. Repeated lowercase spaced levels also establish a log after a Yarn command or when every message has machine-style capitalization. An optional context such as `INFO (main)` is part of the header; sentence-style headings such as `Warning:` need other log evidence.

A Rust panic needs a leading runtime header or Cargo run preamble. Runtime rows/panic headers override incidental Code/Config labels, including locked labels, without changing language or lock fields. Derived Logs remain in Logs rather than Code/Config. Config handles take precedence over ambiguous colon-prefixed levels; assignments (`INFO = value`), bare TOML headers (`[INFO]`), and genuine Dockerfile instructions remain code/config.

### Sampling And Stored Projections

Line-based semantic and shell-sequence samples retain complete LF/CRLF records within 64 KiB. A final row cut mid-record is discarded when an earlier complete row exists. If no complete row fits, the UTF-8-safe bounded prefix remains available for prose and handles. This prevents a cut command, CSV/TSV record, or log row from changing sampled structure while allowing long single-line prose.

JSONL detection parses every complete record beginning within the 64 KiB sample. A cut final record is finished only when that individual record also fits 64 KiB; larger records remain outside JSONL classification. Complete records at the boundary and CRLF input remain eligible.

Derived kinds are rebuilt through the [storage projection repair policy](storage.md#compatibility-policy).

### Normalized Filters

Language filters normalize both stored metadata and the requested value, preserving interoperability across legacy/current labels (`csharp` and `cs`). Search ranking also checks normalized languages to retain metadata relevance as labels evolve.

## Text Export Extensions

[`preferred_extension`](../crates/localpaste_core/src/detection/extensions.rs) centrally maps recognized text formats to conventional extensions, independently of grammar support. CSV and TSV export as `.csv` and `.tsv`; JSON Lines keeps its `jsonl` label and exports as `.jsonl` while sharing JSON highlighting. Both Magika JSON labels are refined by checking for multiple valid line records. Document aliases resolve to `.md`, `.rst`, or `.tex`. Unsupported highlighting does not force `.txt`; unknown formats still use `.txt`. Export writes the current editor content unchanged.

## GUI Highlight Resolution

Markdown uses the project-owned [LocalPaste Markdown grammar](../crates/localpaste_gui/assets/LocalPaste-Markdown.sublime-syntax), with [list contexts added at load time](../crates/localpaste_gui/src/app/highlight/markdown.rs). Matching or longer delimiters close fences. Top-level closers allow at most three leading spaces and no quote/list prefix. In list continuations, that limit is relative to the item indentation; four additional spaces remain literal code. Bullet/nested/ordered lists support backtick and tilde fences. A quoted fence also ends when its blockquote ends, without consuming the next unquoted line.

Footnote markers end at the reference/definition boundary so body text remains readable. Escaped punctuation stays literal in prose; backslashes inside code do not escape delimiters. Inline-code spans can cross content lines within a paragraph or list item, but an unmatched span ends at a blank paragraph boundary or interrupting heading, rule, fence, or new list item. Structural rules accept LF and CRLF. Fenced bodies use one string color with no embedded-language highlighting; scope mappings use the theme's foreground, string, and keyword colors.

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
   Edit hints carry UTF-8 byte positions from the editor buffer; the worker counts preceding LF bytes off the UI thread to find the parser line. This avoids mixing Rope's CR/Unicode line indices with syntax-parser LF lines. A single-edit pass checks every touched line before reusing a matching tail; unchanged interior lines do not end a multiline edit. Other edits use hash alignment, retaining original line indices so deleting a line cannot reuse a suffix with the deleted line's parser state.
3. UI merges matching patches into staged/current highlight state.
4. Staged highlight applies:
   - immediately only when there is no current render,
   - otherwise only after idle threshold.

Current policy constants (virtual editor):

- idle apply threshold: `200ms`
- adaptive debounce windows:
  - tiny edits below 64 KiB (`<=4` changed chars, `<=2` touched lines): `15ms`
  - medium edits: `35ms`
  - larger supported buffers (`>=64 KiB`): `50ms`
- plain rendering threshold: `>=256 KiB` content

Primary implementation:

- request/stage/apply lifecycle: [`../crates/localpaste_gui/src/app/highlight_flow.rs`](../crates/localpaste_gui/src/app/highlight_flow.rs)
- virtual edit hint capture: [`../crates/localpaste_gui/src/app/virtual_ops_apply.rs`](../crates/localpaste_gui/src/app/virtual_ops_apply.rs)
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

Magika's `gitattributes` label also requires attribute-shaped content: a path pattern with attribute tokens, or recognized attribute assignments. Short clipboard prose does not acquire that label merely because it contains whitespace-separated words. Manual language values remain unchanged by this guard. Automatic Batch labels are rejected for prose starting with `set`, `export`, or `source` when their arguments lack the [setup-command structure](#commands). Script headers/comments keep their existing behavior; stored Batch-labelled setup prose derives Document without changing its language or manual/locked state.

## Validation Targets

When touching detection/highlight behavior, validate:

- core detection tests (`localpaste_core::detection::tests`),
- GUI resolver/worker tests (`localpaste_gui::app::highlight::worker::resolver_tests`),
- GUI manual checks in [dev/gui-notes.md](dev/gui-notes.md),
- GUI perf checks in [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md).
