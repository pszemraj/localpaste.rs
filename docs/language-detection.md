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

An inferred Markdown label is refined when the entire sampled body has strong technical structure: recognized shell commands, runtime log rows and wrappers, or comment-prefixed Python imports and source lines. Comment-prefixed commands with an environment assignment or executable flags/path syntax also reject the weak Markdown label without assigning a new language. Ordinary Markdown prose, links, and embedded fenced examples retain their document behavior. Other statistical language labels are unchanged by this Markdown refinement.

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

Retrieval kinds also apply this whole-body technical exception to stored Markdown labels. Automatically detected labels become locked, so the stored lock flag cannot distinguish them from user-selected labels. A command-only paste with explicitly selected Markdown therefore receives the same content-derived Code/Other kind as identical automatically labelled content; raw runtime output receives Log. This preserves the stored highlighting language and lock flag. Documentary headings or blockquotes with surrounding prose and embedded snippets remain Documents, as do reStructuredText and LaTeX labels. Empty or whitespace-only bodies with document-language labels also derive Document, with no handle or terms; other empty bodies derive Other. The Documents collection uses the derived Document kind so a Markdown highlighting label does not return technical Other content to that collection.

Line-based semantic and shell-sequence samples are bounded to 64 KiB and retain complete LF/CRLF records: a final row cut by the byte cap is discarded when an earlier complete row exists. When no complete row fits, the UTF-8-safe bounded prefix remains available for prose and handles. This keeps long single-line prose readable while preventing a truncated command, CSV/TSV record, or log row from changing the sampled structure. JSONL retains its separate independently valid-record detection policy.

Leading shell comments and `$`, `%`, or `>` command prompts are handled before extracting known command handles. `rustup` is included in the known executable vocabulary. Environment-prefixed commands and unknown lowercase executables with flags or path arguments are excluded from prose when they have command structure; ordinary notes mentioning options or paths retain the existing prose classification.

Language filter matching normalizes both:

- stored language metadata,
- incoming filter value.

This preserves interoperability across legacy and current labels (for example, `csharp` and `cs`).

Search ranking also checks normalized language values to avoid losing metadata relevance as stored labels evolve.

The Documents collection includes Markdown, reStructuredText, LaTeX, and prose notes without stronger code/config/log/link signals. Document languages take precedence over embedded code and misleading titles/tags. A paste consisting entirely of one fenced block is classified by its info word or body: Python/shell fences belong to Code, JSON/YAML fences to Config, and structurally recognized log output belongs to Logs even in a `text` fence. Other unlabeled prose fences and explicit `text` fences stay in Documents; unlabeled bodies use structural heuristics without statistical inference or a model lock, and structural data that remains `Other` does not become prose merely because it is fenced. The Markdown language label remains intact for highlighting and export. Fences embedded in surrounding prose, and fences explicitly containing a document language, remain Documents. A fallback prose classification yields to explicit filename suffixes, lowercase executable prefixes, URL-shaped names, languages, and whole-word tags, such as `deploy.log` or a `logs` tag. Title fragments such as `log` in `holographic` and `script` in `transcript` do not exclude prose. Ambiguous `make` and `just` title prefixes need a command-derived kind to exclude a document.

