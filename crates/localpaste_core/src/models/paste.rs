//! Paste-related data models and language detection.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Paste metadata stored in the database and returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paste {
    pub id: String,
    pub name: String,
    pub content: String,
    pub language: Option<String>,
    #[serde(default)]
    pub language_is_manual: bool,
    pub folder_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub tags: Vec<String>,
    pub is_markdown: bool,
}

/// Lightweight paste metadata used by GUI list/search paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PasteMeta {
    pub id: String,
    pub name: String,
    pub language: Option<String>,
    pub folder_id: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub tags: Vec<String>,
    pub content_len: usize,
    pub is_markdown: bool,
}

/// Request payload for creating a paste.
#[derive(Debug, Deserialize)]
pub struct CreatePasteRequest {
    pub content: String,
    pub language: Option<String>,
    pub language_is_manual: Option<bool>,
    pub folder_id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub name: Option<String>,
}

/// Request payload for updating a paste.
#[derive(Debug, Deserialize, Clone)]
pub struct UpdatePasteRequest {
    pub content: Option<String>,
    pub name: Option<String>,
    pub language: Option<String>,
    pub language_is_manual: Option<bool>,
    pub folder_id: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Query parameters for searching pastes.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub folder_id: Option<String>,
    pub language: Option<String>,
    pub limit: Option<usize>,
}

/// Query parameters for listing pastes.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<usize>,
    pub folder_id: Option<String>,
}

impl Paste {
    /// Create a new paste with inferred language and defaults.
    ///
    /// # Arguments
    /// - `content`: Paste content.
    /// - `name`: Paste display name.
    ///
    /// # Returns
    /// A new [`Paste`] instance.
    pub fn new(content: String, name: String) -> Self {
        let now = Utc::now();
        let language = detect_language(&content);
        let is_markdown = is_markdown_content(&content);
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            content,
            language,
            language_is_manual: false,
            folder_id: None,
            created_at: now,
            updated_at: now,
            tags: Vec::new(),
            is_markdown,
        }
    }
}

impl From<&Paste> for PasteMeta {
    fn from(value: &Paste) -> Self {
        Self {
            id: value.id.clone(),
            name: value.name.clone(),
            language: value.language.clone(),
            folder_id: value.folder_id.clone(),
            updated_at: value.updated_at,
            tags: value.tags.clone(),
            content_len: value.content.len(),
            is_markdown: value.is_markdown,
        }
    }
}

/// Normalize an optional language filter value.
///
/// # Returns
/// Lowercased language when non-empty after trimming, otherwise `None`.
pub fn normalize_language_filter(language: Option<&str>) -> Option<String> {
    language
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn is_markdown_heading_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut hash_count = 0usize;
    while hash_count < bytes.len() && bytes[hash_count] == b'#' {
        hash_count += 1;
    }
    if hash_count == 0 || hash_count > 6 {
        return false;
    }
    bytes.get(hash_count) == Some(&b' ')
}

fn is_markdown_ordered_list_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut digits = 0usize;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if digits == 0 {
        return false;
    }
    bytes.get(digits) == Some(&b'.') && bytes.get(digits + 1) == Some(&b' ')
}

fn is_markdown_list_line(line: &str) -> bool {
    ((line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ "))
        && !line.contains(": "))
        || is_markdown_ordered_list_line(line)
}

/// Heuristic markdown detection used for persisted paste metadata.
///
/// This intentionally avoids broad character checks (such as raw `#`) that
/// produce false positives in source/config formats.
///
/// # Returns
/// `true` when the content appears to use markdown structure markers.
pub fn is_markdown_content(content: &str) -> bool {
    if content.trim().is_empty() {
        return false;
    }
    if content.contains("```") || content.contains("](") {
        return true;
    }
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        is_markdown_heading_line(trimmed)
            || trimmed.starts_with("> ")
            || is_markdown_list_line(trimmed)
    })
}

