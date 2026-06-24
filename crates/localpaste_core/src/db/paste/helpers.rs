//! Helper functions shared by paste storage operations.

use crate::db::versioning::{
    decode_version_meta_list, encode_version_meta_list, prune_version_meta_to_limit_preserving,
};
use crate::error::AppError;
use crate::models::paste::*;
use crate::semantic::{DerivedMeta, PasteKind};
use chrono::{DateTime, Utc};
use redb::ReadableTable;
use serde::de::DeserializeOwned;
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

/// Removes all historical version rows for a paste when undo payloads fit a cap.
///
/// This preflights serialized content sizes before removing rows, so callers can
/// fall back to non-undo deletion without deserializing an unbounded history.
///
/// # Arguments
/// - `versions_meta`: Open mutable version metadata table.
/// - `versions_content`: Open mutable version content table.
/// - `paste_id`: Paste id whose version rows should be removed.
/// - `max_payload_bytes`: Optional cap for serialized version content bytes.
///
/// # Returns
/// `Some` deleted version metadata/content pairs in stored metadata order, or
/// `None` when the payload would exceed `max_payload_bytes` or capped undo
/// cannot safely include every historical content row. When `None` is returned,
/// no version rows have been removed.
///
/// # Errors
/// Returns an error when storage access, metadata decoding, content decoding, or
/// uncapped content/meta consistency checks fail.
#[cfg(test)]
pub(crate) fn remove_paste_versions_for_delete_capped(
    versions_meta: &mut redb::Table<&str, &[u8]>,
    versions_content: &mut redb::Table<(&str, u64), &[u8]>,
    paste_id: &str,
    max_payload_bytes: Option<usize>,
) -> Result<Option<Vec<DeletedPasteVersion>>, AppError> {
    let version_items = decode_version_meta_list(
        versions_meta
            .get(paste_id)?
            .as_ref()
            .map(|value| value.value()),
    )?;
    if let Some(max_payload_bytes) = max_payload_bytes {
        let mut payload_bytes = 0usize;
        for version in &version_items {
            let Some(content_guard) = versions_content.get((paste_id, version.version_id_ms))?
            else {
                return Ok(None);
            };
            payload_bytes = payload_bytes.saturating_add(content_guard.value().len());
            if payload_bytes > max_payload_bytes {
                return Ok(None);
            }
        }
    }

    let mut versions = Vec::with_capacity(version_items.len());
    for version in version_items {
        let content = versions_content
            .remove((paste_id, version.version_id_ms))?
            .map(|guard| bincode::deserialize::<String>(guard.value()))
            .transpose()?
            .ok_or_else(|| {
                AppError::StorageMessage(format!(
                    "Missing version content for paste '{}' version {}",
                    paste_id, version.version_id_ms
                ))
            })?;
        versions.push(DeletedPasteVersion {
            meta: version,
            content,
        });
    }
    let _ = versions_meta.remove(paste_id)?;
    Ok(Some(versions))
}

/// Removes all historical version rows for a paste without loading contents.
///
/// This is the non-undo delete path. It only needs the metadata row to discover
/// version ids, then removes matching content rows opportunistically.
///
/// # Arguments
/// - `versions_meta`: Open mutable version metadata table.
/// - `versions_content`: Open mutable version content table.
/// - `paste_id`: Paste id whose version rows should be removed.
///
/// # Returns
/// `Ok(())` when all reachable version rows have been removed.
///
/// # Errors
/// Returns an error when storage access or metadata decoding fails.
pub(crate) fn discard_paste_versions_for_delete(
    versions_meta: &mut redb::Table<&str, &[u8]>,
    versions_content: &mut redb::Table<(&str, u64), &[u8]>,
    paste_id: &str,
) -> Result<(), AppError> {
    let version_items = decode_version_meta_list(
        versions_meta
            .get(paste_id)?
            .as_ref()
            .map(|value| value.value()),
    )?;
    for version in version_items {
        let _ = versions_content.remove((paste_id, version.version_id_ms))?;
    }
    let _ = versions_meta.remove(paste_id)?;
    Ok(())
}

