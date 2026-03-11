//! Lightweight locally-derived semantic metadata for retrieval.

use crate::detection::canonical::canonicalize;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const SAMPLE_MAX_BYTES: usize = 64 * 1024;
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

    let kind = classify_kind(sample, language);
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

    let mut end = trimmed.len().min(SAMPLE_MAX_BYTES);
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = &trimmed[..end];

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

fn classify_kind(sample: &str, language: Option<&str>) -> PasteKind {
    let lang = canonicalize(language.unwrap_or_default().trim());
    let lower = sample.to_ascii_lowercase();

    if looks_like_single_url(sample) {
        return PasteKind::Link;
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
        "json" | "yaml" | "toml" | "xml" | "dockerfile" | "makefile"
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

    PasteKind::Other
}

fn extract_definition_handle(sample: &str, language: Option<&str>) -> Option<String> {
    let lang = canonicalize(language.unwrap_or_default().trim());
    let patterns: &[&str] = match lang.as_str() {
        "rust" => &["fn ", "struct ", "enum ", "trait ", "impl "],
        "python" => &["def ", "class ", "async def "],
        "javascript" | "typescript" => &["function ", "class ", "const ", "export function "],
        "go" => &["func ", "type ", "package "],
        _ => return None,
    };

    for line in sample.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
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
    }

    None
}

fn extract_command_handle(sample: &str) -> Option<String> {
    const COMMANDS: &[&str] = &[
        "cargo", "git", "docker", "kubectl", "python", "pytest", "uv", "pip", "npm", "pnpm",
        "yarn", "make", "just", "curl", "wget", "ssh", "torchrun",
    ];

    for line in sample.lines() {
        let trimmed = line.trim().trim_matches('`');
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().take(4).collect();
        let Some(cmd) = parts.first().map(|part| part.to_ascii_lowercase()) else {
            continue;
        };
        if !COMMANDS.iter().any(|known| *known == cmd) {
            continue;
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

#[cfg(test)]
mod tests {
    use super::{derive, PasteKind};

    #[test]
    fn derive_matrix_covers_code_config_log_link_and_other() {
        let code = derive("fn handle_request(input: &str) {}\n", Some("rust"));
        assert_eq!(code.kind, PasteKind::Code);
        assert_eq!(code.handle.as_deref(), Some("fn handle_request"));

        let config = derive("model: gpt-4\nbatch: 32\n", Some("yaml"));
        assert_eq!(config.kind, PasteKind::Config);
        assert_eq!(config.handle.as_deref(), Some("model gpt-4"));

        let log = derive(
            "panic: failed to bind\ncaused by: port already in use\n",
            Some("text"),
        );
        assert_eq!(log.kind, PasteKind::Log);
        assert!(log
            .handle
            .as_deref()
            .map(|handle| handle.starts_with("panic"))
            .unwrap_or(false));

        let link = derive("https://example.com/docs\n", Some("text"));
        assert_eq!(link.kind, PasteKind::Link);
        assert_eq!(link.handle.as_deref(), Some("example.com"));

        let other = derive("hi", Some("text"));
        assert_eq!(other.kind, PasteKind::Other);
        assert!(other.handle.is_none());
    }

    #[test]
    fn derive_terms_prefers_repeated_technical_tokens() {
        let derived = derive(
            "validation failed for fsdp2 after cublaslt retry\nfsdp2 validation repeated\n",
            Some("text"),
        );
        assert!(derived.terms.iter().any(|term| term == "fsdp2"));
        assert!(derived.terms.iter().any(|term| term == "validation"));
        assert!(derived.terms.iter().any(|term| term == "cublaslt"));
    }
}