Untyped prose needs at least three whitespace-separated words, at least 70% letters and at most 20% symbols among non-whitespace characters. Single tokens, hexadecimal blobs, and recognized commands stay out of Documents. Delimited records need consistent field counts; comma/semicolon rows also need compact or quoted fields, or numeric data, so comma-heavy sentences remain prose. A stored TSV label also protects a one-row tab-separated header from log-level interpretation. Two leading strong machine-level rows establish a log even when consistently tab-delimited. Repeated lowercase spaced levels also establish a log when introduced by a Yarn command or when each message has machine-style capitalization; this keeps lowercase sentence-shaped `info`/`error` prose in Documents. An optional context such as `INFO (main)` is part of the header. Config handles take precedence over ambiguous colon-prefixed levels. Uppercase spaced levels and lower/uppercase levels with colons or brackets are single-line log signals; lowercase spaced prose, shell function definitions, and tabular headers are excluded. A Rust panic requires a leading runtime header or Cargo run preamble; sentence-style headings such as `Warning:` need other log evidence. Spaced/bracketed runtime messages and Rust panic headers take precedence over incidental detected Code/Config labels, including labels already locked onto stored pastes. This changes retrieval kind without changing the stored language or its manual/locked state. Assignments such as `INFO = value`, bare TOML headers such as `[INFO]`, and genuine Dockerfile instructions remain code/config. A derived Log appears in Logs rather than being returned to Code/Config by its stale language label. Executable names are case-sensitive. Leading setup commands (`cd`, `export`, `source`, and `set`) require command-specific argument structure: `export` uses leading options or assignment operands, `set` uses an option, an assignment (including CMD variable names with spaces), or a single variable query, `source` starts with a path (including a quoted path containing spaces), and `cd` takes leading options, one path, a quoted path, or a CMD drive-switch/path form. Closed leading quoted arguments and shell control operators after command-shaped arguments establish setup commands; punctuation inside a prose URL does not. Compact bare setup arguments may introduce a sequence when immediately followed by a genuine command; prose copulas and prepositions keep sentence-shaped notes out of those sequences. Digits or incidental apostrophes/slashes in prose do not establish setup commands. A bare `cd repo` needs an immediately following command after optional blank lines/comments; a command mentioned later in a note cannot establish the sequence. Meanwhile, `conda`, `mkdir`, `sudo`, `echo`, and `make`/`just` target lists are recognized directly. Determiners, pronouns, prepositions, and verb inflection keep sentence-shaped `make`/`just` prose in Documents, and `git` requires a known subcommand (including `blame`, `rev-parse`, and `submodule`) unless shell syntax makes the command explicit. Command handles use the first command after optional leading shell comments, so a command name mentioned later in an ordinary note cannot reclassify the whole note. Derived kinds are rebuilt through the [storage projection repair policy](storage.md#compatibility-policy).

JSONL detection parses every complete record that begins within the 64 KiB sample. If the boundary cuts the last such record, detection finishes it only when that individual record also fits 64 KiB; larger records stay outside JSONL classification. Complete records at the boundary and CRLF input remain eligible.

## Text Export Extensions

[`preferred_extension`](../crates/localpaste_core/src/detection/extensions.rs) centrally maps recognized text formats to conventional extensions, independently of grammar support. CSV and TSV export as `.csv` and `.tsv`; JSON Lines keeps its `jsonl` label and exports as `.jsonl` while sharing JSON highlighting. Both Magika JSON labels are refined by checking for multiple valid line records. Document aliases resolve to `.md`, `.rst`, or `.tex`. Unsupported highlighting does not force `.txt`; unknown formats still use `.txt`. Export writes the current editor content unchanged.

## GUI Highlight Resolution

Markdown uses the project-owned [LocalPaste Markdown grammar](../crates/localpaste_gui/assets/LocalPaste-Markdown.sublime-syntax). Footnote markers end at the reference/definition boundary so body text remains readable. Indented, list, and blockquote fence prefixes are recognized; matching or longer closing fences terminate the block. Top-level closers allow at most three spaces of indentation and no quote/list prefix. A quoted fence also ends when its containing blockquote ends, without consuming the following unquoted line. Escaped punctuation stays literal in prose; backslashes inside code do not escape delimiters. Inline-code spans retain their matching delimiter across content lines within a paragraph or list item, but an unmatched span ends at a blank paragraph boundary or an interrupting heading, rule, fence, or new list item. Structural rules accept LF and CRLF line endings. Fenced bodies use one existing string color, with no embedded-language highlighting. Scope mappings use the current theme's foreground, string, and keyword colors.

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
   A single-edit pass checks every touched line before reusing a matching tail; unchanged interior lines do not end a multiline edit. Hash alignment retains original line indices so deleting a line cannot reuse a suffix with the deleted line's parser state. Bare CR and Unicode line separators use hash alignment because editor and syntax-parser line indices differ.
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

Magika's `gitattributes` label also requires attribute-shaped content: a path pattern with attribute tokens, or recognized attribute assignments. Short clipboard prose does not acquire that label merely because it contains whitespace-separated words. Manual language values remain unchanged by this guard. Likewise, automatic Batch labels are rejected for prose starting with `set`, `export`, or `source` when their arguments lack setup-command structure. Quoted and unquoted CMD assignments may contain spaces before or after `=` without losing Code membership. Script headers/comments keep their existing behavior; slash-prefixed `set` options and CMD `cd` drive-switch/path forms retain Code membership. A bare unquoted `cd Program Files` remains ambiguous unless followed immediately by a command. Stored Batch-labelled setup prose also belongs to Documents through structural derivation without changing its language or manual/locked state; other stored language precedence stays intact.

## Validation Targets

When touching detection/highlight behavior, validate:

- core detection tests (`localpaste_core::detection::tests`),
- GUI resolver/worker tests (`localpaste_gui::app::highlight::worker::resolver_tests`),
- GUI manual checks in [dev/gui-notes.md](dev/gui-notes.md),
- GUI perf checks in [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md).
