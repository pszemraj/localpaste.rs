//! Lightweight locally-derived semantic metadata for retrieval.

use crate::detection::canonical::canonicalize;
use crate::text::{complete_line_prefix_by_bytes, TEXT_SAMPLE_MAX_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Shared command recognition and semantic-handle extraction.
pub(crate) mod commands;

const SAMPLE_MAX_LINES: usize = 256;
const MAX_TERMS: usize = 4;
const MAX_HANDLE_CHARS: usize = 48;

/// Coarse content class used by metadata-only retrieval and GUI filters.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PasteKind {
    #[default]
    Other,
    Code,
    Config,
    Log,
    Link,
    Document,
}

impl PasteKind {
    /// User-facing compact label for UI and search diagnostics.
    ///
    /// # Returns
    /// Stable short label suitable for read-only metadata display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Other => "Other",
            Self::Code => "Code",
            Self::Config => "Config",
            Self::Log => "Log",
            Self::Link => "Link",
            Self::Document => "Document",
        }
    }
}

/// Persisted retrieval hints derived from paste content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DerivedMeta {
    pub kind: PasteKind,
    pub handle: Option<String>,
    #[serde(default)]
    pub terms: Vec<String>,
}

/// Derive cheap structural retrieval hints from paste content.
///
/// # Arguments
/// - `content`: Paste body sampled for structural signals and technical terms.
/// - `language`: Optional stored language label used to bias classification.
///
/// # Returns
/// Persistable semantic retrieval metadata for metadata-only search and UI.
pub fn derive(content: &str, language: Option<&str>) -> DerivedMeta {
    let sample = sample_prefix(content);
    if sample.trim().is_empty() {
        return DerivedMeta {
            kind: if is_document_language(language) {
                PasteKind::Document
            } else {
                PasteKind::Other
            },
            ..DerivedMeta::default()
        };
    }

    let kind = classify_kind(content, language);
    let terms = extract_terms(sample, language);
    let handle = extract_definition_handle(sample, language)
        .or_else(|| commands::extract_command_handle(sample))
        .or_else(|| extract_config_handle(sample))
        .or_else(|| extract_url_handle(sample))
        .or_else(|| synthesize_handle_from_terms(&terms))
        .map(|value| truncate_chars(value.as_str(), MAX_HANDLE_CHARS));

    DerivedMeta {
        kind,
        handle,
        terms,
    }
}

fn sample_prefix(content: &str) -> &str {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return trimmed;
    }

    let prefix = complete_line_prefix_by_bytes(trimmed, TEXT_SAMPLE_MAX_BYTES);

    let mut line_end = prefix.len();
    let mut seen = 0usize;
    for (idx, ch) in prefix.char_indices() {
        if ch == '\n' {
            seen += 1;
            if seen >= SAMPLE_MAX_LINES {
                line_end = idx;
                break;
            }
        }
    }
    &prefix[..line_end]
}

/// Whether a stored language explicitly identifies a prose/document format.
///
/// # Returns
/// True for Markdown, reStructuredText, or LaTeX (including canonical aliases).
pub fn is_document_language(language: Option<&str>) -> bool {
    matches!(
        canonicalize(language.unwrap_or_default()).as_str(),
        "markdown" | "rst" | "latex"
    )
}

