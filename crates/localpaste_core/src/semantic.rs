//! Lightweight locally-derived semantic metadata for retrieval.

use crate::detection::canonical::canonicalize;
use crate::text::{utf8_prefix_by_bytes, TEXT_SAMPLE_MAX_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
        return DerivedMeta::default();
    }

    let kind = classify_kind(content, language);
    let terms = extract_terms(sample, language);
    let handle = extract_definition_handle(sample, language)
        .or_else(|| extract_command_handle(sample))
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

    let prefix = utf8_prefix_by_bytes(trimmed, TEXT_SAMPLE_MAX_BYTES);

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
                return PasteKind::Document;
            }
            if !is_document_language(Some(&inner_language)) {
                let detected = inner_language
                    .is_empty()
                    .then(|| crate::detection::detect_heuristically(body))
                    .flatten();
                return match classify_kind(
                    body,
                    detected.as_deref().or(Some(inner_language.as_str())),
                ) {
                    PasteKind::Document | PasteKind::Other if inner_language.is_empty() => {
                        PasteKind::Document
                    }
                    PasteKind::Document | PasteKind::Other => PasteKind::Code,
                    kind => kind,
                };
            }
        }
    }

    if is_document_language(language) {
        return PasteKind::Document;
    }

    if looks_like_single_url(sample) {
        return PasteKind::Link;
    }

    // Statistical detection can lock an incidental code/config label onto a
    // real log. Structural runtime headers still determine retrieval kind.
    // Attribute assignments and bare TOML table headers remain configuration.
    if has_unambiguous_log_header(sample) || crate::detection::looks_like_rust_panic(sample) {
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
        || extract_command_handle(sample).is_some()
    {
        return PasteKind::Code;
    }

    if extract_config_handle(sample).is_some() {
        return PasteKind::Config;
    }

    if starts_with_log_level(sample) || crate::detection::looks_like_rust_panic(sample) {
        return PasteKind::Log;
    }

    if (lang.is_empty() || lang == "text") && looks_like_prose(sample) {
        PasteKind::Document
    } else {
        PasteKind::Other
    }
}

/// Returns whether the first non-empty line starts with a machine-style log level.
///
/// Spaced levels must be uppercase or bracketed. Lowercase words with spaces
/// introduce ordinary prose; colon-delimited lowercase levels remain supported.
fn starts_with_log_level(sample: &str) -> bool {
    if looks_like_delimited_records(sample) {
        return false;
    }
    let Some(first_line) = sample
        .lines()
        .map(str::trim_start)
        .find(|line| !line.is_empty())
    else {
        return false;
    };
    let (level, bracketed, colon) = if let Some(bracketed) = first_line.strip_prefix('[') {
        let Some((level, _)) = bracketed.split_once(']') else {
            return false;
        };
        (level, true, false)
    } else {
        let level = first_line.split([':', ' ', '\t']).next().unwrap_or("");
        (level, false, first_line[level.len()..].starts_with(':'))
    };
    let lower = level.to_ascii_lowercase();
    if !bracketed
        && !colon
        && first_line[level.len()..]
            .trim_start()
            .starts_with(['(', '{', '='])
    {
        return false;
    }
    (level == lower || level == level.to_ascii_uppercase())
        && (bracketed || colon || level == level.to_ascii_uppercase())
        && [
            "trace:", "debug:", "info:", "warn:", "warning:", "error:", "fatal:",
        ]
        .iter()
        .any(|marker| marker.strip_suffix(':') == Some(lower.as_str()))
}

/// Whether a spaced or bracketed log level has a message rather than an assignment.
///
/// # Returns
/// True for strong log headers even when a detector supplied a code/config label.
fn has_unambiguous_log_header(sample: &str) -> bool {
    if !starts_with_log_level(sample) {
        return false;
    }
    let Some(first_line) = sample.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return false;
    };
    let message = if first_line.starts_with('[') {
        first_line.split_once(']').map(|(_, message)| message)
    } else {
        first_line
            .split_once(char::is_whitespace)
            .and_then(|(level, message)| (!level.contains(':')).then_some(message))
    };
    message.is_some_and(|message| {
        let message = message.trim_start();
        !message.is_empty() && !message.starts_with(['=', ':', '(', '{'])
    })
}

/// Returns whether untyped text resembles prose rather than a compact data blob.
///
/// Require several words and mostly letters; compact tokens and symbol-heavy
/// snippets have too little evidence to become documents without a language hint.
fn looks_like_prose(sample: &str) -> bool {
    if looks_like_delimited_records(sample) || looks_like_hex_blob(sample) {
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

fn extract_command_handle(sample: &str) -> Option<String> {
    const COMMANDS: &[&str] = &[
        "brew",
        "cargo",
        "git",
        "docker",
        "kubectl",
        "python",
        "pytest",
        "uv",
        "pip",
        "npm",
        "pnpm",
        "yarn",
        "make",
        "just",
        "curl",
        "wget",
        "ssh",
        "torchrun",
        "ls",
        "sudo",
        "echo",
        "printf",
        "systemctl",
    ];

    // Later command-shaped lines may be explanations or quoted email content.
    // A command must lead the paste and use an executable's case-sensitive name.
    if let Some(line) = sample.lines().find(|line| !line.trim().is_empty()) {
        let trimmed = line.trim().trim_matches('`');

        let parts: Vec<&str> = trimmed.split_whitespace().take(4).collect();
        let cmd = parts.first().map(|part| part.to_ascii_lowercase())?;
        if parts[0] != cmd || !COMMANDS.iter().any(|known| *known == cmd) {
            return None;
        }
        // Commands use their case-sensitive executable name. `make` and `just`
        // also begin ordinary lowercase sentences with an article.
        if matches!(cmd.as_str(), "make" | "just")
            && parts
                .get(1)
                .is_some_and(|part| ["a", "an", "the"].contains(part))
        {
            return None;
        }

        // Unquoted copulas near the verb are prose evidence (`echo chamber is`,
        // `sudo is required`). Quoting, options, and shell syntax supply command
        // evidence even when the argument text contains those words.
        if !trimmed.contains(['\'', '"', '$', '|', '>', '<'])
            && !parts.iter().skip(1).any(|part| part.starts_with('-'))
            && parts
                .iter()
                .skip(1)
                .any(|part| matches!(*part, "is" | "are" | "was" | "were"))
        {
            return None;
        }

        let sub = parts
            .get(1)
            .copied()
            .map(clean_atom)
            .filter(|value| !value.is_empty() && !value.starts_with('-'));

        return Some(match sub {
            Some(sub) => format!("{} {}", cmd, sub),
            None => cmd,
        });
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
