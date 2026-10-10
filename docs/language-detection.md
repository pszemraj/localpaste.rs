# Language Detection And Highlighting

Implementation roots:

- Core detection entrypoint: [`../crates/localpaste_core/src/detection/mod.rs`](../crates/localpaste_core/src/detection/mod.rs)
- Retrieval kinds, handles, and terms: [`../crates/localpaste_core/src/semantic.rs`](../crates/localpaste_core/src/semantic.rs)
- GUI highlight pipeline: [`../crates/localpaste_gui/src/app/highlight/mod.rs`](../crates/localpaste_gui/src/app/highlight/mod.rs), with its lifecycle described in [dev/highlighting.md](dev/highlighting.md)

## Feature Topology

- `localpaste_core` keeps `magika` as opt-in (`default = []`).
- `localpaste_gui` and `localpaste_server` enable `magika` by default.
- `localpaste_cli` sends content to the API; detection runs in the receiving GUI/server, not the CLI.

Magika builds download ONNX Runtime binaries. To use structural and heuristic detection without that dependency, disable the binary crate's default features:

```bash
cargo run -p localpaste_gui --bin localpaste-gui --no-default-features
# Or run the headless server with the same fallback:
cargo run -p localpaste_server --bin localpaste --no-default-features
```

## Detection Flow

For auto-detected language (`language_is_manual == false`), detection tries cheap structural checks first, then the Magika model, then heuristics:

