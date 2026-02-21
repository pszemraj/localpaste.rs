//! Helper functions shared by paste storage operations.

use crate::models::paste::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Converts a timestamp into a reverse-sorted key for newest-first indexes.
///
/// # Returns
/// A monotonic reverse key where newer timestamps produce smaller deltas from
/// `u64::MAX`.
pub(crate) fn reverse_timestamp_key(updated_at: DateTime<Utc>) -> u64 {
    // Pre-epoch timestamps are clamped to preserve total ordering semantics for
    // expected runtime data while avoiding negative->u64 underflow.
    let millis = updated_at.timestamp_millis().max(0) as u64;
    u64::MAX.saturating_sub(millis)
}

/// Applies an [`UpdatePasteRequest`] onto an existing [`Paste`] in place.
///
/// This helper centralizes update semantics so server and GUI write paths keep
/// language/manual-mode behavior aligned.
///
/// # Arguments
/// - `paste`: Mutable paste row to update.
/// - `update`: Incoming patch payload.
pub(crate) fn apply_update_request(paste: &mut Paste, update: &UpdatePasteRequest) {
    let mut content_changed = false;

    if let Some(content) = &update.content {
        paste.content = content.clone();
        paste.is_markdown = is_markdown_content(&paste.content);
        content_changed = true;
    }
    if let Some(name) = &update.name {
        paste.name = name.clone();
    }
    if let Some(language) = &update.language {
        paste.language = Some(language.clone());
        if update.language_is_manual.is_none() {
            paste.language_is_manual = true;
        }
    }
    if let Some(is_manual) = update.language_is_manual {
        paste.language_is_manual = is_manual;
    }
    // Explicit auto toggle clears any previously locked classification so
    // auto state only reflects "unresolved/pending detection".
    if update.language_is_manual == Some(false) && update.language.is_none() && !content_changed {
        paste.language = None;
    }
    let should_auto_detect =
        update.language.is_none() && !paste.language_is_manual && content_changed;
    if should_auto_detect {
        let detected = detect_language(&paste.content);
        paste.language = detected;
        if paste.language.is_some() {
            // Auto mode is one-shot: once we classify a concrete language, lock
            // it until the user explicitly switches back to auto.
            paste.language_is_manual = true;
        }
    }

    if let Some(ref fid) = update.folder_id {
        paste.folder_id = if fid.is_empty() {
            None
        } else {
            Some(fid.clone())
        };
    }
    if let Some(tags) = &update.tags {
        paste.tags = tags.clone();
    }

    paste.updated_at = Utc::now();
}

/// Returns `true` when a paste language satisfies the provided filter.
///
/// Both values are canonicalized first so aliases such as `cs`/`csharp` match.
///
/// # Arguments
/// - `language`: Persisted language label on the row, if any.
/// - `filter`: User-selected language filter, if any.
///
/// # Returns
/// `true` when no filter is set or when canonicalized labels match.
pub(super) fn language_matches_filter(language: Option<&str>, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let canonical_filter = crate::detection::canonical::canonicalize(filter);
    language
        .map(crate::detection::canonical::canonicalize)
        .filter(|value| !value.is_empty())
        .map(|value| value == canonical_filter)
        .unwrap_or(false)
}

/// Returns `true` when metadata matches both folder and language filters.
///
/// # Arguments
/// - `meta`: Metadata row under evaluation.
/// - `folder_filter`: Optional folder id filter.
/// - `language_filter`: Optional language filter.
///
/// # Returns
/// `true` when all provided filters match.
pub(super) fn meta_matches_filters(
    meta: &PasteMeta,
    folder_filter: Option<&str>,
    language_filter: Option<&str>,
) -> bool {
    if let Some(folder_id) = folder_filter {
        if meta.folder_id.as_deref() != Some(folder_id) {
            return false;
        }
    }
    language_matches_filter(meta.language.as_deref(), language_filter)
}

