//! Atomic cross-table transaction helpers for folder-affecting mutations.

use super::tables::{
    FOLDERS, FOLDERS_DELETING, PASTES, PASTES_BY_UPDATED, PASTES_META, PASTE_VERSIONS_CONTENT,
    PASTE_VERSIONS_META,
};
use super::Database;
use crate::db::paste::{apply_update_request, deserialize_paste, reverse_timestamp_key};
use crate::db::versioning::{
    decode_version_meta_list, encode_version_meta_list, next_version_meta_for_content,
    should_record_version,
};
use crate::error::AppError;
use crate::models::folder::Folder;
use crate::models::paste::{Paste, PasteMeta, UpdatePasteRequest};
use redb::ReadableTable;
use std::sync::MutexGuard;

/// Atomic operations that update paste and folder rows together.
pub struct TransactionOps;

/// Guard that proves the caller holds the global folder transaction lock.
pub struct FolderTxnGuard<'a> {
    _guard: MutexGuard<'a, ()>,
}

fn ensure_folder_assignable_in_txn(
    folders: &redb::Table<&str, &[u8]>,
    deleting: &redb::Table<&str, ()>,
    folder_id: &str,
) -> Result<(), AppError> {
    if folders.get(folder_id)?.is_none() {
        return Err(AppError::NotFound);
    }
    if deleting.get(folder_id)?.is_some() {
        return Err(AppError::BadRequest(format!(
            "Folder with id '{}' is being deleted",
            folder_id
        )));
    }
    Ok(())
}

fn load_folder(
    folders: &redb::Table<&str, &[u8]>,
    folder_id: &str,
) -> Result<Option<Folder>, AppError> {
    let Some(guard) = folders.get(folder_id)? else {
        return Ok(None);
    };
    Ok(Some(bincode::deserialize(guard.value())?))
}

fn folder_disappeared_after_assignability_error(folder_id: &str) -> AppError {
    AppError::StorageMessage(format!(
        "Folder '{}' disappeared inside a single write transaction after assignability check",
        folder_id
    ))
}

fn apply_folder_count_transition(
    folders: &mut redb::Table<&str, &[u8]>,
    old_folder_id: Option<&str>,
    new_folder_id: Option<&str>,
) -> Result<(), AppError> {
    if old_folder_id == new_folder_id {
        return Ok(());
    }

    if let Some(old_id) = old_folder_id {
        if let Some(mut old_folder) = load_folder(folders, old_id)? {
            old_folder.paste_count = old_folder.paste_count.saturating_sub(1);
            let encoded_old = bincode::serialize(&old_folder)?;
            folders.insert(old_id, encoded_old.as_slice())?;
        }
    }

    if let Some(new_id) = new_folder_id {
        let mut new_folder = load_folder(folders, new_id)?
            .ok_or_else(|| folder_disappeared_after_assignability_error(new_id))?;
        new_folder.paste_count = new_folder.paste_count.saturating_add(1);
        let encoded_new = bincode::serialize(&new_folder)?;
        folders.insert(new_id, encoded_new.as_slice())?;
    }

    Ok(())
}

struct PersistPasteIndexUpdate<'a> {
    old_recency_key: Option<u64>,
    old_folder_id: Option<&'a str>,
    new_folder_id: Option<&'a str>,
}

fn persist_paste_with_indexes_and_folder_counts(
    pastes: &mut redb::Table<&str, &[u8]>,
    metas: &mut redb::Table<&str, &[u8]>,
    updated: &mut redb::Table<(u64, &str), ()>,
    folders: &mut redb::Table<&str, &[u8]>,
    paste: &Paste,
    index_update: PersistPasteIndexUpdate<'_>,
) -> Result<(), AppError> {
    let paste_id = paste.id.as_str();
    let encoded_paste = bincode::serialize(paste)?;
    let encoded_meta = bincode::serialize(&PasteMeta::from(paste))?;
    if let Some(old_key) = index_update.old_recency_key {
        let _ = updated.remove((old_key, paste_id))?;
    }
    updated.insert((reverse_timestamp_key(paste.updated_at), paste_id), ())?;
    pastes.insert(paste_id, encoded_paste.as_slice())?;
    metas.insert(paste_id, encoded_meta.as_slice())?;
    apply_folder_count_transition(
        folders,
        index_update.old_folder_id,
        index_update.new_folder_id,
    )?;
    Ok(())
}

