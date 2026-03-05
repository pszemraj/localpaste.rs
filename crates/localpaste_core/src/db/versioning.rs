//! Shared helpers for persisted paste-version snapshots.

use crate::error::AppError;
use crate::models::paste::VersionMeta;
use chrono::{DateTime, Utc};
use serde::Deserialize;

fn version_id_from_millis(ms: i64) -> u64 {
    ms.max(0) as u64
}

/// Build a deterministic content hash for snapshot de-duplication.
///
/// # Arguments
/// - `content`: Snapshot content to hash.
///
/// # Returns
/// Lowercase BLAKE3 hex digest.
pub(crate) fn content_hash_hex(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

/// Build a version metadata row for a content snapshot at `now`.
///
/// # Arguments
/// - `content`: Snapshot content.
/// - `now`: Snapshot timestamp.
///
/// # Returns
/// A [`VersionMeta`] row for `content`.
pub(crate) fn version_meta_for_content(
    content: &str,
    language: Option<&str>,
    language_is_manual: bool,
    now: DateTime<Utc>,
) -> VersionMeta {
    VersionMeta {
        version_id_ms: version_id_from_millis(now.timestamp_millis()),
        created_at: now,
        content_hash: content_hash_hex(content),
        len: content.len(),
        language: language.map(ToString::to_string),
        language_is_manual,
    }
}

/// Build a monotonic version metadata row for a content snapshot at `now`.
///
/// When multiple snapshots are recorded within the same millisecond, this helper
/// ensures `version_id_ms` remains strictly increasing relative to `latest`.
///
/// # Arguments
/// - `content`: Snapshot content.
/// - `now`: Snapshot timestamp.
/// - `latest`: Most-recent persisted version metadata, if any.
///
/// # Returns
/// A monotonic [`VersionMeta`] suitable for persistence.
pub(crate) fn next_version_meta_for_content(
    content: &str,
    language: Option<&str>,
    language_is_manual: bool,
    now: DateTime<Utc>,
    latest: Option<&VersionMeta>,
) -> VersionMeta {
    let mut next = version_meta_for_content(content, language, language_is_manual, now);
    if let Some(latest) = latest {
        if next.version_id_ms <= latest.version_id_ms {
            next.version_id_ms = latest.version_id_ms.saturating_add(1);
        }
    }
    next
}

/// Decode serialized version metadata list bytes.
///
/// # Arguments
/// - `bytes`: Serialized `Vec<VersionMeta>` bytes, or `None` when absent.
///
/// # Returns
/// Decoded metadata rows, or an empty vector when `bytes` is `None`.
///
/// # Errors
/// Returns an error when bytes are malformed/incompatible.
pub(crate) fn decode_version_meta_list(bytes: Option<&[u8]>) -> Result<Vec<VersionMeta>, AppError> {
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    if let Ok(items) = bincode::deserialize::<Vec<VersionMeta>>(bytes) {
        return Ok(items);
    }
    let legacy_items = bincode::deserialize::<Vec<LegacyVersionMeta>>(bytes)?;
    Ok(legacy_items.into_iter().map(VersionMeta::from).collect())
}

/// Encode a version metadata list for persistence.
///
/// # Arguments
/// - `items`: Version metadata rows to serialize.
///
/// # Returns
/// Serialized bytes suitable for redb storage.
///
/// # Errors
/// Returns an error when serialization fails.
pub(crate) fn encode_version_meta_list(items: &[VersionMeta]) -> Result<Vec<u8>, AppError> {
    Ok(bincode::serialize(items)?)
}

/// Returns whether a new version should be persisted.
///
/// # Arguments
/// - `latest`: Most recent persisted version metadata.
/// - `next`: Candidate next metadata row.
/// - `min_interval_secs`: Minimum elapsed seconds required between snapshots.
///
/// # Returns
/// `true` when `next` should be persisted.
pub(crate) fn should_record_version(
    latest: Option<&VersionMeta>,
    next: &VersionMeta,
    min_interval_secs: u64,
) -> bool {
    let Some(latest) = latest else {
        return true;
    };
    if latest.content_hash == next.content_hash {
        return false;
    }

    let elapsed_secs = next
        .created_at
        .signed_duration_since(latest.created_at)
        .num_seconds();
    elapsed_secs >= min_interval_secs as i64
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyVersionMeta {
    version_id_ms: u64,
    created_at: DateTime<Utc>,
    content_hash: String,
    len: usize,
}

impl From<LegacyVersionMeta> for VersionMeta {
    fn from(value: LegacyVersionMeta) -> Self {
        Self {
            version_id_ms: value.version_id_ms,
            created_at: value.created_at,
            content_hash: value.content_hash,
            len: value.len,
            language: None,
            language_is_manual: false,
        }
    }
}