/// Best-effort language detection based on simple heuristics.
///
/// Detection samples only the first portion of text to keep allocation and scan
/// costs bounded for large pastes.
///
/// # Returns
/// Detected language identifier, or `None` if unknown.
pub fn detect_language(content: &str) -> Option<String> {
    const SAMPLE_MAX_BYTES: usize = 64 * 1024;
    const SAMPLE_MAX_LINES: usize = 512;

    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let sample = utf8_prefix(trimmed, SAMPLE_MAX_BYTES);
    let lower = sample.to_ascii_lowercase();
    let lines = || sample.lines().take(SAMPLE_MAX_LINES);

    // JSON: structural check without full parsing (avoids expensive serde_json)
    if sample.starts_with('{') || sample.starts_with('[') {
        // When sampling truncates very large JSON payloads, the prefix may not end
        // with the final closing delimiter. Keep large-document detection stable.
        let sample_truncated = sample.len() < trimmed.len();
        let looks_closed = sample.ends_with('}') || sample.ends_with(']');
        if sample.contains('"')
            && (sample.contains(':') || sample.starts_with('['))
            && (looks_closed || sample_truncated)
        {
            return Some("json".to_string());
        }
    }

    // HTML before generic XML so we don't mis-classify
    if lower.contains("<!doctype html")
        || lower.contains("<html")
        || lower.contains("<body")
        || lower.contains("<div")
    {
        return Some("html".to_string());
    }

    if lower.starts_with("<?xml")
        || (sample.starts_with('<') && lower.contains("</") && !lower.contains("<html"))
    {
        return Some("xml".to_string());
    }

    if lower.starts_with("#!/bin/")
        || lower.starts_with("#!/usr/bin/env bash")
        || (lower.contains("echo ") && lower.contains('$') && lower.contains('\n'))
        || lower.contains("\nfi")
        || lower.contains("\ndone")
    {
        return Some("shell".to_string());
    }

    let yaml_pairs = lines()
        .filter(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                return false;
            }
            (t.starts_with("- ") || t.contains(": ")) && !t.contains('{')
        })
        .count();
    if (lower.starts_with("---") || yaml_pairs >= 2) && !sample.contains('{') {
        return Some("yaml".to_string());
    }

    let has_toml_header = lines().any(|l| {
        let t = l.trim();
        t.starts_with('[') && t.ends_with(']') && t.len() > 2
    });
    let toml_assignments = lines()
        .filter(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with('[') {
                return false;
            }
            t.contains('=') && !t.contains("==")
        })
        .count();
    if has_toml_header && toml_assignments >= 1 {
        return Some("toml".to_string());
    }

    if is_markdown_content(sample) {
        return Some("markdown".to_string());
    }

    if lower.contains("\\begin{")
        || lower.contains("\\documentclass")
        || lower.contains("\\section")
    {
        return Some("latex".to_string());
    }

    if lower.contains('{') && lower.contains('}') && lower.contains(':') && lower.contains(';') {
        let css_tokens = [
            "color:",
            "background",
            "margin",
            "padding",
            "font-",
            "display",
            "position",
            "flex",
            "grid",
        ];
        if css_tokens.iter().any(|token| lower.contains(token)) {
            return Some("css".to_string());
        }
    }

    let keyword_hits =
        |keywords: &[&str]| -> usize { keywords.iter().filter(|kw| lower.contains(*kw)).count() };

    // Specialised checks for languages with distinctive constructs
    if lower.contains("using system")
        || (lower.contains("namespace ") && lower.contains("class ") && lower.contains("console."))
    {
        return Some("csharp".to_string());
    }

    if lower.contains("std::")
        || lower.contains("using namespace std")
        || lower.contains("template <")
    {
        return Some("cpp".to_string());
    }

    if lower.contains("#include") && (lower.contains("int main") || lower.contains("printf")) {
        return Some("c".to_string());
    }

    let scored_languages: &[(&str, &[&str], usize)] = &[
        (
            "rust",
            &[
                "fn ", "impl", "crate::", "let ", "mut ", "pub ", "struct ", "enum", "match ",
                "trait",
            ],
            2,
        ),
        (
            "python",
            &[
                "def ",
                "import ",
                "from ",
                "class ",
                "self",
                "async def",
                "elif",
                "print(",
            ],
            2,
        ),
        (
            "javascript",
            &[
                "function",
                "const ",
                "let ",
                "=>",
                "console.",
                "document.",
                "export ",
                "import ",
            ],
            2,
        ),
        (
            "typescript",
            &[
                "interface ",
                " type ",
                ": string",
                ": number",
                "implements ",
                " enum ",
                "<t>",
                "readonly ",
            ],
            2,
        ),
        (
            "go",
            &[
                "package ",
                "func ",
                "fmt.",
                "defer ",
                "go ",
                "chan",
                "interface",
                "select {",
            ],
            2,
        ),
        (
            "java",
            &[
                "public class",
                "import java.",
                "system.out",
                " implements ",
                " extends ",
                " void main",
            ],
            2,
        ),
        (
            "csharp",
            &[
                "using system",
                "namespace ",
                "public class",
                "console.",
                " async ",
                " task<",
                " get;",
            ],
            2,
        ),
        (
            "sql",
            &[
                "select ",
                "insert ",
                "update ",
                "delete ",
                " from ",
                " where ",
                " join ",
                "create table",
            ],
            2,
        ),
        (
            "latex",
            &[
                "\\begin{",
                "\\end{",
                "\\usepackage",
                "\\documentclass",
                "\\section",
            ],
            2,
        ),
    ];

    let mut best_match: Option<(&str, usize)> = None;
    for (lang, keywords, threshold) in scored_languages {
        let hits = keyword_hits(keywords);
        if hits >= *threshold {
            match best_match {
                Some((_, best_hits)) if best_hits >= hits => {}
                _ => best_match = Some((*lang, hits)),
            }
        }
    }

    if let Some((lang, _)) = best_match {
        return Some(lang.to_string());
    }

    None
}

fn utf8_prefix(content: &str, max_bytes: usize) -> &str {
    if content.len() <= max_bytes {
        return content;
    }
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &content[..end]
}
