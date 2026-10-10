//! Lightweight locally-derived semantic metadata for retrieval.

use crate::detection::canonical::canonicalize;
use crate::text::{complete_line_prefix_by_bytes, TEXT_SAMPLE_MAX_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Shared command recognition and semantic-handle extraction.
pub(crate) mod commands;
/// Definition handles for source in recognized languages.
mod definitions;
use definitions::{extract_definition_handle, extract_definition_handle_from_line};
/// Runtime-output recognition shared by detection and semantic kinds.
pub(crate) mod logs;
mod makefile;
pub(crate) use makefile::{
    makefile_body_language, makefile_note_body, makefile_rules_read_as_prose,
};

const SAMPLE_MAX_LINES: usize = 256;
const MAX_TERMS: usize = 4;
const MAX_HANDLE_CHARS: usize = 48;
/// Longest command operand or config value copied into a handle. Longer ones
/// keep their leading characters (a config value keeps whole leading words), so
/// the handle stays a prefix of text that appears in the paste.
const MAX_HANDLE_ATOM_CHARS: usize = 24;
/// Longest token kept as a derived term, counted in characters rather than
/// bytes so non-ASCII words get the same allowance. Longer tokens (minified
/// blobs, base64 lines, a single giant word) are not retrieval terms and would
/// otherwise be persisted in the metadata projection and shown in full by the
/// UI.
const MAX_TERM_CHARS: usize = 48;

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

/// Recognize a single prose sentence containing incidental JavaScript keywords.
///
/// # Arguments
/// - `content`: Body to inspect for grammatical prose around language keywords.
///
/// # Returns
/// Whether grammatical words establish a note rather than source declarations.
pub(crate) fn javascript_keyword_prose(content: &str) -> bool {
    let sample = sample_prefix(content);
    let lower = sample.to_ascii_lowercase();
    // This grammatical imperative cannot be a JavaScript declaration; ordinary
    // parentheses or a semicolon later in the sentence do not turn it into one.
    let note_lead = lower.starts_with("let me know ");
    (note_lead || !sample.contains(['\n', '=', '(', ')', '{', '}', ';']))
        && !["//", "/*"].iter().any(|prefix| sample.starts_with(prefix))
        && lower.contains("let ")
        && lower.contains("function")
        && looks_like_prose(sample)
        && sample.split_whitespace().any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "me" | "you" | "your" | "our" | "the" | "please"
            )
        })
}