/// Scores a metadata row for search ranking.
///
/// Higher values indicate a stronger match against name, tags, and language.
///
/// # Arguments
/// - `meta`: Metadata row to score.
/// - `query_lower`: Lowercased search query.
///
/// # Returns
/// A non-negative score used for top-k ordering.
pub(super) fn score_meta_match(meta: &PasteMeta, query_lower: &str) -> i32 {
    let mut score = 0;
    let canonical_query = crate::detection::canonical::canonicalize(query_lower);
    if meta.name.to_lowercase().contains(query_lower) {
        score += 10;
    }
    if meta
        .tags
        .iter()
        .any(|tag| tag.to_lowercase().contains(query_lower))
    {
        score += 5;
    }
    if meta
        .language
        .as_ref()
        .map(|lang| {
            let language_lower = lang.to_lowercase();
            let canonical_language = crate::detection::canonical::canonicalize(lang);
            language_lower.contains(query_lower)
                || canonical_language.contains(query_lower)
                || (!canonical_query.is_empty()
                    && (language_lower == canonical_query || canonical_language == canonical_query))
        })
        .unwrap_or(false)
    {
        score += 2;
    }
    score
}

/// Scores a full paste row for search ranking.
///
/// Name and tag matches are weighted above content matches.
///
/// # Arguments
/// - `paste`: Paste row to score.
/// - `query_lower`: Lowercased search query.
///
/// # Returns
/// A non-negative score used for top-k ordering.
pub(super) fn score_paste_match(paste: &Paste, query_lower: &str) -> i32 {
    let mut score = 0;
    if contains_case_insensitive(&paste.name, query_lower) {
        score += 10;
    }
    if paste
        .tags
        .iter()
        .any(|tag| contains_case_insensitive(tag, query_lower))
    {
        score += 5;
    }
    if contains_case_insensitive(&paste.content, query_lower) {
        score += 1;
    }
    score
}

/// Adds a metadata candidate into a bounded top-k ranking set.
///
/// # Arguments
/// - `results`: Mutable top-k working set.
/// - `candidate`: Candidate row with `(score, updated_at, meta)`.
/// - `limit`: Maximum number of rows retained.
pub(super) fn push_ranked_meta_top_k(
    results: &mut Vec<(i32, DateTime<Utc>, PasteMeta)>,
    candidate: (i32, DateTime<Utc>, PasteMeta),
    limit: usize,
) {
    push_ranked_top_k(results, candidate, limit);
}

fn push_ranked_top_k<T>(
    results: &mut Vec<(i32, DateTime<Utc>, T)>,
    candidate: (i32, DateTime<Utc>, T),
    limit: usize,
) {
    if limit == 0 {
        return;
    }
    if results.len() < limit {
        results.push(candidate);
        return;
    }

    let Some((worst_idx, worst_entry)) = results
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
    else {
        results.push(candidate);
        return;
    };

    let candidate_better = candidate.0 > worst_entry.0
        || (candidate.0 == worst_entry.0 && candidate.1 > worst_entry.1);
    if candidate_better {
        results[worst_idx] = candidate;
    }
}

/// Sorts ranked metadata candidates and returns the highest scoring rows.
///
/// # Arguments
/// - `ranked_results`: Unordered ranking tuples.
/// - `limit`: Maximum number of metadata rows to return.
///
/// # Returns
/// Metadata rows sorted by score then recency.
pub(super) fn finalize_meta_search_results(
    mut ranked_results: Vec<(i32, DateTime<Utc>, PasteMeta)>,
    limit: usize,
) -> Vec<PasteMeta> {
    ranked_results.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    ranked_results
        .into_iter()
        .take(limit)
        .map(|(_, _, meta)| meta)
        .collect()
}

fn contains_case_insensitive(haystack: &str, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    if query_lower.is_ascii() {
        let needle = query_lower.as_bytes();
        let hay = haystack.as_bytes();
        if needle.len() > hay.len() {
            return false;
        }
        for idx in 0..=hay.len() - needle.len() {
            if hay[idx..idx + needle.len()]
                .iter()
                .map(u8::to_ascii_lowercase)
                .eq(needle.iter().copied())
            {
                return true;
            }
        }
        return false;
    }
    haystack.to_lowercase().contains(query_lower)
}

/// Returns `true` when a paste's current folder assignment matches expectation.
///
/// # Arguments
/// - `current_folder_id`: Current folder id on the paste.
/// - `expected_folder_id`: Expected folder id for the operation.
///
/// # Returns
/// `true` when both optional folder ids are equal.
pub(super) fn folder_matches_expected(
    current_folder_id: Option<&str>,
    expected_folder_id: Option<&str>,
) -> bool {
    current_folder_id == expected_folder_id
}