impl TransactionOps {
    /// Acquire the global folder transaction guard.
    ///
    /// This typed guard must be passed to guarded mutation helpers to guarantee
    /// consistent lock ordering across crates.
    ///
    /// # Returns
    /// A guard that must be held for the full folder-affecting critical section.
    ///
    /// # Errors
    /// Returns an error when the lock is poisoned.
    pub fn acquire_folder_txn_guard(db: &Database) -> Result<FolderTxnGuard<'_>, AppError> {
        let guard = db.folder_txn_lock.lock().map_err(|_| {
            AppError::StorageMessage("Folder transaction lock poisoned".to_string())
        })?;
        Ok(FolderTxnGuard { _guard: guard })
    }

    /// Atomically create a paste and increment the destination folder count.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `paste`: Paste row to insert.
    /// - `folder_id`: Destination folder id.
    ///
    /// # Returns
    /// `Ok(())` when the write commits.
    ///
    /// # Errors
    /// Returns an error when folder assignment is invalid, id already exists,
    /// serialization fails, or storage operations fail.
    pub fn create_paste_with_folder(
        db: &Database,
        paste: &Paste,
        folder_id: &str,
    ) -> Result<(), AppError> {
        if let Some(existing_folder_id) = paste.folder_id.as_deref() {
            if existing_folder_id != folder_id {
                return Err(AppError::BadRequest(format!(
                    "Create folder_id '{}' does not match paste.folder_id '{}'",
                    folder_id, existing_folder_id
                )));
            }
        }
        let guard = Self::acquire_folder_txn_guard(db)?;
        Self::create_paste_with_folder_locked(db, &guard, paste, folder_id)
    }

    /// Create a paste while holding a folder transaction guard.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `_folder_guard`: Active folder transaction guard for this critical section.
    /// - `paste`: Paste row to insert.
    /// - `folder_id`: Destination folder id.
    ///
    /// # Returns
    /// `Ok(())` when the write commits.
    ///
    /// # Errors
    /// Returns an error when folder assignment is invalid, id already exists,
    /// serialization fails, or storage operations fail.
    pub fn create_paste_with_folder_locked(
        db: &Database,
        _folder_guard: &FolderTxnGuard<'_>,
        paste: &Paste,
        folder_id: &str,
    ) -> Result<(), AppError> {
        // Keep caller-owned model values immutable at this layer: persistence
        // uses a cloned row with the canonical folder assignment applied.
        let mut paste = paste.clone();
        paste.folder_id = Some(folder_id.to_string());

        let write_txn = db.db.begin_write()?;
        {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;
            let mut folders = write_txn.open_table(FOLDERS)?;
            let deleting = write_txn.open_table(FOLDERS_DELETING)?;

            ensure_folder_assignable_in_txn(&folders, &deleting, folder_id)?;
            if pastes.get(paste.id.as_str())?.is_some() {
                return Err(AppError::StorageMessage(format!(
                    "Paste id '{}' already exists",
                    paste.id
                )));
            }

            persist_paste_with_indexes_and_folder_counts(
                &mut pastes,
                &mut metas,
                &mut updated,
                &mut folders,
                &paste,
                PersistPasteIndexUpdate {
                    old_recency_key: None,
                    old_folder_id: None,
                    new_folder_id: Some(folder_id),
                },
            )?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Atomically delete a paste and decrement folder count when applicable.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `paste_id`: Paste id to remove.
    ///
    /// # Returns
    /// `Ok(true)` when a paste was removed, `Ok(false)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn delete_paste_with_folder(db: &Database, paste_id: &str) -> Result<bool, AppError> {
        let guard = Self::acquire_folder_txn_guard(db)?;
        Self::delete_paste_with_folder_locked(db, &guard, paste_id)
    }

    /// Delete a paste while holding a folder transaction guard.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `_folder_guard`: Active folder transaction guard for this critical section.
    /// - `paste_id`: Paste id to remove.
    ///
    /// # Returns
    /// `Ok(true)` when a paste was removed, `Ok(false)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn delete_paste_with_folder_locked(
        db: &Database,
        _folder_guard: &FolderTxnGuard<'_>,
        paste_id: &str,
    ) -> Result<bool, AppError> {
        let write_txn = db.db.begin_write()?;
        let deleted = {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;
            let mut versions_meta = write_txn.open_table(PASTE_VERSIONS_META)?;
            let mut versions_content = write_txn.open_table(PASTE_VERSIONS_CONTENT)?;
            let mut folders = write_txn.open_table(FOLDERS)?;

            let Some(old_guard) = pastes.get(paste_id)? else {
                return Ok(false);
            };
            let paste = deserialize_paste(old_guard.value())?;
            let old_recency_key = reverse_timestamp_key(paste.updated_at);
            let old_folder_id = paste.folder_id;
            drop(old_guard);

            let _ = updated.remove((old_recency_key, paste_id))?;
            let _ = pastes.remove(paste_id)?;
            let _ = metas.remove(paste_id)?;
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

            apply_folder_count_transition(&mut folders, old_folder_id.as_deref(), None)?;
            true
        };

        write_txn.commit()?;
        Ok(deleted)
    }

    /// Atomically move a paste between folders while applying additional updates.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `paste_id`: Paste id to update.
    /// - `new_folder_id`: Destination folder id, or `None` for unfiled.
    /// - `update_req`: Additional patch fields for the paste row.
    ///
    /// # Returns
    /// `Ok(Some(paste))` when updated, `Ok(None)` when the paste does not exist.
    ///
    /// # Errors
    /// Returns an error when destination assignment is invalid, or when storage /
    /// serialization operations fail.
    pub fn move_paste_between_folders(
        db: &Database,
        paste_id: &str,
        new_folder_id: Option<&str>,
        update_req: UpdatePasteRequest,
    ) -> Result<Option<Paste>, AppError> {
        if let Some(request_folder_id) = update_req.folder_id.as_deref() {
            let normalized_request_folder = if request_folder_id.is_empty() {
                None
            } else {
                Some(request_folder_id)
            };
            if normalized_request_folder != new_folder_id {
                return Err(AppError::BadRequest(format!(
                    "Move new_folder_id {:?} does not match update_req.folder_id {:?}",
                    new_folder_id, normalized_request_folder
                )));
            }
        }
        let guard = Self::acquire_folder_txn_guard(db)?;
        Self::move_paste_between_folders_locked(db, &guard, paste_id, new_folder_id, update_req)
    }

    /// Move a paste between folders while holding a folder transaction guard.
    ///
    /// # Arguments
    /// - `db`: Open database handle.
    /// - `_folder_guard`: Active folder transaction guard for this critical section.
    /// - `paste_id`: Paste id to update.
    /// - `new_folder_id`: Destination folder id, or `None` for unfiled.
    /// - `update_req`: Additional patch fields for the paste row.
    ///
    /// # Returns
    /// `Ok(Some(paste))` when updated, `Ok(None)` when missing.
    ///
    /// # Errors
    /// Returns an error when destination assignment is invalid, or when storage /
    /// serialization operations fail.
    pub fn move_paste_between_folders_locked(
        db: &Database,
        _folder_guard: &FolderTxnGuard<'_>,
        paste_id: &str,
        new_folder_id: Option<&str>,
        update_req: UpdatePasteRequest,
    ) -> Result<Option<Paste>, AppError> {
        let version_interval_secs = db.pastes.version_interval_secs();
        let write_txn = db.db.begin_write()?;
        let updated_paste = {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;
            let mut versions_meta = write_txn.open_table(PASTE_VERSIONS_META)?;
            let mut versions_content = write_txn.open_table(PASTE_VERSIONS_CONTENT)?;
            let mut folders = write_txn.open_table(FOLDERS)?;
            let deleting = write_txn.open_table(FOLDERS_DELETING)?;

            let Some(old_guard) = pastes.get(paste_id)? else {
                return Ok(None);
            };
            let mut paste = deserialize_paste(old_guard.value())?;
            let old_folder_id = paste.folder_id.clone();
            let folder_changing = old_folder_id.as_deref() != new_folder_id;
            let old_recency_key = reverse_timestamp_key(paste.updated_at);
            drop(old_guard);

            if folder_changing {
                if let Some(new_id) = new_folder_id {
                    ensure_folder_assignable_in_txn(&folders, &deleting, new_id)?;
                }
            }

            let old_content = paste.content.clone();
            let old_language = paste.language.clone();
            let old_language_is_manual = paste.language_is_manual;
            let old_folder_id_ref = old_folder_id.as_deref();
            let mut version_items = decode_version_meta_list(
                versions_meta
                    .get(paste_id)?
                    .as_ref()
                    .map(|value| value.value()),
            )?;
            apply_update_request(&mut paste, &update_req);
            paste.folder_id = new_folder_id.map(ToString::to_string);
            let content_changed = paste.content != old_content;

            if content_changed {
                let latest = version_items.first();
                let next = next_version_meta_for_content(
                    old_content.as_str(),
                    old_language.as_deref(),
                    old_language_is_manual,
                    paste.updated_at,
                    latest,
                );
                if should_record_version(latest, &next, version_interval_secs) {
                    let encoded_content = bincode::serialize(&old_content)?;
                    versions_content
                        .insert((paste_id, next.version_id_ms), encoded_content.as_slice())?;
                    version_items.insert(0, next);
                    let encoded_versions = encode_version_meta_list(&version_items)?;
                    versions_meta.insert(paste_id, encoded_versions.as_slice())?;
                }
            }

            persist_paste_with_indexes_and_folder_counts(
                &mut pastes,
                &mut metas,
                &mut updated,
                &mut folders,
                &paste,
                PersistPasteIndexUpdate {
                    old_recency_key: Some(old_recency_key),
                    old_folder_id: old_folder_id_ref,
                    new_folder_id,
                },
            )?;

            Some(paste)
        };

        write_txn.commit()?;
        Ok(updated_paste)
    }
}
