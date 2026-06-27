//! Persistent deleted-paste undo transactions.

use super::tables::{
    DELETED_PASTES, DELETED_PASTE_VERSIONS_CONTENT, DELETED_PASTE_VERSIONS_META, FOLDERS,
    FOLDERS_DELETING, PASTES, PASTES_BY_UPDATED, PASTES_META, PASTE_VERSIONS_CONTENT,
    PASTE_VERSIONS_META,
};
use super::transactions::{apply_folder_count_transition, FolderTxnGuard, TransactionOps};
use super::Database;
use crate::db::paste::{deserialize_paste, reverse_timestamp_key};
use crate::db::time_util::unix_timestamp_millis;
use crate::db::versioning::{decode_version_meta_list, encode_version_meta_list};
use crate::error::AppError;
use crate::models::paste::{DeletedPasteRecord, Paste, PasteMeta};
use redb::ReadableTable;
use std::time::SystemTime;

fn collect_deleted_version_content_ids(
    deleted_versions_content: &redb::Table<(&str, u64), &[u8]>,
    token: &str,
) -> Result<Vec<u64>, AppError> {
    let mut ids = Vec::new();
    for row in deleted_versions_content.iter()? {
        let (key, _) = row?;
        let (row_token, version_id_ms) = key.value();
        if row_token == token {
            ids.push(version_id_ms);
        }
    }
    Ok(ids)
}

fn remove_deleted_paste_undo_rows(
    deleted_pastes: &mut redb::Table<&str, &[u8]>,
    deleted_versions_meta: &mut redb::Table<&str, &[u8]>,
    deleted_versions_content: &mut redb::Table<(&str, u64), &[u8]>,
    token: &str,
) -> Result<bool, AppError> {
    let removed_paste = deleted_pastes.remove(token)?.is_some();
    let version_items = match deleted_versions_meta.remove(token)? {
        Some(meta_guard) => decode_version_meta_list(Some(meta_guard.value()))?,
        None => Vec::new(),
    };
    if version_items.is_empty() {
        let orphan_ids = collect_deleted_version_content_ids(deleted_versions_content, token)?;
        for version_id_ms in orphan_ids {
            let _ = deleted_versions_content.remove((token, version_id_ms))?;
        }
    } else {
        for version in version_items {
            let _ = deleted_versions_content.remove((token, version.version_id_ms))?;
        }
    }
    Ok(removed_paste)
}

impl TransactionOps {
    /// Atomically delete a paste into persistent undo staging.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `paste_id`: Paste id to remove.
    /// - `undo_token`: Token used to restore the staged tombstone.
    /// - `expires_at_ms`: Absolute expiration time in Unix milliseconds.
    ///
    /// # Returns
    /// `Ok(true)` when a paste was staged, `Ok(false)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access, serialization, or tombstone staging fails.
    pub fn delete_paste_with_folder_staged_undo(
        db: &Database,
        paste_id: &str,
        undo_token: &str,
        expires_at_ms: i64,
    ) -> Result<bool, AppError> {
        let guard = Self::acquire_folder_txn_guard(db)?;
        Self::delete_paste_with_folder_staged_undo_locked(
            db,
            &guard,
            paste_id,
            undo_token,
            expires_at_ms,
        )
    }