/// Deserializes a [`Paste`] row, with compatibility for legacy serialized rows.
///
/// # Returns
/// A decoded [`Paste`] value.
///
/// # Errors
/// Returns the primary deserialization error when neither current nor legacy
/// wire formats can be decoded.
pub(crate) fn deserialize_paste(bytes: &[u8]) -> Result<Paste, bincode::Error> {
    bincode::deserialize::<Paste>(bytes).or_else(|err| {
        bincode::deserialize::<LegacyPaste>(bytes)
            .map(Paste::from)
            .map_err(|_| err)
    })
}

/// Deserializes a [`PasteMeta`] row from storage bytes.
///
/// # Returns
/// A decoded [`PasteMeta`] value.
///
/// # Errors
/// Returns a bincode error when the row bytes are malformed or incompatible.
pub(super) fn deserialize_meta(bytes: &[u8]) -> Result<PasteMeta, bincode::Error> {
    bincode::deserialize(bytes)
}

#[derive(Serialize, Deserialize)]
struct LegacyPaste {
    id: String,
    name: String,
    content: String,
    language: Option<String>,
    folder_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    tags: Vec<String>,
    is_markdown: bool,
}

impl From<LegacyPaste> for Paste {
    fn from(old: LegacyPaste) -> Self {
        let LegacyPaste {
            id,
            name,
            content,
            language,
            folder_id,
            created_at,
            updated_at,
            tags,
            is_markdown,
        } = old;
        Self {
            id,
            name,
            content,
            language,
            // Legacy rows predate persisted manual intent. Keep migration deterministic
            // and cheap by defaulting to auto-detect mode instead of re-running detector
            // logic during deserialization.
            language_is_manual: false,
            folder_id,
            created_at,
            updated_at,
            tags,
            is_markdown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{reverse_timestamp_key, score_meta_match, LegacyPaste, Paste};
    use crate::models::paste::PasteMeta;
    use chrono::{TimeZone, Utc};

    #[test]
    fn legacy_language_manual_flag_migrates_in_auto_mode() {
        let legacy_cases = [
            (
                "legacy-id",
                "legacy",
                "pub fn main() {\n    let x = 1;\n    println!(\"hello\");\n}",
                Some("rust"),
            ),
            ("legacy-id-2", "legacy-2", "fn main() {}", Some("python")),
        ];

        for (id, name, content, language) in legacy_cases {
            let legacy = LegacyPaste {
                id: id.to_string(),
                name: name.to_string(),
                content: content.to_string(),
                language: language.map(str::to_string),
                folder_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                tags: Vec::new(),
                is_markdown: false,
            };
            let migrated: Paste = legacy.into();
            assert!(!migrated.language_is_manual);
        }
    }

    #[test]
    fn reverse_timestamp_key_clamps_pre_epoch_values() {
        let pre_epoch = Utc
            .with_ymd_and_hms(1960, 1, 1, 0, 0, 0)
            .single()
            .expect("valid timestamp");
        assert_eq!(reverse_timestamp_key(pre_epoch), u64::MAX);
    }

    #[test]
    fn language_filter_aliases_match_canonical_values() {
        assert!(super::language_matches_filter(Some("csharp"), Some("cs")));
        assert!(super::language_matches_filter(Some("cs"), Some("csharp")));
        assert!(super::language_matches_filter(Some("bash"), Some("shell")));
        assert!(super::language_matches_filter(
            Some("pwsh"),
            Some("powershell")
        ));
    }

    #[test]
    fn search_language_scoring_respects_aliases() {
        let base = PasteMeta {
            id: "id-1".to_string(),
            name: "sample".to_string(),
            language: None,
            folder_id: None,
            updated_at: Utc::now(),
            tags: Vec::new(),
            content_len: 10,
            is_markdown: false,
        };

        let cs_meta = PasteMeta {
            language: Some("cs".to_string()),
            ..base.clone()
        };
        let csharp_meta = PasteMeta {
            language: Some("csharp".to_string()),
            ..base.clone()
        };
        let css_meta = PasteMeta {
            language: Some("css".to_string()),
            ..base
        };

        assert_eq!(score_meta_match(&cs_meta, "csharp"), 2);
        assert_eq!(score_meta_match(&csharp_meta, "cs"), 2);
        assert_eq!(score_meta_match(&css_meta, "csharp"), 0);
    }
}