fn classify_kind(content: &str, language: Option<&str>) -> PasteKind {
    let sample = sample_prefix(content);
    let lang = canonicalize(language.unwrap_or_default().trim());
    let lower = sample.to_ascii_lowercase();

    // Highlight language describes the wrapper; retrieval kind describes what
    // the standalone fence contains. Prose plus fences remains a document.
    if lang == "markdown" {
        if let Some((info, body)) = crate::detection::standalone_fenced_block(content) {
            let inner_language = canonicalize(info);
            if inner_language == "text" {
                return match classify_kind(body, Some("text")) {
                    PasteKind::Log => PasteKind::Log,
                    _ => PasteKind::Document,
                };
            }
            if !is_document_language(Some(&inner_language)) {
                let detected = inner_language
                    .is_empty()
                    .then(|| crate::detection::detect_heuristically(body))
                    .flatten();
                // A Markdown body contains literal markup, including inner
                // fences; unwrapping those again changes its meaning and makes
                // nested fences rescan the whole body at every level.
                if detected.as_deref() == Some("markdown") {
                    return PasteKind::Document;
                }
                return match classify_kind(
                    body,
                    detected.as_deref().or(Some(inner_language.as_str())),
                ) {
                    PasteKind::Document if inner_language.is_empty() => PasteKind::Document,
                    PasteKind::Other if inner_language.is_empty() => PasteKind::Other,
                    PasteKind::Document | PasteKind::Other => PasteKind::Code,
                    kind => kind,
                };
            }
        }
        if let Some(technical_language) = markdown_technical_language(sample) {
            return classify_kind(sample, Some(technical_language));
        }
        if has_positive_markdown_document_evidence(sample) {
            return PasteKind::Document;
        }
    }

    if lang != "markdown" && is_document_language(language) {
        return PasteKind::Document;
    }

    if looks_like_single_url(sample) {
        return PasteKind::Link;
    }

    // Statistical detection can lock an incidental code/config label onto a
    // real log. Structural runtime headers still determine retrieval kind.
    // Attribute assignments and bare TOML table headers remain configuration.
    if looks_like_multiline_log(sample)
        || crate::detection::looks_like_rust_panic(sample)
        || crate::detection::looks_like_python_traceback(sample)
    {
        return PasteKind::Log;
    }
    let log_header = leading_log_header(sample, lang.as_str());
    if log_header.is_some_and(|header| header.strong_single_line) {
        return PasteKind::Log;
    }

    if matches!(
        lang.as_str(),
        "rust"
            | "python"
            | "javascript"
            | "typescript"
            | "go"
            | "java"
            | "kotlin"
            | "swift"
            | "ruby"
            | "php"
            | "c"
            | "cpp"
            | "cs"
            | "shell"
            | "powershell"
            | "sql"
            | "html"
            | "css"
            | "scss"
            | "sass"
            | "zig"
            | "lua"
            | "perl"
            | "elixir"
    ) {
        return PasteKind::Code;
    }

    if matches!(
        lang.as_str(),
        "json" | "jsonl" | "yaml" | "toml" | "xml" | "dockerfile" | "makefile"
    ) {
        return PasteKind::Config;
    }

    if lang == "log" {
        return PasteKind::Log;
    }

    let log_hits = [
        "traceback",
        "stack trace",
        "exception",
        "panic",
        "stderr",
        "stdout",
        "error:",
        "warn:",
        "info:",
        "caused by:",
        "exit code",
        "exit status",
        "segmentation fault",
        "cublas_status",
    ]
    .iter()
    .filter(|needle| lower.contains(**needle))
    .count();
    if log_hits >= 2 {
        return PasteKind::Log;
    }

    if extract_definition_handle(sample, language).is_some()
        || commands::extract_command_handle(sample).is_some()
    {
        return PasteKind::Code;
    }

    if extract_config_handle(sample).is_some() {
        return PasteKind::Config;
    }

    if log_header.is_some() {
        return PasteKind::Log;
    }

    if ((lang.is_empty() || lang == "text") && looks_like_prose(sample))
        || (lang == "batch" && looks_like_batch_prose(sample))
    {
        PasteKind::Document
    } else {
        PasteKind::Other
    }
}

/// Recognize a wholly technical body carrying an incidental Markdown label.
///
/// # Arguments
/// - `content`: Paste content to inspect within the normal semantic sample.
///
/// # Returns
/// A shell, log, Python, or generic text label for complete technical bodies, without
/// reclassifying documentary prose, links, or embedded fenced examples.
pub(crate) fn markdown_technical_language(content: &str) -> Option<&'static str> {
    let sample = sample_prefix(content);
    if crate::detection::looks_like_shell_command_sequence(sample)
        || looks_like_tabbed_command_recipe(sample)
    {
        return Some("shell");
    }
    if sample.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("```") || line.starts_with("~~~")
    }) {
        return None;
    }
    if crate::detection::looks_like_python_traceback(sample) {
        return Some("log");
    }
    if crate::detection::looks_like_python_source(sample) {
        return Some("python");
    }
    let mut log_lines = 0;
    let mut runtime_marker = false;
    let whole_log = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .all(|line| {
            let line = line.strip_prefix("> ").unwrap_or(line);
            if matches!(line, "stderr:" | "stdout:")
                || line
                    .strip_prefix("exit code ")
                    .is_some_and(|code| code.parse::<i32>().is_ok())
            {
                runtime_marker = true;
                return true;
            }
            if let Some(header) = parse_log_header(line, false) {
                log_lines += 1;
                runtime_marker |= header.strong_single_line;
                return true;
            }
            false
        });
    if whole_log && log_lines > 0 && runtime_marker {
        return Some("log");
    }
    if !sample.starts_with('#') {
        return None;
    }
    let mut source_lines = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let first = source_lines.next()?;
    if commands::has_unlisted_command_syntax(first)
        && source_lines.clone().all(|line| {
            commands::has_unlisted_command_syntax(line)
                || commands::extract_command_handle(line).is_some()
        })
    {
        return Some("text");
    }
    None
}

