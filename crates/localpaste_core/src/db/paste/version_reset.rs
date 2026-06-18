//! Hard-reset operations for paste version history.

use super::{deserialize_paste, reverse_timestamp_key, PasteDb};
use crate::{
    db::{
        tables::*,
        versioning::{
            decode_version_meta_list, encode_version_meta_list, next_version_meta_for_content,
        },
    },
    error::AppError,
    models::paste::{is_markdown_content, Paste, PasteMeta},
    validation::ensure_paste_content_size,
};
use chrono::Utc;
use redb::ReadableTable;

impl PasteDb {
    /// Reset current paste content to a historical version and prune newer snapshots.
    ///
    /// # Arguments
    /// - `paste_id`: Canonical paste id.
    /// - `version_id_ms`: Target historical version id.
    /// - `max_paste_size`: Maximum allowed content size for the restored head row.
    ///
    /// # Returns
    /// `Ok(Some(updated))` when reset succeeds, `Ok(None)` when paste/version is missing.
    ///
    /// # Errors
    /// Returns an error when storage access or serialization fails.
    pub fn reset_hard_to_version(
        &self,
        paste_id: &str,
        version_id_ms: u64,
        max_paste_size: usize,
    ) -> Result<Option<Paste>, AppError> {
        self.reset_hard_to_version_inner(paste_id, version_id_ms, max_paste_size, false)
    }

    /// Reset current paste content while archiving the outgoing head first.
    ///
    /// This is for GUI save-and-reset flows where dirty editor content was just
    /// persisted as the current head and must remain recoverable after reset.
    /// Normal API hard resets should call [`Self::reset_hard_to_version`].
    ///
    /// # Arguments
    /// - `paste_id`: Canonical paste id.
    /// - `version_id_ms`: Target historical version id.
    /// - `max_paste_size`: Maximum allowed content size for the restored head row.
    ///
    /// # Returns
    /// `Ok(Some(updated))` when reset succeeds, `Ok(None)` when paste/version is missing.
    ///
    /// # Errors
    /// Returns an error when storage access or serialization fails.
    pub fn reset_hard_to_version_preserving_current_head(
        &self,
        paste_id: &str,
        version_id_ms: u64,
        max_paste_size: usize,
    ) -> Result<Option<Paste>, AppError> {
        self.reset_hard_to_version_inner(paste_id, version_id_ms, max_paste_size, true)
    }

    fn reset_hard_to_version_inner(
        &self,
        paste_id: &str,
        version_id_ms: u64,
        max_paste_size: usize,
        preserve_current_head: bool,
    ) -> Result<Option<Paste>, AppError> {
        let write_txn = self.db.begin_write()?;
        let updated_paste = {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;
            let mut versions_meta = write_txn.open_table(PASTE_VERSIONS_META)?;
            let mut versions_content = write_txn.open_table(PASTE_VERSIONS_CONTENT)?;

            let Some(paste_guard) = pastes.get(paste_id)? else {
                return Ok(None);
            };
            let mut paste = deserialize_paste(paste_guard.value())?;
            let old_recency_key = reverse_timestamp_key(paste.updated_at);
            let old_content = paste.content.clone();
            let old_language = paste.language.clone();
            let old_language_is_manual = paste.language_is_manual;
            drop(paste_guard);

            let mut version_items = decode_version_meta_list(
                versions_meta
                    .get(paste_id)?
                    .as_ref()
                    .map(|value| value.value()),
            )?;
            let Some(target_meta) = version_items
                .iter()
                .find(|item| item.version_id_ms == version_id_ms)
                .cloned()
            else {
                return Ok(None);
            };

            let Some(content_guard) = versions_content.get((paste_id, version_id_ms))? else {
                return Ok(None);
            };
            let target_content: String = bincode::deserialize(content_guard.value())?;
            drop(content_guard);
            ensure_paste_content_size(&target_content, max_paste_size)?;

            let reset_at = Utc::now();
            let current_head_matches_target = old_content == target_content
                && old_language.as_deref() == target_meta.language.as_deref()
                && old_language_is_manual == target_meta.language_is_manual;
            let preserved_current_head_version_id =
                if preserve_current_head && !current_head_matches_target {
                    let latest = version_items.first();
                    let current_head_meta = next_version_meta_for_content(
                        old_content.as_str(),
                        old_language.as_deref(),
                        old_language_is_manual,
                        reset_at,
                        latest,
                    );
                    let encoded_content = bincode::serialize(&old_content)?;
                    versions_content.insert(
                        (paste_id, current_head_meta.version_id_ms),
                        encoded_content.as_slice(),
                    )?;
                    let version_id = current_head_meta.version_id_ms;
                    version_items.insert(0, current_head_meta);
                    Some(version_id)
                } else {
                    None
                };

            // Reset must restore the exact stored snapshot semantics. Reusing
            // `apply_update_request` here is incorrect because it can re-run
            // auto-detection and silently mutate `language` / `language_is_manual`
            // instead of replaying the persisted historical state.
            paste.content = target_content;
            paste.is_markdown = is_markdown_content(&paste.content);
            paste.language = target_meta.language.clone();
            paste.language_is_manual = target_meta.language_is_manual;
            paste.updated_at = reset_at;

            let encoded_paste = bincode::serialize(&paste)?;
            let encoded_meta = bincode::serialize(&PasteMeta::from(&paste))?;
            let new_recency_key = reverse_timestamp_key(paste.updated_at);
            pastes.insert(paste_id, encoded_paste.as_slice())?;
            metas.insert(paste_id, encoded_meta.as_slice())?;
            let _ = updated.remove((old_recency_key, paste_id))?;
            updated.insert((new_recency_key, paste_id), ())?;

            let mut removed_versions = Vec::new();
            version_items.retain(|item| {
                // Historical table stores only snapshots older than current head.
                // After reset, the target snapshot becomes the new head, so drop it
                // and everything newer, except for the explicitly archived outgoing
                // head from a dirty save-and-reset recovery path.
                let keep = item.version_id_ms < version_id_ms
                    || Some(item.version_id_ms) == preserved_current_head_version_id;
                if !keep {
                    removed_versions.push(item.version_id_ms);
                }
                keep
            });
            for removed in removed_versions {
                let _ = versions_content.remove((paste_id, removed))?;
            }
            let encoded_versions = encode_version_meta_list(&version_items)?;
            versions_meta.insert(paste_id, encoded_versions.as_slice())?;

            Some(paste)
        };

        write_txn.commit()?;
        Ok(updated_paste)
    }
}