/// Prune version metadata/content rows to the configured retention limit and persist metadata.
///
/// # Arguments
/// - `versions_meta`: Open mutable version metadata table.
/// - `versions_content`: Open mutable version content table.
/// - `paste_id`: Paste id whose version rows should be pruned.
/// - `version_items`: Newest-first version metadata rows to prune in place.
/// - `retention_limit`: Maximum number of newest snapshots to retain.
/// - `protected_version_id_ms`: Optional version id that must survive pruning.
///
/// # Returns
/// `Ok(())` after pruned content rows are removed and metadata is persisted.
///
/// # Errors
/// Returns an error when content removal, metadata encoding, or metadata persistence fails.
pub(crate) fn prune_and_persist_version_meta(
    versions_meta: &mut redb::Table<&str, &[u8]>,
    versions_content: &mut redb::Table<(&str, u64), &[u8]>,
    paste_id: &str,
    version_items: &mut Vec<VersionMeta>,
    retention_limit: usize,
    protected_version_id_ms: Option<u64>,
) -> Result<(), AppError> {
    for pruned in prune_version_meta_to_limit_preserving(
        version_items,
        retention_limit,
        protected_version_id_ms,
    ) {
        let _ = versions_content.remove((paste_id, pruned.version_id_ms))?;
    }
    let encoded_versions = encode_version_meta_list(version_items)?;
    versions_meta.insert(paste_id, encoded_versions.as_slice())?;
    Ok(())
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
    let was_manual_before_update = paste.language_is_manual;

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
    // Explicit manual->auto toggle clears previously locked classification so
    // auto state only reflects "unresolved/pending detection".
    //
    // When the row is already auto-managed (`language_is_manual == false`),
    // metadata-only updates intentionally preserve an existing resolved
    // language value (legacy compatibility and no-surprise saves).
    let switched_manual_to_auto =
        was_manual_before_update && update.language_is_manual == Some(false);
    if switched_manual_to_auto && update.language.is_none() && !content_changed {
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
/// - `query`: Search query.
/// - `case_sensitive`: Whether string matching must preserve case.
///
/// # Returns
/// A non-negative score used for top-k ordering.
pub(super) fn score_meta_match(meta: &PasteMeta, query: &str, case_sensitive: bool) -> i32 {
    score_meta_match_with_options(meta, query, case_sensitive, true)
}

fn score_literal_meta_match(meta: &PasteMeta, query: &str, case_sensitive: bool) -> i32 {
    score_meta_match_with_options(meta, query, case_sensitive, false)
}

fn score_meta_match_with_options(
    meta: &PasteMeta,
    query: &str,
    case_sensitive: bool,
    include_derived: bool,
) -> i32 {
    let query = query.trim();
    if query.is_empty() {
        return 0;
    }

    let mut score = 0;
    let query_lower = query.to_lowercase();
    let query_for_match = if case_sensitive {
        query
    } else {
        query_lower.as_str()
    };
    let canonical_query = crate::detection::canonical::canonicalize(query_lower.as_str());
    let name_for_match = if case_sensitive {
        meta.name.as_str().to_string()
    } else {
        meta.name.to_lowercase()
    };
    let handle_for_match = if include_derived {
        meta.derived
            .handle
            .as_deref()
            .unwrap_or_default()
            .to_string()
    } else {
        String::new()
    };
    let derived_terms_for_match: Vec<String> = if include_derived {
        meta.derived
            .terms
            .iter()
            .map(|term| {
                if case_sensitive {
                    term.clone()
                } else {
                    term.to_ascii_lowercase()
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let handle_for_match = if case_sensitive {
        handle_for_match
    } else {
        handle_for_match.to_ascii_lowercase()
    };
    let tag_candidates: Vec<String> = meta
        .tags
        .iter()
        .map(|tag| {
            if case_sensitive {
                tag.clone()
            } else {
                tag.to_lowercase()
            }
        })
        .collect();
    let query_terms = if case_sensitive {
        split_meta_query_terms(query)
    } else {
        split_meta_query_terms(query_lower.as_str())
    };
    let mut matched_query_terms = 0;

    if name_for_match.contains(query_for_match) {
        score += 12;
    }
    if include_derived && !handle_for_match.is_empty() && handle_for_match.contains(query_for_match)
    {
        score += 10;
    }
    if include_derived
        && derived_terms_for_match
            .iter()
            .any(|term| term.contains(query_for_match))
    {
        score += 8;
    }
    if tag_candidates
        .iter()
        .any(|tag| tag.contains(query_for_match))
    {
        score += 5;
    }
    if language_matches_query(
        meta.language.as_deref(),
        query_for_match,
        canonical_query.as_str(),
        case_sensitive,
    ) {
        score += 2;
    }
    if include_derived && kind_matches_query(meta.derived.kind, query_for_match, case_sensitive) {
        score += 3;
    }

    // Metadata search still uses derived semantic hints so multi-term retrieval
    // stays useful without scanning full content in hot paths.
    for term in &query_terms {
        let mut term_matched = false;

        if name_for_match.contains(*term) {
            score += 3;
            term_matched = true;
        }
        if include_derived && !handle_for_match.is_empty() && handle_for_match.contains(*term) {
            score += 4;
            term_matched = true;
        }
        if include_derived
            && derived_terms_for_match
                .iter()
                .any(|candidate| candidate.contains(*term))
        {
            score += 3;
            term_matched = true;
        }
        if tag_candidates.iter().any(|tag| tag.contains(*term)) {
            score += 2;
            term_matched = true;
        }
        if include_derived && kind_matches_query(meta.derived.kind, term, case_sensitive) {
            score += 1;
            term_matched = true;
        }
        if language_matches_query(meta.language.as_deref(), term, term, case_sensitive) {
            score += 1;
            term_matched = true;
        }

        if term_matched {
            matched_query_terms += 1;
        }
    }

    if matched_query_terms > 1 {
        // Multi-term retrieval should prefer rows that cover more of the
        // query intent across metadata signals instead of over-rewarding one
        // strong partial match.
        score += matched_query_terms * 2;
    }

    score
}

fn split_meta_query_terms(query_lower: &str) -> Vec<&str> {
    let mut terms = Vec::new();
    for term in query_lower.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
    {
        if term.len() < 2 || terms.contains(&term) {
            continue;
        }
        terms.push(term);
    }
    terms
}

fn language_matches_query(
    language: Option<&str>,
    query: &str,
    canonical_query: &str,
    case_sensitive: bool,
) -> bool {
    language
        .map(|lang| {
            if case_sensitive {
                return lang.contains(query);
            }
            let language_lower = lang.to_lowercase();
            let canonical_language = crate::detection::canonical::canonicalize(lang);
            language_lower.contains(query)
                || canonical_language.contains(query)
                || (!canonical_query.is_empty()
                    && (language_lower == canonical_query || canonical_language == canonical_query))
        })
        .unwrap_or(false)
}

fn kind_matches_query(kind: PasteKind, query: &str, case_sensitive: bool) -> bool {
    if kind == PasteKind::Other {
        return false;
    }
    let query = query.trim();
    if case_sensitive {
        kind.label().contains(query)
    } else {
        kind.label().to_ascii_lowercase().contains(query)
    }
}

/// Scores a full paste row for search ranking.
///
/// Metadata-derived matches are weighted above full-content matches so canonical
/// search keeps the same retrieval surface as metadata search while adding body
/// substring hits.
///
/// # Arguments
/// - `paste`: Paste row to score.
/// - `meta`: Metadata projection for the same paste.
/// - `query`: Search query.
/// - `case_sensitive`: Whether string matching must preserve case.
///
/// # Returns
/// A non-negative score used for top-k ordering.
pub(super) fn score_paste_match(
    paste: &Paste,
    meta: &PasteMeta,
    query: &str,
    case_sensitive: bool,
) -> i32 {
    let query = query.trim();
    if query.is_empty() {
        return 0;
    }
    let query_lower;
    let query_for_match = if case_sensitive {
        query
    } else {
        query_lower = query.to_lowercase();
        query_lower.as_str()
    };
    let content_matches = contains_search(&paste.content, query_for_match, case_sensitive);
    let meta_score = if content_matches || case_sensitive {
        // Avoid counting the same body hit as both raw content and derived
        // content terms. Case-sensitive full-content search also avoids
        // normalized body-derived terms when the raw body did not match.
        score_literal_meta_match(meta, query, case_sensitive)
    } else {
        score_meta_match(meta, query, case_sensitive)
    };
    let mut score = meta_score.saturating_mul(10);
    if content_matches {
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

fn contains_search(haystack: &str, query: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        haystack.contains(query)
    } else {
        contains_case_insensitive(haystack, query)
    }
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
    deserialize_current_or_legacy::<Paste, LegacyPaste>(bytes, Paste::from)
}

/// Deserializes a [`PasteMeta`] row from storage bytes.
///
/// # Returns
/// A decoded [`PasteMeta`] value.
///
/// # Errors
/// Returns a bincode error when the row bytes are malformed or incompatible.
pub(super) fn deserialize_meta(bytes: &[u8]) -> Result<PasteMeta, bincode::Error> {
    deserialize_current_or_legacy::<PasteMeta, LegacyPasteMeta>(bytes, PasteMeta::from)
}

fn deserialize_current_or_legacy<T, L>(
    bytes: &[u8],
    upgrade_legacy: impl FnOnce(L) -> T,
) -> Result<T, bincode::Error>
where
    T: DeserializeOwned,
    L: DeserializeOwned,
{
    bincode::deserialize::<T>(bytes).or_else(|err| {
        bincode::deserialize::<L>(bytes)
            .map(upgrade_legacy)
            .map_err(|_| err)
    })
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

#[derive(Serialize, Deserialize)]
struct LegacyPasteMeta {
    id: String,
    name: String,
    language: Option<String>,
    folder_id: Option<String>,
    updated_at: DateTime<Utc>,
    tags: Vec<String>,
    content_len: usize,
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
            language_is_manual: false,
            folder_id,
            created_at,
            updated_at,
            tags,
            is_markdown,
        }
    }
}

impl From<LegacyPasteMeta> for PasteMeta {
    fn from(old: LegacyPasteMeta) -> Self {
        let LegacyPasteMeta {
            id,
            name,
            language,
            folder_id,
            updated_at,
            tags,
            content_len,
            is_markdown,
        } = old;
        Self {
            id,
            name,
            language,
            folder_id,
            updated_at,
            tags,
            content_len,
            is_markdown,
            derived: DerivedMeta::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{reverse_timestamp_key, score_meta_match, split_meta_query_terms};
    use crate::models::paste::PasteMeta;
    use crate::semantic::{DerivedMeta, PasteKind};
    use chrono::{TimeZone, Utc};

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
            derived: DerivedMeta::default(),
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

        assert_eq!(score_meta_match(&cs_meta, "csharp", false), 2);
        assert_eq!(score_meta_match(&csharp_meta, "cs", false), 3);
        assert_eq!(score_meta_match(&css_meta, "csharp", false), 0);
    }

    #[test]
    fn score_meta_match_prefers_handle_then_terms_then_tags_then_language() {
        let base = PasteMeta {
            id: "id-1".to_string(),
            name: "random-slug".to_string(),
            language: None,
            folder_id: None,
            updated_at: Utc::now(),
            tags: Vec::new(),
            content_len: 10,
            is_markdown: false,
            derived: DerivedMeta::default(),
        };

        let by_handle = PasteMeta {
            derived: DerivedMeta {
                kind: PasteKind::Code,
                handle: Some("cargo test".to_string()),
                terms: Vec::new(),
            },
            ..base.clone()
        };
        let by_terms = PasteMeta {
            derived: DerivedMeta {
                kind: PasteKind::Code,
                handle: None,
                terms: vec!["cargo".to_string(), "test".to_string()],
            },
            ..base.clone()
        };
        let by_tag = PasteMeta {
            tags: vec!["cargo-test".to_string()],
            ..base.clone()
        };
        let by_language = PasteMeta {
            language: Some("test".to_string()),
            ..base
        };

        let handle_score = score_meta_match(&by_handle, "cargo test", false);
        let term_score = score_meta_match(&by_terms, "cargo test", false);
        let tag_score = score_meta_match(&by_tag, "cargo test", false);
        let language_score = score_meta_match(&by_language, "test", false);

        assert!(handle_score > term_score);
        assert!(term_score > tag_score);
        assert!(tag_score > language_score);
    }

    #[test]
    fn split_meta_query_terms_dedupes_and_skips_short_tokens() {
        assert_eq!(
            split_meta_query_terms(" cargo test  c c cargo "),
            vec!["cargo", "test"]
        );
        assert_eq!(
            split_meta_query_terms("docker-compose postgres"),
            vec!["docker-compose", "postgres"]
        );
    }
}