/// Require body-level document structure before trusting a weak Markdown label.
fn has_positive_markdown_document_evidence(sample: &str) -> bool {
    if sample.contains("```") || sample.contains("~~~") || sample.contains("](") {
        return true;
    }
    let mut non_empty = 0usize;
    let mut quoted = 0usize;
    let mut previous = "";
    for line in sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        non_empty = non_empty.saturating_add(1);
        quoted = quoted.saturating_add(usize::from(line.starts_with("> ")));
        if crate::models::paste::is_markdown_heading_line(line)
            || ((line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ "))
                && !line.contains(": "))
            || crate::models::paste::is_markdown_ordered_list_line(line)
            || (line.starts_with('[') && line.contains("]:"))
            || (line.chars().filter(|ch| !ch.is_whitespace()).count() >= 3
                && ['-', '*', '_']
                    .iter()
                    .any(|marker| line.chars().all(|ch| ch == *marker || ch.is_whitespace())))
            || (!previous.is_empty() && line.chars().all(|ch| ch == '='))
            || (line.contains('|')
                && line.trim_matches('|').split('|').all(|cell| {
                    let cell = cell.trim().trim_matches(':');
                    cell.len() >= 3 && cell.chars().all(|ch| ch == '-')
                }))
            || ["**", "__", "*", "_", "~~", "`"].iter().any(|marker| {
                line.split_once(marker).is_some_and(|(before, rest)| {
                    rest.split_once(marker).is_some_and(|(body, after)| {
                        !body.is_empty()
                            && !body.starts_with(char::is_whitespace)
                            && !body.ends_with(char::is_whitespace)
                            && (!marker.starts_with('_')
                                || (!before.ends_with(char::is_alphanumeric)
                                    && !after.starts_with(char::is_alphanumeric)))
                    })
                })
            })
            || (looks_like_prose(line) && commands::extract_command_handle(line).is_none())
        {
            return true;
        }
        previous = line;
    }

    (non_empty > 0 && quoted == non_empty)
        || (commands::extract_command_handle(sample).is_none() && looks_like_prose(sample))
}

/// Recognize a whole target-and-tabbed-command body without treating embedded examples as code.
fn looks_like_tabbed_command_recipe(sample: &str) -> bool {
    let mut saw_target = false;
    let mut saw_recipe = false;
    let mut in_definition = false;
    for line in sample.lines().filter(|line| !line.trim().is_empty()) {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if in_definition {
            in_definition = trimmed != "endef";
            continue;
        }
        if makefile_directive_with_argument(trimmed, "define") {
            in_definition = true;
            continue;
        }
        if looks_like_makefile_assignment(trimmed)
            || ["include", "-include", "ifeq", "ifneq", "ifdef", "ifndef"]
                .iter()
                .any(|directive| makefile_directive_with_argument(trimmed, directive))
            || matches!(trimmed, "export" | "else" | "endif")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("else ")
        {
            continue;
        }
        if line.starts_with('\t') {
            let recipe = trimmed.trim_start_matches(['@', '+', '-']);
            let command = recipe.split_whitespace().next().unwrap_or_default();
            let executable = !command.is_empty()
                && command.chars().all(|ch| {
                    ch.is_ascii_lowercase()
                        || ch.is_ascii_digit()
                        || matches!(ch, '_' | '-' | '.' | '/' | '\\')
                });
            if !saw_target
                || !(executable
                    || commands::has_unlisted_command_syntax(recipe)
                    || (command.starts_with("$(") && command.ends_with(')')))
            {
                return false;
            }
            saw_recipe = true;
        } else if trimmed.split_once(':').is_some_and(|(target, _)| {
            let target = target.trim();
            !(target.is_empty() || (target.starts_with('[') && target.ends_with(']')))
        }) {
            saw_target = true;
        } else {
            return false;
        }
    }
    saw_target && saw_recipe && !in_definition
}

fn makefile_directive_with_argument(line: &str, directive: &str) -> bool {
    line.strip_prefix(directive)
        .is_some_and(|rest| rest.starts_with(char::is_whitespace) && !rest.trim().is_empty())
}

