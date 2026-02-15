//! Atomic cross-table transaction helpers for folder-affecting mutations.

use super::tables::{FOLDERS, FOLDERS_DELETING, PASTES, PASTES_BY_UPDATED, PASTES_META};
use super::Database;
use crate::db::paste::{apply_update_request, deserialize_paste, reverse_timestamp_key};
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

impl TransactionOps {
    /// Acquire the global folder-transaction lock.
    ///
    /// # Returns
    /// A guard that must be held for the full folder-affecting critical section.
    ///
    /// # Errors
    /// Returns an error when the lock is poisoned.
    pub fn acquire_folder_txn_lock(
        db: &Database,
    ) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
        db.folder_txn_lock
            .lock()
            .map_err(|_| AppError::StorageMessage("Folder transaction lock poisoned".to_string()))
    }

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
        let guard = Self::acquire_folder_txn_lock(db)?;
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

        let encoded_paste = bincode::serialize(&paste)?;
        let encoded_meta = bincode::serialize(&PasteMeta::from(&paste))?;
        let recency_key = reverse_timestamp_key(paste.updated_at);

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

            let mut folder = load_folder(&folders, folder_id)?.ok_or_else(|| {
                AppError::StorageMessage(format!(
                    "Folder '{}' disappeared inside a single write transaction after assignability check",
                    folder_id
                ))
            })?;
            folder.paste_count = folder.paste_count.saturating_add(1);
            let encoded_folder = bincode::serialize(&folder)?;

            pastes.insert(paste.id.as_str(), encoded_paste.as_slice())?;
            metas.insert(paste.id.as_str(), encoded_meta.as_slice())?;
            updated.insert((recency_key, paste.id.as_str()), ())?;
            folders.insert(folder_id, encoded_folder.as_slice())?;
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
            let mut folders = write_txn.open_table(FOLDERS)?;

            let Some(old_guard) = pastes.get(paste_id)? else {
                return Ok(false);
            };
            let paste = deserialize_paste(old_guard.value())?;
            let old_recency_key = reverse_timestamp_key(paste.updated_at);
            let old_folder_id = paste.folder_id.clone();
            drop(old_guard);

            let _ = updated.remove((old_recency_key, paste_id))?;
            let _ = pastes.remove(paste_id)?;
            let _ = metas.remove(paste_id)?;

            if let Some(folder_id) = old_folder_id.as_deref() {
                if let Some(mut folder) = load_folder(&folders, folder_id)? {
                    folder.paste_count = folder.paste_count.saturating_sub(1);
                    let encoded_folder = bincode::serialize(&folder)?;
                    folders.insert(folder_id, encoded_folder.as_slice())?;
                }
            }
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
        let write_txn = db.db.begin_write()?;
        let updated_paste = {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;
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

            apply_update_request(&mut paste, &update_req);
            paste.folder_id = new_folder_id.map(ToString::to_string);

            let encoded_paste = bincode::serialize(&paste)?;
            let encoded_meta = bincode::serialize(&PasteMeta::from(&paste))?;
            let new_recency_key = reverse_timestamp_key(paste.updated_at);

            pastes.insert(paste_id, encoded_paste.as_slice())?;
            metas.insert(paste_id, encoded_meta.as_slice())?;
            let _ = updated.remove((old_recency_key, paste_id))?;
            updated.insert((new_recency_key, paste_id), ())?;

            if folder_changing {
                if let Some(old_id) = old_folder_id.as_deref() {
                    if let Some(mut old_folder) = load_folder(&folders, old_id)? {
                        old_folder.paste_count = old_folder.paste_count.saturating_sub(1);
                        let encoded_old_folder = bincode::serialize(&old_folder)?;
                        folders.insert(old_id, encoded_old_folder.as_slice())?;
                    }
                }
                if let Some(new_id) = new_folder_id {
                    let mut new_folder = load_folder(&folders, new_id)?.ok_or_else(|| {
                        AppError::StorageMessage(format!(
                            "Folder '{}' disappeared inside a single write transaction after assignability check",
                            new_id
                        ))
                    })?;
                    new_folder.paste_count = new_folder.paste_count.saturating_add(1);
                    let encoded_new_folder = bincode::serialize(&new_folder)?;
                    folders.insert(new_id, encoded_new_folder.as_slice())?;
                }
            }

            Some(paste)
        };

        write_txn.commit()?;
        Ok(updated_paste)
    }
}