fn classify_kind(content: &str, language: Option<&str>) -> PasteKind {
    let sample = sample_prefix(content);
    let lang = canonicalize(language.unwrap_or_default().trim());

    // Detection locks inferred labels too. Positive note structure corrects the
    // retrieval kind without rewriting a stored highlighting/export choice.
    if (matches!(lang.as_str(), "shell" | "makefile") && makefile_note_body(sample))
        || (lang == "makefile" && makefile_rules_read_as_prose(sample))
        || (lang == "javascript" && javascript_keyword_prose(sample))
    {
        return PasteKind::Document;
    }

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
    }

    if lang != "markdown" && is_document_language(language) {
        return PasteKind::Document;
    }

    if looks_like_single_url(sample) {
        return PasteKind::Link;
    }

    // Statistical detection can lock an incidental code, config, or Markdown
    // label onto a real log. Structural runtime output still determines the
    // retrieval kind; attribute assignments and bare TOML table headers remain
    // configuration.
    if logs::looks_like_runtime_log(sample) {
        return PasteKind::Log;
    }
    if lang == "markdown" && has_positive_markdown_document_evidence(sample) {
        return PasteKind::Document;
    }
    let log_header = logs::leading_log_header(sample, lang.as_str());
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

    if extract_definition_handle(sample, language).is_some()
        || commands::extract_command_handle(sample).is_some()
        || commands::looks_like_shell_command_sequence(sample)
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
    if commands::looks_like_shell_command_sequence(sample)
        || makefile_body_language(sample).is_some()
    {
        return Some("shell");
    }
    if sample.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("```") || line.starts_with("~~~")
    }) {
        return None;
    }
    if logs::looks_like_crash_output(sample) {
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
            if let Some(header) = logs::parse_log_header(line, false) {
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
        !commands::setup_command_is_valid(command, &arguments, false)
    } else {
        !line.starts_with(['@', ':'])
            && !["rem", "if", "for", "dir", "type", "copy", "move", "call"]
                .iter()
                .any(|directive| command.eq_ignore_ascii_case(directive))
            && arguments
                .iter()
                .zip(arguments.iter().skip(1))
                .any(|(&word, &next)| {
                    matches!(word, "in" | "at" | "by") && commands::is_shell_path(next)
                })
            && !commands::command_has_explicit_arguments(command, line, &arguments)
    };
    prose_arguments
        && commands::extract_command_handle(sample).is_none()
        && looks_like_prose(sample)
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
            // The key must match whole: cutting it at its first space would
            // turn prose such as `service is down: restart it` into a config
            // entry. A flow-style one-liner such as `{"name": "Ada"}` opens
            // its first key with the container.
            let key = unquote(unquote(key).trim_start_matches(['{', '[']));
            if !PREFERRED_KEYS
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(key))
            {
                continue;
            }
            let value = clean_config_value(value);
            if !value.is_empty() {
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
        let lower = normalize_term(token);
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
            if token.chars().any(char::is_numeric) && token.chars().any(char::is_alphabetic) {
                score += 3;
            }
            if token.contains('_') || token.contains('-') || token.contains("::") {
                score += 2;
            }
            if (4..=24).contains(&token.chars().count()) {
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

/// Split text into term tokens: runs of letters and digits in any script
/// (`char::is_alphanumeric`) plus `_`, `-`, `:`, and `.`. Emoji, symbols, and
/// other punctuation separate tokens. Scripts written without spaces, such as
/// Chinese and Japanese, yield one token per run between punctuation marks.
fn tokenize(input: &str) -> impl Iterator<Item = &str> {
    input
        .split(|ch: char| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.')))
        .filter(|token| !token.is_empty())
}

/// Bring a token to the form derived terms are stored in: edge punctuation
/// trimmed ([`trim_term_edges`]) and full Unicode lowercasing applied.
///
/// Metadata search normalizes its query terms with this same function, so a
/// query such as `--release`, `Note:`, or `Café` compares equal to the stored
/// term `release`, `note`, or `café`.
///
/// # Returns
/// The trimmed, lowercased token, which may be empty.
pub(crate) fn normalize_term(token: &str) -> String {
    trim_term_edges(token).to_lowercase()
}

/// Strip sentence and key punctuation from both ends of a token.
///
/// `.`, `:`, and `-` are token characters so that `localpaste.rs`,
/// `hash_map::entry`, and `kebab-case` stay whole, but at an edge they come
/// from prose, YAML keys, URL schemes, or CLI flags (`deploy.`, `name:`,
/// `https:`, `--release`) and would make `name` and `name:` distinct terms.
/// Underscores are kept because leading and trailing ones are part of
/// identifiers such as `__init__` and `_private`. Case-sensitive metadata
/// search applies only this step to its query terms, because stored terms are
/// lowercase and a differently cased query must not match them.
///
/// # Returns
/// The token without leading or trailing `.`, `:`, or `-`.
pub(crate) fn trim_term_edges(token: &str) -> &str {
    token.trim_matches(['.', ':', '-'])
}

/// Whether a normalized token qualifies as a derived term.
///
/// A term is 3 to [`MAX_TERM_CHARS`] characters long, holds at least one
/// letter, and is neither a stopword nor a keyword of `language`. Tokens
/// without a letter (`2026-10-10`, `06:00:01`, `1.2.3`, `127.0.0.1`) are dates,
/// times, versions, and addresses that would otherwise lead a timestamped
/// log's synthesized handle; body search still finds them.
fn is_search_worthy_token(token: &str, language: &str) -> bool {
    // Tokens may hold any script, so the limits count characters, not bytes.
    let chars = token.chars().count();
    if !(3..=MAX_TERM_CHARS).contains(&chars) || !token.chars().any(char::is_alphabetic) {
        return false;
    }

    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "from", "that", "this", "into", "your", "have", "has", "was",
        "were", "are", "but", "not", "all", "any", "none", "some", "then", "when", "true", "false",
        "null", "body", "line", "lines", "text", "paste", "failed", "after", "before", "retry",
        "retries", "repeated", "error", "errors", "warning", "warnings", "http", "https", "www",
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

/// Strip surrounding whitespace, quotes, and backticks from a handle operand.
fn unquote(value: &str) -> &str {
    value.trim().trim_matches(['"', '\'', '`'])
}

/// Characters that may appear inside a handle operand: letters and digits in
/// any script, and the path characters `_ - . : / \ ~ @ + ,`. `=` is
/// deliberately absent: the text after it is an assigned value, often a secret
/// or noise, and the name before it is the useful part of the handle.
fn is_atom_char(ch: char) -> bool {
    ch.is_alphanumeric()
        || matches!(
            ch,
            '_' | '-' | '.' | ':' | '/' | '\\' | '~' | '@' | '+' | ','
        )
}

/// Copy a command operand into a handle.
///
/// The operand is taken verbatim from its start up to the first character that
/// is not an identifier or path character (a space, quote, glob, `$`, `;`, `=`,
/// and so on) and is capped at [`MAX_HANDLE_ATOM_CHARS`]. Characters are never
/// deleted from the middle, so the result is always a prefix of text that
/// appears in the paste, with a trailing separator comma dropped.
fn clean_atom(value: &str) -> String {
    let atom: String = unquote(value)
        .chars()
        .take_while(|ch| is_atom_char(*ch))
        .take(MAX_HANDLE_ATOM_CHARS)
        .collect();
    atom.trim_end_matches(',').to_string()
}

/// Copy a config value into a handle.
///
/// Unlike a command operand, a config value may hold several words, so the
/// same character set plus whitespace is read up to the first other character
/// (`#`, a quote, `{`, `=`, and so on) and whitespace runs collapse to one
/// space. A value longer than [`MAX_HANDLE_ATOM_CHARS`] keeps whole words, and
/// only a first word that is itself too long is cut mid-word.
fn clean_config_value(value: &str) -> String {
    let text = unquote(value);
    let end = text
        .find(|ch: char| !(is_atom_char(ch) || ch.is_whitespace()))
        .unwrap_or(text.len());
    let mut out = String::new();
    for word in text[..end].split_whitespace() {
        let separator = usize::from(!out.is_empty());
        if out.chars().count() + separator + word.chars().count() > MAX_HANDLE_ATOM_CHARS {
            if out.is_empty() {
                out = truncate_chars(word, MAX_HANDLE_ATOM_CHARS);
            }
            break;
        }
        if separator == 1 {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.trim_end_matches(',').to_string()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Command-versus-prose regression coverage.
#[cfg(test)]
mod command_tests;
/// Structure-based log recognition regression coverage.
#[cfg(test)]
mod log_tests;
/// Derived-term extraction regression coverage.
#[cfg(test)]
mod term_tests;
/// Content classification and retrieval regression coverage.
#[cfg(test)]
mod tests;