fn looks_like_makefile_assignment(line: &str) -> bool {
    let line = line.strip_prefix("export ").unwrap_or(line);
    ["?=", ":=", "+=", "!=", "="].iter().any(|operator| {
        line.split_once(operator).is_some_and(|(name, _)| {
            let name = name.trim();
            !name.is_empty() && !name.ends_with(':') && !name.contains(char::is_whitespace)
        })
    })
}

/// Recognize setup prose and sentences containing a later path mistaken for Batch.
///
/// # Arguments
/// - `content`: Paste body whose Batch label is being inspected.
///
/// # Returns
/// Whether prose lacks command-specific arguments. Windows Batch directives,
/// immediate path operands, and options retain their script meaning.
pub(crate) fn looks_like_batch_prose(content: &str) -> bool {
    let sample = sample_prefix(content);
    let Some(line) = sample.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return false;
    };
    let mut parts = line.split_whitespace();
    let command = parts.next().unwrap_or_default();
    if extract_definition_handle_from_line(line, Some("javascript")).is_some() {
        return false;
    }
    let arguments: Vec<_> = parts.collect();
    let prose_arguments = if matches!(command, "set" | "export" | "source") {
        !crate::detection::setup_command_is_valid(command, &arguments, false)
    } else {
        !line.starts_with(['@', ':'])
            && !["rem", "if", "for", "dir", "type", "copy", "move", "call"]
                .iter()
                .any(|directive| command.eq_ignore_ascii_case(directive))
            && arguments
                .iter()
                .zip(arguments.iter().skip(1))
                .any(|(&word, &next)| {
                    matches!(word, "in" | "at" | "by") && crate::detection::is_shell_path(next)
                })
            && !crate::detection::command_has_explicit_arguments(command, line, &arguments)
    };
    prose_arguments
        && commands::extract_command_handle(sample).is_none()
        && looks_like_prose(sample)
}

/// Parse a leading log header while excluding record and TSV header shapes.
///
/// Spaced levels must be uppercase or bracketed. Lowercase words with spaces
/// introduce ordinary prose; colon-delimited lowercase levels remain supported.
fn leading_log_header(sample: &str, language: &str) -> Option<ParsedLogHeader> {
    let first_line = sample
        .lines()
        .map(str::trim_start)
        .find(|line| !line.is_empty())?;
    let header = parse_log_header(first_line, false)?;
    if looks_like_delimited_records(sample) && !header.contextual {
        return None;
    }
    // A detector-provided TSV label plus a tab is sufficient to identify a
    // one-row header. Without this guard, an `INFO`/`ERROR` header is mistaken
    // for a spaced runtime level before stored-language precedence can apply.
    // An explicit parsed context such as `(main)` is log structure rather than
    // a typed table column and may pass the guard.
    if language == "tsv" && first_line.contains('\t') && !header.contextual {
        return None;
    }
    Some(header)
}

#[derive(Clone, Copy)]
struct ParsedLogHeader {
    strong_single_line: bool,
    machine_message: bool,
    contextual: bool,
}

fn parse_log_header(line: &str, allow_lowercase_spaced: bool) -> Option<ParsedLogHeader> {
    const LEVELS: &[&str] = &[
        "trace", "debug", "info", "warn", "warning", "error", "fatal", "success",
    ];

    let line = line.trim_start();
    let (level, mut message, bracketed, colon) =
        if let Some(bracketed_line) = line.strip_prefix('[') {
            let (level, message) = bracketed_line.split_once(']')?;
            (level, message, true, false)
        } else {
            let level = line.split([':', ' ', '\t']).next().unwrap_or("");
            let rest = line.get(level.len()..)?;
            let colon = rest.starts_with(':');
            let message = if colon { rest.get(1..)? } else { rest };
            (level, message, false, colon)
        };
    let lower = level.to_ascii_lowercase();
    if !LEVELS.contains(&lower.as_str())
        || (level != lower && level != level.to_ascii_uppercase())
        || (!allow_lowercase_spaced && !bracketed && !colon && level != level.to_ascii_uppercase())
    {
        return None;
    }

    message = message.trim_start();
    let contextual = message.starts_with('(');
    if let Some(context) = message.strip_prefix('(') {
        let (name, rest) = context.split_once(')')?;
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        {
            return None;
        }
        message = rest.trim_start();
    }
    if message.is_empty() || message.starts_with(['=', ':', '{']) {
        return None;
    }

    Some(ParsedLogHeader {
        strong_single_line: bracketed || (!colon && level == level.to_ascii_uppercase()),
        machine_message: message.chars().next().is_some_and(char::is_uppercase),
        contextual,
    })
}

