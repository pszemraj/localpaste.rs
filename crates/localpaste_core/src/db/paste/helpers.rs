//! Helper functions shared by paste storage operations.

use crate::models::paste::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(crate) fn reverse_timestamp_key(updated_at: DateTime<Utc>) -> u64 {
    // Pre-epoch timestamps are clamped to preserve total ordering semantics for
    // expected runtime data while avoiding negative->u64 underflow.
    let millis = updated_at.timestamp_millis().max(0) as u64;
    u64::MAX.saturating_sub(millis)
}

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
    let should_auto_detect = update.language.is_none()
        && !paste.language_is_manual
        && (content_changed || update.language_is_manual == Some(false));
    if should_auto_detect {
        paste.language = detect_language(&paste.content);
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

pub(super) fn folder_matches_expected(
    current_folder_id: Option<&str>,
    expected_folder_id: Option<&str>,
) -> bool {
    current_folder_id == expected_folder_id
}

pub(crate) fn deserialize_paste(bytes: &[u8]) -> Result<Paste, bincode::Error> {
    bincode::deserialize::<Paste>(bytes).or_else(|err| {
        bincode::deserialize::<LegacyPaste>(bytes)
            .map(Paste::from)
            .map_err(|_| err)
    })
}

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
        let language_is_manual =
            infer_legacy_language_is_manual(content.as_str(), language.as_deref());
        Self {
            id,
            name,
            content,
            language,
            language_is_manual,
            folder_id,
            created_at,
            updated_at,
            tags,
            is_markdown,
        }
    }
}

fn infer_legacy_language_is_manual(content: &str, stored_language: Option<&str>) -> bool {
    let Some(stored) = stored_language
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let stored = crate::detection::canonical::canonicalize(stored);
    let inferred = detect_language(content);
    inferred
        .as_deref()
        .map(|value| value != stored.as_str())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::{
        infer_legacy_language_is_manual, reverse_timestamp_key, score_meta_match, LegacyPaste,
        Paste,
    };
    use crate::models::paste::PasteMeta;
    use chrono::{TimeZone, Utc};

    #[test]
    fn legacy_language_manual_flag_preserves_auto_detected_values() {
        let legacy = LegacyPaste {
            id: "legacy-id".to_string(),
            name: "legacy".to_string(),
            content: "pub fn main() {\n    let x = 1;\n    println!(\"hello\");\n}".to_string(),
            language: Some("rust".to_string()),
            folder_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            tags: Vec::new(),
            is_markdown: false,
        };
        let migrated: Paste = legacy.into();
        assert!(!migrated.language_is_manual);
    }

    #[test]
    fn legacy_language_manual_flag_marks_divergent_language_as_manual() {
        assert!(infer_legacy_language_is_manual(
            "fn main() {}",
            Some("python")
        ));
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