    /// Delete a paste into persistent undo staging while holding the folder guard.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `_folder_guard`: Active folder transaction guard for this critical section.
    /// - `paste_id`: Paste id to remove.
    /// - `undo_token`: Token used to restore the staged tombstone.
    /// - `expires_at_ms`: Absolute expiration time in Unix milliseconds.
    ///
    /// # Returns
    /// `Ok(true)` when a paste was staged, `Ok(false)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access, serialization, or tombstone staging fails.
    pub fn delete_paste_with_folder_staged_undo_locked(
        db: &Database,
        _folder_guard: &FolderTxnGuard<'_>,
        paste_id: &str,
        undo_token: &str,
        expires_at_ms: i64,
    ) -> Result<bool, AppError> {
        let write_txn = db.db.begin_write()?;
        let deleted = {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;
            let mut versions_meta = write_txn.open_table(PASTE_VERSIONS_META)?;
            let mut versions_content = write_txn.open_table(PASTE_VERSIONS_CONTENT)?;
            let mut folders = write_txn.open_table(FOLDERS)?;
            let mut deleted_pastes = write_txn.open_table(DELETED_PASTES)?;
            let mut deleted_versions_meta = write_txn.open_table(DELETED_PASTE_VERSIONS_META)?;
            let mut deleted_versions_content =
                write_txn.open_table(DELETED_PASTE_VERSIONS_CONTENT)?;

            if deleted_pastes.get(undo_token)?.is_some() {
                return Err(AppError::StorageMessage(format!(
                    "Delete undo token '{}' already exists",
                    undo_token
                )));
            }
            let Some(old_guard) = pastes.get(paste_id)? else {
                return Ok(false);
            };
            let paste = deserialize_paste(old_guard.value())?;
            let old_recency_key = reverse_timestamp_key(paste.updated_at);
            let old_folder_id = paste.folder_id.clone();
            drop(old_guard);

            let version_items = decode_version_meta_list(
                versions_meta
                    .get(paste_id)?
                    .as_ref()
                    .map(|value| value.value()),
            )?;
            for version in &version_items {
                let Some(content_guard) =
                    versions_content.get((paste_id, version.version_id_ms))?
                else {
                    return Err(AppError::StorageMessage(format!(
                        "Missing version content for paste '{}' version {}",
                        paste_id, version.version_id_ms
                    )));
                };
                let content_bytes = content_guard.value().to_vec();
                drop(content_guard);
                deleted_versions_content.insert(
                    (undo_token, version.version_id_ms),
                    content_bytes.as_slice(),
                )?;
            }

            let deleted_record = DeletedPasteRecord {
                paste: paste.clone(),
                expires_at_ms,
            };
            let encoded_record = bincode::serialize(&deleted_record)?;
            let encoded_versions = encode_version_meta_list(&version_items)?;
            deleted_pastes.insert(undo_token, encoded_record.as_slice())?;
            deleted_versions_meta.insert(undo_token, encoded_versions.as_slice())?;

            let _ = updated.remove((old_recency_key, paste_id))?;
            let _ = pastes.remove(paste_id)?;
            let _ = metas.remove(paste_id)?;
            let _ = versions_meta.remove(paste_id)?;
            for version in version_items {
                let _ = versions_content.remove((paste_id, version.version_id_ms))?;
            }
            apply_folder_count_transition(&mut folders, old_folder_id.as_deref(), None)?;
            true
        };

        write_txn.commit()?;
        Ok(deleted)
    }