fn looks_like_multiline_log(sample: &str) -> bool {
    let mut headers = 0usize;
    let mut strong_header = false;
    let mut machine_messages = 0usize;
    let mut header_run_started = false;
    let mut yarn_preamble = false;
    for line in sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if !header_run_started && line.starts_with("yarn ") {
            yarn_preamble = true;
            continue;
        }
        if !header_run_started
            && yarn_preamble
            && line.starts_with('[')
            && line.contains('/')
            && line.contains(']')
        {
            continue;
        }
        if let Some(header) = parse_log_header(line, true) {
            header_run_started = true;
            headers = headers.saturating_add(1);
            strong_header |= header.strong_single_line;
            machine_messages = machine_messages.saturating_add(usize::from(header.machine_message));
        } else {
            break;
        }
    }
    headers >= 2 && (strong_header || yarn_preamble || machine_messages == headers)
}

/// Returns whether untyped text resembles prose rather than a compact data blob.
///
/// Require several words and mostly letters; compact tokens and symbol-heavy
/// snippets have too little evidence to become documents without a language hint.
fn looks_like_prose(sample: &str) -> bool {
    if looks_like_delimited_records(sample) || looks_like_hex_blob(sample) {
        return false;
    }
    let first_line = sample
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or_default();
    if commands::has_unlisted_command_syntax(first_line) {
        return false;
    }
    let words = sample
        .split_whitespace()
        .filter(|token| {
            let word = token.trim_matches(|ch: char| !ch.is_alphabetic());
            !word.is_empty()
                && word
                    .chars()
                    .all(|ch| ch.is_alphabetic() || matches!(ch, '\'' | '’' | '-'))
        })
        .count();
    let mut letters = 0usize;
    let mut symbols = 0usize;
    let mut total = 0usize;
    for ch in sample.chars().filter(|ch| !ch.is_whitespace()) {
        total += 1;
        letters += usize::from(ch.is_alphabetic());
        symbols += usize::from(!ch.is_alphanumeric());
    }
    words >= 3 && letters * 100 >= total * 70 && symbols * 100 <= total * 20
}

/// Returns whether multiple non-empty rows share a common delimited-record shape.
fn looks_like_delimited_records(sample: &str) -> bool {
    let rows: Vec<&str> = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    rows.len() >= 2
        && [',', '\t', ';'].iter().any(|delimiter| {
            let count = rows[0].matches(*delimiter).count();
            if count == 0
                || rows
                    .iter()
                    .any(|row| row.matches(*delimiter).count() != count)
            {
                return false;
            }
            // Commas and semicolons are common in prose. Compact/quoted fields or
            // numeric data distinguish record rows from punctuated sentences.
            *delimiter == '\t'
                || rows.iter().all(|row| {
                    row.split(*delimiter).all(|field| {
                        let field = field.trim();
                        !field.contains(char::is_whitespace)
                            || (field.starts_with('"') && field.ends_with('"'))
                    })
                })
                || rows.iter().any(|row| {
                    row.split(*delimiter)
                        .any(|field| field.trim().parse::<f64>().is_ok())
                })
        })
}

/// Returns whether text is a long whitespace-separated hexadecimal blob.
fn looks_like_hex_blob(sample: &str) -> bool {
    let compact: String = sample
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let hex = compact.strip_prefix("0x").unwrap_or(compact.as_str());
    hex.len() >= 16 && hex.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn extract_definition_handle(sample: &str, language: Option<&str>) -> Option<String> {
    sample
        .lines()
        .find_map(|line| extract_definition_handle_from_line(line, language))
}

/// Extract a compact definition handle from one source line.
///
/// # Arguments
/// - `line`: Source line to inspect.
/// - `language`: Optional language label used to select supported definition patterns.
///
/// # Returns
/// A code-facing handle such as `fn run` or `export function render`, when the
/// line starts with a supported definition pattern for `language`.
fn extract_definition_handle_from_line(line: &str, language: Option<&str>) -> Option<String> {
    let lang = canonicalize(language.unwrap_or_default().trim());
    let patterns: &[&str] = match lang.as_str() {
        "rust" => &["fn ", "struct ", "enum ", "trait ", "impl "],
        "python" => &["def ", "class ", "async def "],
        "javascript" | "typescript" => &[
            "function ",
            "class ",
            "const ",
            "export function ",
            "export class ",
            "export const ",
        ],
        "go" => &["func ", "type ", "package "],
        _ => return None,
    };

    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
        return None;
    }
    for pattern in patterns {
        if let Some(rest) = trimmed.strip_prefix(pattern) {
            let ident: String = rest
                .chars()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect();
            if !ident.is_empty() {
                return Some(format!("{} {}", pattern.trim_end(), ident));
            }
        }
    }

    None
}

