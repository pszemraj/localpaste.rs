//! Paste-related data models and language detection.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::detect_language as detect_language_impl;

/// Paste metadata stored in the database and returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paste {
    pub id: String,
    pub name: String,
    pub content: String,
    pub language: Option<String>,
    /// Language lock flag.
    ///
    /// `true` means the stored language is locked against automatic
    /// re-detection. This can come from explicit user choice or from
    /// create-time detection resolving a concrete language.
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
    /// Create a new paste with explicit language/manual-state values.
    ///
    /// This constructor is used by callers that already resolved language and
    /// want to avoid duplicate detection work during creation paths.
    ///
    /// # Arguments
    /// - `content`: Paste content.
    /// - `name`: Paste display name.
    /// - `language`: Precomputed or manually provided language label.
    /// - `language_is_manual`: Whether language should be treated as user-managed.
    ///
    /// # Returns
    /// A new [`Paste`] instance.
    pub fn new_with_language(
        content: String,
        name: String,
        language: Option<String>,
        language_is_manual: bool,
    ) -> Self {
        let now = Utc::now();
        let is_markdown = is_markdown_content(&content);
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            content,
            language,
            language_is_manual,
            folder_id: None,
            created_at: now,
            updated_at: now,
            tags: Vec::new(),
            is_markdown,
        }
    }

    /// Create a new paste with inferred language and defaults.
    ///
    /// # Arguments
    /// - `content`: Paste content.
    /// - `name`: Paste display name.
    ///
    /// # Returns
    /// A new [`Paste`] instance.
    pub fn new(content: String, name: String) -> Self {
        let language = detect_language(&content);
        // When creation-time detection resolves a concrete language, persist it
        // as a locked choice to avoid repeated re-detect churn on subsequent edits.
        let language_is_manual = language.is_some();
        Self::new_with_language(content, name, language, language_is_manual)
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
        .map(crate::detection::canonical::canonicalize)
        .filter(|value| !value.is_empty())
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

/// Detect language for paste content using core detection adapters.
///
/// # Returns
/// Canonical language label when detection succeeds, otherwise `None`.
pub fn detect_language(content: &str) -> Option<String> {
    detect_language_impl(content)
}
