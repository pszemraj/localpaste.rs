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

1. Structural overrides run before any model inference: a paste that is a single standalone Markdown fence, a source shebang, Python compound statements, Makefiles, shell command sequences, and runtime output labeled `log`: a run of two or more level-prefixed lines, timestamped lines, a Rust panic, a Python traceback, a Java stack trace, or a rustc diagnostic, as described under the Log kind below.
2. With the `magika` feature, Magika classifies the body. CRLF is normalized to LF for inference only; stored content is unchanged. Non-text results and generic labels (`txt`, `randomtxt`, `unknown`, `empty`, `undefined`) are discarded. Any other label is normalized and [refined](#label-refinement), and a label that survives and is not `text` is the result.
3. Otherwise (Magika disabled, unavailable, failed, generic, or rejected by refinement), the heuristic fallback runs. Its label is normalized and returned unless it is empty or `text`.

Auto mode is a pending state rather than a continuously updated label. Switching a paste to Auto clears its resolved language, and the next content edit runs detection. Once detection resolves a concrete language, the paste is locked to it (`language_is_manual = true`) until it is switched back to Auto. API create requests that omit `language_is_manual` detect immediately and lock when detection resolves; a request with `language_is_manual: false` and no `language` starts unresolved and defers detection to a later edit. A manual language is never re-detected on content edits.

Structural recognition is conservative where shapes overlap. A colon followed by indented lines can be a Makefile target, a Python compound statement, or a note, so each is accepted only when the whole sampled body supports it.

A `target:` line followed by tab-indented lines is claimed as a Makefile before model inference only when every indented line looks like a command: it names a known tool, uses a flag, path, variable, or shell operator, or it is a lowercase tool name followed by plain arguments, such as `zig build`. A recipe's `echo` line counts even when it prints a sentence, such as `@echo Building the app`. Makefiles for unfamiliar build tools therefore qualify, while stack traces, assembly, logs, and other tab-indented code keep their detected language. Distinctive Make syntax is enough on its own: continued lines, `override` assignments, `vpath`, Make function calls, `.mk` includes, and closed `define` blocks.

Three document shapes look like Makefiles but are not. A chat transcript or labeled notes such as `ana: the deploy failed again` / `bob: which step?` have only `name: text` lines with no recipe, and the text after every colon reads as English by the prose-grammar rule below, while Make prerequisites are names; Magika's Makefile label is discarded for such a body. A note such as `Agenda:` followed by indented sentences or short items resolves to Markdown, unless prerequisites, a second target, directives, or command syntax show a real build file. A README section keeps its document label when a capitalized label such as `Install:` sits under a Markdown heading, or when its only label is `Example:` or `Steps:`. When Magika itself reports a Makefile, only one of these complete document shapes changes the label; unfamiliar syntax alone does not.

Python compound statements (`if`, `for`, or `def` headers followed by indented bodies) can resemble targets with tabbed recipes as well. A body keeps its Python label while its sampled lines are consistently Python-shaped; unrelated targets, Make directives, unquoted shell variables, or command recipes anywhere in the sample disqualify it. An explicit source shebang takes precedence.

Note-shaped bodies also correct the retrieval kind. A paste stored as Shell or Makefile whose body is a note, a Makefile-labeled chat transcript, or a prose sentence stored as JavaScript only because it contains `let` and `function`, derives the Document kind. The stored language and lock are untouched, so highlighting and export keep that choice; switching to Auto clears the label, and the next edit detects it again.

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

Two principles run through the rules. The first is that the whole body outranks a single label. Auto-detection locks the labels it infers, so a stored label cannot be told apart from a user's choice, and a wrong one would misfile the paste: a log stored as `dockerfile`, or a shell session stored as `markdown`. Structural evidence in the body, such as a runtime header, a command sequence, or Python source, therefore wins. The second is that prose outweighs command evidence. A command must lead the paste with a recognized executable, such as `git`, `cargo`, `npm`, `grep`, `rsync`, or `ffmpeg` (the `COMMANDS` list in `crates/localpaste_core/src/semantic/commands.rs` covers build, file, text, archive, and cloud tools, but omits words that routinely open English sentences, such as `find`, `cat`, `head`, `which`, `open`, `sort`, and `touch`), or the setup builtins `cd`, `export`, `source`, and `set`, and none of its unquoted words may read as English: a function word (an article, pronoun, preposition, auxiliary, conjunction, or question word such as `the`, `my`, `on`, `is`, or `why`), a contraction such as `doesn't`, or sentence-ending punctuation attached to a word. Options, paths, numbers, and operators never outweigh that evidence, because sentences mention them too, while quoted text is skipped, so `echo 'this is a test'` stays a command. A `git` line must name one of Git's own subcommands, and a setup builtin with plain operands such as `cd repo` counts only when a command follows it. A body that continues with a prose sentence after its leading command is a note quoting that command, unless a `$ ` or `% ` prompt marks it as a terminal transcript.

The kinds:

- **Document.** Markdown, reStructuredText, LaTeX, and unlabeled prose, meaning several words that are mostly letters with little punctuation. A Markdown label alone is not enough; the body must read as a document. A document can quote code, commands, or log lines without changing kind: `The --release option optimizes the build.` is a Document, and so is a Markdown note headed `# Log notes` that quotes an `[INFO]` line. A paste that is a single fenced block is classified by the fence instead of the Markdown wrapper: a `python` or `sh` fence is Code, a `json` fence is Config, a `text` fence holding log output is a Log, and a `text` fence holding prose is a Document. A fence surrounded by prose, or one that wraps Markdown, stays a Document.
- **Code.** Source in a recognized programming language, and bodies that lead with a command. `git rev-parse HEAD` and `cd repo && git status` are Code, with handles such as `git rev-parse`; a note that mentions `git` halfway through is not, and neither is `cargo build takes forever on my laptop` or a commit message whose subject reads like a command but whose body is prose. Source files also get definition handles such as `fn handle_request`.
- **Config.** JSON, JSONL, YAML, TOML, XML, Dockerfile, and Makefile content, plus unlabeled `key: value` bodies using well-known keys such as `name`, `model`, or `image`. A YAML paste containing `model: gpt-4` is Config with the handle `model gpt-4`.
- **Log.** Runtime output, recognized by line structure rather than vocabulary, so a note that mentions a panic, a stack trace, or an error budget stays a Document. The recognized shapes are:
  - Timestamped lines. A line counts when it starts with a timestamp that includes seconds: a date and time such as `2026-10-10T06:00:01Z`, `2026-10-10 06:00:01.123`, or `2026/10/10 06:00:01+02:00`, with an optional fraction and zone and optionally in brackets, or syslog's `Oct 10 06:00:01`. At least half of the unindented lines must start with one, so indented stack frames do not count against a log, and a single stamped line needs a spaced uppercase or bracketed level after the timestamp, such as ` INFO ` or `[INFO]`. A comma or tab right after the timestamp marks a record field, so a CSV or TSV whose first column is a time stays Other, and a date without seconds is not a timestamp.
  - Level-prefixed lines such as `[INFO] Server started`, `INFO Starting the server`, `error: unable to open database`, `INFO:root:Starting`, npm's `npm ERR!` and `npm WARN` lines, and yarn output. Lowercase level words are also ordinary English (`debug Safari crash on login`) and YAML keys (`info: v1.2.3`), so two or more lowercase levels at the start of a paste need a `yarn` banner, a `[1/4]` progress step, or a path, version, quote, ellipsis, or similar machine token in some message. Without one, the lines are a to-do list, glossary, or mapping, and a YAML or other config label wins. A `key: value` line whose value is a single token or one quoted string is a mapping entry and gives no such evidence, so `error: /var/log/error.log` followed by `info: /var/log/info.log` stays a mapping.
  - Rust panics, alone or after Cargo's status lines (`Compiling demo v0.1.0`, `Updating crates.io index`); rustc diagnostics such as `error[E0308]: mismatched types` followed by a `-->` location; Python tracebacks; and Java stack traces with an `at Class.method(File.java:12)` frame.

  Runtime structure overrides an incidental Code, Config, or Markdown label, including a locked one, but a panic header must lead the paste, so source that contains one later stays Code.
- **Link.** A body that is a single URL. `https://example.com/docs` is a Link with the handle `example.com`.
- **Other.** Everything the rules above do not claim: single words such as `hi`, hexadecimal blobs, and delimited tables such as TSV.

The sidebar collections Documents, Code, Config, Logs, and Links filter on these kinds. Language, title, and tags can also place a paste in Code, Config, Logs, or Links, so a paste named `deploy.log` appears under Logs even when its body is unremarkable, and a paste of kind Other appears in none of these collections unless that metadata points to one. Document-kind pastes accept only explicit signals (a file suffix, a language label, or a whole-word tag), so a note titled `holographic otter` is not mistaken for a log.

Classification reads a bounded leading sample of the body, ending on a line boundary so a partly included row cannot change the result. Very large pastes are therefore classified from their opening content.

Handles are short labels for the sidebar tooltip and metadata search. The first rule that applies wins: a definition in a recognized language (`fn handle_request`), the leading command with its first operand (`git rev-parse`, `cd ~/projects/app`), a well-known config key with its value (`model gpt-4`, `dataset /data/train/v2`), the host of a single URL (`example.com`), and otherwise the first two search terms. Handles are capped at 48 characters (`MAX_HANDLE_CHARS`). Command operands and config values are copied from the paste, never edited: the text is read up to the first character that is not a letter, digit, or one of `_ - . : / \ ~ @ + ,`, so a path keeps its slashes and `ls *.txt` gets the handle `ls`, not a string that never appeared in the paste. `=` ends the text as well, because what follows it is an assigned value that is often a secret: `export API_KEY=...` gets the handle `export API_KEY`, and `name: token=...` gets `name token`. Surrounding quotes and a trailing comma are dropped. A command operand is a single whitespace-delimited word, but a config value may hold several, so it also accepts spaces, which collapse to one: `name: My Project` gets `name My Project`, and `name: web # prod` gets `name web`. Both are capped at 24 characters (`MAX_HANDLE_ATOM_CHARS`) by keeping their start, and a config value keeps whole words under the cap, cutting mid-word only when its first word alone is longer. A config key must match a well-known key exactly, so `service is down: restart it` is not a config entry.

Search terms come from the same sample: up to four word-like tokens, preferring repeated and identifier-shaped ones. Punctuation is trimmed from both ends of each token before it is judged: leading and trailing `.`, `:`, and `-` are removed, so `deploy.`, `name:`, and `--release` count as `deploy`, `name`, and `release`, while internal punctuation stays (`localpaste.rs`, `127.0.0.1`, `hash_map::entry`, `kebab-case`). Underscores are not trimmed because they belong to identifiers such as `__init__` and `_private`. The URL scheme words `http`, `https`, and `www`, like other stopwords, never become terms. After trimming, a token must be 3 to 48 characters long (`MAX_TERM_CHARS` in `semantic.rs`, matching the 48-character handle cap) to qualify. A longer token, such as a minified line or a base64 blob, is dropped rather than truncated, because a prefix of it is not a meaningful term; a handle synthesized from terms therefore cannot be built from one either. Body search still finds such strings. Realistic identifiers such as `std::collections::hash_map::entry` or `HIGHLIGHT_DEBOUNCE_LARGE_BYTES` fit well within the cap.

Language filters and metadata search normalize both the stored and the requested value with the [same rules](#normalization-contract), so a filter of `cs` also lists pastes stored as `csharp`.

## Text Export Extensions

[`preferred_extension`](../crates/localpaste_core/src/detection/extensions.rs) maps each recognized text format to its conventional extension, independently of highlighting grammar support: `zig` exports as `.zig` even though it renders as plain text. CSV and TSV export as `.csv` and `.tsv`. JSON Lines exports as `.jsonl` and shares JSON highlighting; because Magika reports both JSON and JSON Lines under either of its JSON labels, detection chooses `jsonl` when the body holds multiple valid line records. Markdown, reStructuredText, and LaTeX export as `.md`, `.rst`, and `.tex`. Unknown formats use `.txt`. Export writes the current editor content unchanged.

## GUI Highlight Resolution

![YAML, SQL, Markdown, and shell pastes highlighted in the editor, each with its detected language in the toolbar](../assets/screenshots/highlighting.png)

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
