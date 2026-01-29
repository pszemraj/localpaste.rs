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
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            content: content.clone(),
            language: detect_language(&content),
            language_is_manual: false,
            folder_id: None,
            created_at: now,
            updated_at: now,
            tags: Vec::new(),
            is_markdown: content.contains("```") || content.contains('#'),
        }
    }
}

/// Best-effort language detection based on simple heuristics.
///
/// For large content, the caller should sample the first ~10KB before calling.
///
/// # Returns
/// Detected language identifier, or `None` if unknown.
pub fn detect_language(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();
    let lines: Vec<&str> = trimmed.lines().collect();

    // JSON: structural check without full parsing (avoids expensive serde_json)
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        // Look for JSON-like structure: balanced braces, quotes, colons
        if (trimmed.ends_with('}') || trimmed.ends_with(']'))
            && trimmed.contains('"')
            && (trimmed.contains(':') || trimmed.starts_with('['))
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
        || (trimmed.starts_with('<') && lower.contains("</") && !lower.contains("<html"))
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

    let yaml_pairs = lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            if t.is_empty() || t.starts_with('#') {
                return false;
            }
            (t.starts_with("- ") || t.contains(": ")) && !t.contains('{')
        })
        .count();
    if (lower.starts_with("---") || yaml_pairs >= 2) && !trimmed.contains('{') {
        return Some("yaml".to_string());
    }

    let has_toml_header = lines.iter().any(|l| {
        let t = l.trim();
        t.starts_with('[') && t.ends_with(']') && t.len() > 2
    });
    let toml_assignments = lines
        .iter()
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

    let markdown_heading = lines.iter().any(|l| {
        let t = l.trim_start();
        t.starts_with("# ") || t.starts_with("## ") || t.starts_with("### ") || t.starts_with("> ")
    });
    let markdown_list = lines.iter().any(|l| {
        let t = l.trim_start();
        (t.starts_with("- ") || t.starts_with("* ")) && !t.contains(": ")
    });
    if trimmed.contains("```") || lower.contains("](") || markdown_heading || markdown_list {
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