fn extract_config_handle(sample: &str) -> Option<String> {
    const PREFERRED_KEYS: &[&str] = &[
        "name",
        "model",
        "dataset",
        "task",
        "service",
        "image",
        "container",
        "project",
    ];

    for line in sample.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        for separator in [':', '='] {
            let Some((key, value)) = trimmed.split_once(separator) else {
                continue;
            };
            let key = clean_atom(key);
            let value = clean_atom(value);
            if PREFERRED_KEYS
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(key.as_str()))
                && !value.is_empty()
            {
                return Some(format!("{} {}", key, value));
            }
        }
    }

    None
}

fn extract_url_handle(sample: &str) -> Option<String> {
    let line = sample
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let url = line
        .strip_prefix("https://")
        .or_else(|| line.strip_prefix("http://"))
        .or_else(|| line.strip_prefix("www."))?;
    let host = url
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/');
    if host.is_empty() {
        return None;
    }
    Some(host.to_string())
}

fn looks_like_single_url(sample: &str) -> bool {
    let mut non_empty = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let Some(first) = non_empty.next() else {
        return false;
    };
    non_empty.next().is_none()
        && (first.starts_with("https://")
            || first.starts_with("http://")
            || first.starts_with("www."))
}

fn extract_terms(sample: &str, language: Option<&str>) -> Vec<String> {
    let lang = canonicalize(language.unwrap_or_default().trim());
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut first_seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut ordinal = 0usize;

    for token in tokenize(sample) {
        let lower = token.to_ascii_lowercase();
        if !is_search_worthy_token(lower.as_str(), lang.as_str()) {
            continue;
        }
        *counts.entry(lower.clone()).or_insert(0) += 1;
        first_seen.entry(lower).or_insert_with(|| {
            let current = ordinal;
            ordinal += 1;
            current
        });
    }

    let mut ranked: Vec<(i32, usize, String)> = counts
        .into_iter()
        .map(|(token, count)| {
            let mut score = count as i32 * 3;
            if token.chars().any(|ch| ch.is_ascii_digit())
                && token.chars().any(|ch| ch.is_ascii_alphabetic())
            {
                score += 3;
            }
            if token.contains('_') || token.contains('-') || token.contains("::") {
                score += 2;
            }
            if (4..=24).contains(&token.len()) {
                score += 1;
            }
            let seen = *first_seen.get(token.as_str()).unwrap_or(&usize::MAX);
            (score, seen, token)
        })
        .collect();

    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (_, _, token) in ranked {
        if seen.insert(token.clone()) {
            out.push(token);
        }
        if out.len() >= MAX_TERMS {
            break;
        }
    }
    out
}

fn synthesize_handle_from_terms(terms: &[String]) -> Option<String> {
    match terms {
        [first, second, ..] => Some(format!("{} {}", first, second)),
        [only] => Some(only.clone()),
        [] => None,
    }
}

fn tokenize(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.') {
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn is_search_worthy_token(token: &str, language: &str) -> bool {
    if token.len() < 3 || token.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }

    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "from", "that", "this", "into", "your", "have", "has", "was",
        "were", "are", "but", "not", "all", "any", "none", "some", "then", "when", "true", "false",
        "null", "body", "line", "lines", "text", "paste", "failed", "after", "before", "retry",
        "retries", "repeated", "error", "errors", "warning", "warnings",
    ];
    if STOPWORDS.contains(&token) {
        return false;
    }

    let language_keywords: &[&str] = match language {
        "rust" => &["fn", "let", "pub", "impl", "use", "mod", "self"],
        "python" => &["def", "class", "self", "import", "from", "pass"],
        "javascript" | "typescript" => &["const", "let", "class", "function", "export", "import"],
        _ => &[],
    };
    !language_keywords.contains(&token)
}

fn clean_atom(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
        .take(24)
        .collect()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Content classification and retrieval regression coverage.
#[cfg(test)]
mod tests;