1. Structural overrides run before any model inference: a paste that is a single standalone Markdown fence, a source shebang, Python compound statements, Makefiles, shell command sequences, and runtime output such as Rust panics and Python tracebacks.
2. With the `magika` feature, Magika classifies the body. CRLF is normalized to LF for inference only; stored content is unchanged. Non-text results and generic labels (`txt`, `randomtxt`, `unknown`, `empty`, `undefined`) are discarded. Any other label is normalized and [refined](#label-refinement), and a label that survives and is not `text` is the result.
3. Otherwise (Magika disabled, unavailable, failed, generic, or rejected by refinement), the heuristic fallback runs. Its label is normalized and returned unless it is empty or `text`.

Auto mode is a pending state rather than a continuously updated label. Switching a paste to Auto clears its resolved language, and the next content edit runs detection. Once detection resolves a concrete language, the paste is locked to it (`language_is_manual = true`) until it is switched back to Auto. API create requests that omit `language_is_manual` detect immediately and lock when detection resolves; a request with `language_is_manual: false` and no `language` starts unresolved and defers detection to a later edit. A manual language is never re-detected on content edits.

Structural recognition is conservative where shapes overlap. A colon followed by indented lines can be a Makefile target, a Python compound statement, or a note, so each is accepted only when the whole sampled body supports it.

A `target:` line followed by tab-indented lines is claimed as a Makefile before model inference only when every indented line looks like a command: it names a known tool, uses a flag, path, variable, or shell operator, or it is a lowercase tool name followed by plain arguments, such as `zig build`. Makefiles for unfamiliar build tools therefore qualify, while stack traces, assembly, logs, and other tab-indented code keep their detected language. Distinctive Make syntax is enough on its own: continued lines, `override` assignments, `vpath`, Make function calls, `.mk` includes, and closed `define` blocks.

Two document shapes look like Makefiles but are not. A note such as `Agenda:` followed by indented sentences or short items resolves to Markdown, unless prerequisites, a second target, directives, or command syntax show a real build file. A README section keeps its document label when a capitalized label such as `Install:` sits under a Markdown heading, or when its only label is `Example:` or `Steps:`. When Magika itself reports a Makefile, only such a complete note changes the label; unfamiliar syntax alone does not.

Python compound statements (`if`, `for`, or `def` headers followed by indented bodies) can resemble targets with tabbed recipes as well. A body keeps its Python label while its sampled lines are consistently Python-shaped; unrelated targets, Make directives, unquoted shell variables, or command recipes anywhere in the sample disqualify it. An explicit source shebang takes precedence.

Note-shaped bodies also correct the retrieval kind. A paste stored as Shell or Makefile whose body is a note, or a prose sentence stored as JavaScript only because it contains `let` and `function`, derives the Document kind. The stored language and lock are untouched, so highlighting and export keep that choice; switching to Auto clears the label, and the next edit detects it again.

Magika's model loads lazily on first use and is shared by all callers. The GUI backend worker and the server call `prewarm()` at startup so the first save does not pay the load cost. If the model cannot initialize or an inference fails, detection falls back to the heuristics.

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

Unknown values pass through in lowercase. `jsonl` stays distinct from `json`: the two share a highlighter but not a stored format, an export extension, or a language filter. The complete alias table is `canonicalize` in [`detection/canonical.rs`](../crates/localpaste_core/src/detection/canonical.rs).

Manual language picker values are defined centrally in `MANUAL_LANGUAGE_OPTIONS` and stored as normalized values.

## Filter And Search Semantics

The language label controls highlighting and export. Metadata search and the sidebar smart collections rely on a separate derived classification: each paste gets a kind (Document, Code, Config, Log, Link, or Other), a short handle, and a few search terms. They are computed locally from the content and the stored language, persisted with the paste, and never change the language or its lock. The rules live in [`semantic.rs`](../crates/localpaste_core/src/semantic.rs) and its `semantic/` submodules; derived values are rebuilt through the [storage projection repair policy](storage.md#compatibility-policy) when the rules change.

Two principles run through the rules. The first is that the whole body outranks a single label. Auto-detection locks the labels it infers, so a stored label cannot be told apart from a user's choice, and a wrong one would misfile the paste: a log stored as `dockerfile`, or a shell session stored as `markdown`. Structural evidence in the body, such as a runtime header, a command sequence, or Python source, therefore wins. The second is that prose outweighs weak command evidence. A sentence does not become a command because it starts with `set` or mentions `--release`; a command needs a recognized executable with command-shaped arguments such as options, paths, quoting, or shell operators, and it must lead the paste.

The kinds:

- **Document.** Markdown, reStructuredText, LaTeX, and unlabeled prose, meaning several words that are mostly letters with little punctuation. A Markdown label alone is not enough; the body must read as a document. A document can quote code, commands, or log lines without changing kind: `The --release option optimizes the build.` is a Document, and so is a Markdown note headed `# Log notes` that quotes an `[INFO]` line. A paste that is a single fenced block is classified by the fence instead of the Markdown wrapper: a `python` or `sh` fence is Code, a `json` fence is Config, and a `text` fence holding a traceback is a Log. A fence surrounded by prose, or one that wraps Markdown, stays a Document.
- **Code.** Source in a recognized programming language, and bodies that lead with a command. `git rev-parse HEAD` and `cd repo && git status` are Code, with handles such as `git rev-parse`; a note that mentions `git` halfway through is not. Source files also get definition handles such as `fn handle_request`.
- **Config.** JSON, JSONL, YAML, TOML, XML, Dockerfile, and Makefile content, plus unlabeled `key: value` bodies using well-known keys such as `name`, `model`, or `image`. A YAML paste containing `model: gpt-4` is Config with the handle `model gpt-4`.
- **Log.** Runtime output: level-prefixed lines such as `[INFO] Server started` or `INFO Starting the server`, Python tracebacks, and Rust panic headers. Runtime structure overrides an incidental Code or Config label, including a locked one, but a panic header must lead the paste, so source that contains one later stays Code.
- **Link.** A body that is a single URL. `https://example.com/docs` is a Link with the handle `example.com`.
- **Other.** Everything the rules above do not claim: single words such as `hi`, hexadecimal blobs, and delimited tables such as TSV.

The sidebar collections Documents, Code, Config, Logs, and Links filter on these kinds. Language, title, and tags can also place a paste in Code, Config, Logs, or Links, so a paste named `deploy.log` appears under Logs even when its body is unremarkable, and a paste of kind Other appears in none of these collections unless that metadata points to one. Document-kind pastes accept only explicit signals (a file suffix, a language label, or a whole-word tag), so a note titled `holographic otter` is not mistaken for a log.

Classification reads a bounded leading sample of the body, ending on a line boundary so a partly included row cannot change the result. Very large pastes are therefore classified from their opening content.

Language filters and metadata search normalize both the stored and the requested value with the [same rules](#normalization-contract), so a filter of `cs` also lists pastes stored as `csharp`.

## Text Export Extensions

[`preferred_extension`](../crates/localpaste_core/src/detection/extensions.rs) maps each recognized text format to its conventional extension, independently of highlighting grammar support: `zig` exports as `.zig` even though it renders as plain text. CSV and TSV export as `.csv` and `.tsv`. JSON Lines exports as `.jsonl` and shares JSON highlighting; because Magika reports both JSON and JSON Lines under either of its JSON labels, detection chooses `jsonl` when the body holds multiple valid line records. Markdown, reStructuredText, and LaTeX export as `.md`, `.rst`, and `.tex`. Unknown formats use `.txt`. Export writes the current editor content unchanged.

## GUI Highlight Resolution

The editor converts a stored language label into a syntect grammar with `resolve_syntax`. The label is normalized first, so `bash` and `sh` both reach `shell`, and `jsonl` borrows the JSON grammar while remaining JSONL in storage. `markdown` resolves to the project-owned [LocalPaste Markdown grammar](../crates/localpaste_gui/assets/LocalPaste-Markdown.sublime-syntax), in which fenced code is drawn in a single code color rather than highlighted in the fence's language, and `text` and its aliases resolve to plain text. Any other label is tried in this order:

1. exact syntax name
2. exact extension
3. case-insensitive name
4. normalized-name match (alphanumeric only)
5. case-insensitive extension scan
6. explicit fallback candidates for known mismatches
7. plain text

The fallback table is deliberately small. A few high-priority labels borrow a nearby grammar (for example `typescript`, `toml`, and `powershell`), but labels without a bundled grammar, such as `zig`, `kotlin`, and `dart`, render as plain text rather than with misleading tokenization. Their metadata labels stay visible in filters and export.

The fallback candidates live in `syntax_fallback_candidates` in [`syntax.rs`](../crates/localpaste_gui/src/app/highlight/syntax.rs), and `resolver_tests` in [`worker.rs`](../crates/localpaste_gui/src/app/highlight/worker.rs) cover both the borrowed-grammar and the plain-render cases.

## Virtual Editor Async Highlight Flow

Highlighting runs on a background thread. While the user types, the previous colors stay in place, and a new result replaces them only after a short pause in editing, so colors do not flicker mid-burst. Buffers of 256 KiB or more (`HIGHLIGHT_PLAIN_THRESHOLD` in [`app/mod.rs`](../crates/localpaste_gui/src/app/mod.rs)) are not highlighted at all: the editor draws them as plain text, and the sidebar and paste picker show their language as `plain`. The stored language is unchanged.

Request, debounce, and staging mechanics are described in [dev/highlighting.md](dev/highlighting.md).

## Runtime Provider Default (Magika)

When Magika is enabled, ONNX Runtime defaults to the CPU execution provider. The environment variable `MAGIKA_FORCE_CPU` controls this and defaults to `true`; a falsey value (`0`, `false`, `no`, `off`) allows runtime/provider defaults.

Reference: [`../.env.example`](../.env.example)

## Label Refinement

Magika also reports short notes, headers, and prose as structured formats, so its label is checked against the body before it is accepted. A rejected label falls through to the heuristics. The checks are implemented in `refine_magika_label` in [`detection/mod.rs`](../crates/localpaste_core/src/detection/mod.rs):

- Markdown: a label on a body that is wholly commands, runtime output, or Python source is replaced by the matching language. A standalone fence is always Markdown.
- Python: dropped when a Markdown document with surrounding prose wraps the snippet.
- YAML: accepted only with YAML-distinctive structure (see below).
- SCSS: relabeled CSS when the body has no SCSS-specific syntax.
- gitattributes: requires a path pattern with attribute tokens or recognized attribute assignments, so short clipboard prose does not qualify.
- Batch: rejected for prose such as a sentence beginning with `set`, `export`, or `source` whose arguments lack setup-command structure (an assignment, option, or path operand; see `setup_command_is_valid`), or one with `in`, `at`, or `by` before a later path. A stored Batch label on such prose derives the Document kind.
- JSON and JSON Lines: split by checking for multiple valid line records.

These refinements apply during detection only; manual stored language values are never rewritten.

A mapping-heavy sample must show YAML-distinctive structure before it is accepted as YAML. Single-line and prose-like flat mappings stay ambiguous because they also match notes, logs, and email or HTTP headers. YAML is accepted when the sample has a document marker, nested indentation, flow collections, block scalars, anchors, structured sequence items, or multiple config-shaped flat mapping lines.

## Validation Targets

When touching detection/highlight behavior, validate:

- core detection and semantic tests (`localpaste_core::detection::tests`, `localpaste_core::semantic::tests`),
- GUI resolver/worker tests (`localpaste_gui::app::highlight::worker::resolver_tests`),
- GUI highlight flow tests (`localpaste_gui::app::tests::highlight_behaviors`),
- GUI manual checks in [dev/gui-notes.md](dev/gui-notes.md),
- GUI perf checks in [dev/gui-perf-protocol.md](dev/gui-perf-protocol.md).