    /// Permanently discard a staged deleted-paste undo token.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `undo_token`: Token to discard.
    ///
    /// # Returns
    /// `Ok(true)` when a staged paste row was removed.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn discard_deleted_paste_undo(db: &Database, undo_token: &str) -> Result<bool, AppError> {
        let write_txn = db.db.begin_write()?;
        let removed = {
            let mut deleted_pastes = write_txn.open_table(DELETED_PASTES)?;
            let mut deleted_versions_meta = write_txn.open_table(DELETED_PASTE_VERSIONS_META)?;
            let mut deleted_versions_content =
                write_txn.open_table(DELETED_PASTE_VERSIONS_CONTENT)?;
            remove_deleted_paste_undo_rows(
                &mut deleted_pastes,
                &mut deleted_versions_meta,
                &mut deleted_versions_content,
                undo_token,
            )?
        };
        write_txn.commit()?;
        Ok(removed)
    }

    /// Prune expired staged delete-undo tombstones.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `now_ms`: Current Unix timestamp in milliseconds.
    ///
    /// # Returns
    /// Tokens pruned from persistent undo staging.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn prune_expired_deleted_paste_undo(
        db: &Database,
        now_ms: i64,
    ) -> Result<Vec<String>, AppError> {
        let write_txn = db.db.begin_write()?;
        let pruned = {
            let mut deleted_pastes = write_txn.open_table(DELETED_PASTES)?;
            let mut deleted_versions_meta = write_txn.open_table(DELETED_PASTE_VERSIONS_META)?;
            let mut deleted_versions_content =
                write_txn.open_table(DELETED_PASTE_VERSIONS_CONTENT)?;
            let mut expired_tokens = Vec::new();
            for row in deleted_pastes.iter()? {
                let (token_guard, record_guard) = row?;
                let record: DeletedPasteRecord = bincode::deserialize(record_guard.value())?;
                if record.expires_at_ms <= now_ms {
                    expired_tokens.push(token_guard.value().to_string());
                }
            }
            for token in &expired_tokens {
                let _ = remove_deleted_paste_undo_rows(
                    &mut deleted_pastes,
                    &mut deleted_versions_meta,
                    &mut deleted_versions_content,
                    token,
                )?;
            }
            expired_tokens
        };
        write_txn.commit()?;
        Ok(pruned)
    }

    /// Restore a staged deleted paste by undo token.
    ///
    /// If the original folder no longer exists or is being deleted, the paste is
    /// restored unfiled while preserving all other paste fields and version rows.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `undo_token`: Token emitted by a prior staged delete.
    ///
    /// # Returns
    /// `Ok(Some(paste))` when restored, `Ok(None)` when the token is absent.
    ///
    /// # Errors
    /// Returns an error when storage access fails or the paste id already exists.
    pub fn restore_deleted_paste_by_token(
        db: &Database,
        undo_token: &str,
    ) -> Result<Option<Paste>, AppError> {
        let guard = Self::acquire_folder_txn_guard(db)?;
        Self::restore_deleted_paste_by_token_locked(db, &guard, undo_token)
    }

    /// Restore a staged deleted paste by token while holding the folder guard.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `_folder_guard`: Active folder transaction guard for this critical section.
    /// - `undo_token`: Token emitted by a prior staged delete.
    ///
    /// # Returns
    /// `Ok(Some(paste))` when restored, `Ok(None)` when the token is absent.
    ///
    /// # Errors
    /// Returns an error when storage access fails or the paste id already exists.
    pub fn restore_deleted_paste_by_token_locked(
        db: &Database,
        _folder_guard: &FolderTxnGuard<'_>,
        undo_token: &str,
    ) -> Result<Option<Paste>, AppError> {
        let now_ms = unix_timestamp_millis(SystemTime::now())?;
        let write_txn = db.db.begin_write()?;
        let restored = {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;
            let mut versions_meta = write_txn.open_table(PASTE_VERSIONS_META)?;
            let mut versions_content = write_txn.open_table(PASTE_VERSIONS_CONTENT)?;
            let mut folders = write_txn.open_table(FOLDERS)?;
            let deleting = write_txn.open_table(FOLDERS_DELETING)?;
            let mut deleted_pastes = write_txn.open_table(DELETED_PASTES)?;
            let mut deleted_versions_meta = write_txn.open_table(DELETED_PASTE_VERSIONS_META)?;
            let mut deleted_versions_content =
                write_txn.open_table(DELETED_PASTE_VERSIONS_CONTENT)?;

            let Some(record_guard) = deleted_pastes.get(undo_token)? else {
                return Ok(None);
            };
            let record: DeletedPasteRecord = bincode::deserialize(record_guard.value())?;
            drop(record_guard);
            if record.expires_at_ms <= now_ms {
                let _ = remove_deleted_paste_undo_rows(
                    &mut deleted_pastes,
                    &mut deleted_versions_meta,
                    &mut deleted_versions_content,
                    undo_token,
                )?;
                return Ok(None);
            }
            let mut paste = record.paste;
            if pastes.get(paste.id.as_str())?.is_some() {
                return Err(AppError::BadRequest(format!(
                    "Paste '{}' already exists",
                    paste.id
                )));
            }
            let version_items = decode_version_meta_list(
                deleted_versions_meta
                    .get(undo_token)?
                    .as_ref()
                    .map(|value| value.value()),
            )?;
            let mut version_contents = Vec::with_capacity(version_items.len());
            for version in &version_items {
                let Some(content_guard) =
                    deleted_versions_content.get((undo_token, version.version_id_ms))?
                else {
                    return Err(AppError::StorageMessage(format!(
                        "Missing staged version content for undo token '{}' version {}",
                        undo_token, version.version_id_ms
                    )));
                };
                version_contents.push((version.version_id_ms, content_guard.value().to_vec()));
            }

            let restore_folder = match paste.folder_id.as_deref() {
                Some(folder_id)
                    if folders.get(folder_id)?.is_some() && deleting.get(folder_id)?.is_none() =>
                {
                    paste.folder_id.clone()
                }
                _ => None,
            };
            paste.folder_id = restore_folder;

            let encoded_paste = bincode::serialize(&paste)?;
            let encoded_meta = bincode::serialize(&PasteMeta::from(&paste))?;
            let encoded_versions = encode_version_meta_list(&version_items)?;
            pastes.insert(paste.id.as_str(), encoded_paste.as_slice())?;
            metas.insert(paste.id.as_str(), encoded_meta.as_slice())?;
            updated.insert(
                (reverse_timestamp_key(paste.updated_at), paste.id.as_str()),
                (),
            )?;
            versions_meta.insert(paste.id.as_str(), encoded_versions.as_slice())?;
            for (version_id_ms, content_bytes) in version_contents {
                versions_content
                    .insert((paste.id.as_str(), version_id_ms), content_bytes.as_slice())?;
            }

            let _ = remove_deleted_paste_undo_rows(
                &mut deleted_pastes,
                &mut deleted_versions_meta,
                &mut deleted_versions_content,
                undo_token,
            )?;
            apply_folder_count_transition(&mut folders, None, paste.folder_id.as_deref())?;
            Some(paste)
        };

        write_txn.commit()?;
        Ok(restored)
    }
}
