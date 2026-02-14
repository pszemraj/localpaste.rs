//! Folder storage operations backed by redb.

use crate::{db::tables::*, error::AppError, models::folder::*};
use redb::{ReadableDatabase, ReadableTable};
use std::sync::Arc;

/// Accessor for folder-related redb tables.
pub struct FolderDb {
    db: Arc<redb::Database>,
}

impl FolderDb {
    /// Initialize folder tables if they do not exist yet.
    ///
    /// # Returns
    /// A new [`FolderDb`] accessor bound to `db`.
    ///
    /// # Errors
    /// Returns an error when redb transaction/table initialization fails.
    pub fn new(db: Arc<redb::Database>) -> Result<Self, AppError> {
        let write_txn = db.begin_write()?;
        write_txn.open_table(FOLDERS)?;
        write_txn.open_table(FOLDERS_DELETING)?;
        write_txn.commit()?;
        Ok(Self { db })
    }

    /// Insert a new folder row.
    ///
    /// # Arguments
    /// - `folder`: Folder row to persist.
    ///
    /// # Returns
    /// `Ok(())` when the row is inserted.
    ///
    /// # Errors
    /// Returns an error when serialization fails, the id already exists, or
    /// underlying storage operations fail.
    pub fn create(&self, folder: &Folder) -> Result<(), AppError> {
        let encoded = bincode::serialize(folder)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut folders = write_txn.open_table(FOLDERS)?;
            if folders.get(folder.id.as_str())?.is_some() {
                return Err(AppError::StorageMessage(format!(
                    "Folder id '{}' already exists",
                    folder.id
                )));
            }
            folders.insert(folder.id.as_str(), encoded.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Fetch a folder by id.
    ///
    /// # Returns
    /// `Ok(Some(folder))` when found, `Ok(None)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn get(&self, id: &str) -> Result<Option<Folder>, AppError> {
        let read_txn = self.db.begin_read()?;
        let folders = read_txn.open_table(FOLDERS)?;
        match folders.get(id)? {
            Some(value) => Ok(Some(bincode::deserialize(value.value())?)),
            None => Ok(None),
        }
    }

    /// Update folder name and optionally parent id.
    ///
    /// Empty `parent_id` values are normalized to `None`.
    ///
    /// # Arguments
    /// - `id`: Folder id to update.
    /// - `name`: New folder display name.
    /// - `parent_id`: Optional parent id (empty string clears parent).
    ///
    /// # Returns
    /// `Ok(Some(folder))` when updated, `Ok(None)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access or serialization fails.
    pub fn update(
        &self,
        id: &str,
        name: String,
        parent_id: Option<String>,
    ) -> Result<Option<Folder>, AppError> {
        self.update_folder_record(id, move |folder| {
            folder.name = name.clone();
            if let Some(ref pid) = parent_id {
                folder.parent_id = if pid.is_empty() {
                    None
                } else {
                    Some(pid.clone())
                };
            }
            Ok(())
        })
    }

    /// Delete a folder and clear any delete marker for the same id.
    ///
    /// # Returns
    /// `Ok(true)` when deleted, `Ok(false)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage operations fail.
    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        let write_txn = self.db.begin_write()?;
        let removed = {
            let mut folders = write_txn.open_table(FOLDERS)?;
            let mut deleting = write_txn.open_table(FOLDERS_DELETING)?;
            let removed = folders.remove(id)?.is_some();
            let _ = deleting.remove(id)?;
            removed
        };
        write_txn.commit()?;
        Ok(removed)
    }

    /// List all folders sorted by name.
    ///
    /// # Returns
    /// All known folders in ascending name order.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn list(&self) -> Result<Vec<Folder>, AppError> {
        let read_txn = self.db.begin_read()?;
        let folders_table = read_txn.open_table(FOLDERS)?;
        let mut folders = Vec::new();
        for item in folders_table.iter()? {
            let (_, value) = item?;
            let folder: Folder = bincode::deserialize(value.value())?;
            folders.push(folder);
        }
        folders.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(folders)
    }

    /// Add or subtract from a folder's `paste_count`.
    ///
    /// # Arguments
    /// - `id`: Folder id to update.
    /// - `delta`: Signed delta applied with saturation.
    ///
    /// # Returns
    /// `Ok(())` when the count update commits.
    ///
    /// # Errors
    /// Returns [`AppError::NotFound`] when the folder is missing, or storage
    /// / serialization errors when the update cannot be committed.
    pub fn update_count(&self, id: &str, delta: i32) -> Result<(), AppError> {
        let updated = self.update_folder_record(id, move |folder| {
            if delta > 0 {
                folder.paste_count = folder.paste_count.saturating_add(delta as usize);
            } else {
                folder.paste_count = folder.paste_count.saturating_sub((-delta) as usize);
            }
            Ok(())
        })?;
        if updated.is_none() {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    /// Set a folder's `paste_count` to an exact value.
    ///
    /// # Arguments
    /// - `id`: Folder id to update.
    /// - `count`: New exact paste count.
    ///
    /// # Returns
    /// `Ok(())` when the count update commits.
    ///
    /// # Errors
    /// Returns [`AppError::NotFound`] when the folder is missing, or storage
    /// / serialization errors when the update cannot be committed.
    pub fn set_count(&self, id: &str, count: usize) -> Result<(), AppError> {
        let updated = self.update_folder_record(id, move |folder| {
            folder.paste_count = count;
            Ok(())
        })?;
        if updated.is_none() {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    /// Mark folders as in-progress for delete workflows.
    ///
    /// # Returns
    /// `Ok(())` when all markers are written.
    ///
    /// # Errors
    /// Returns an error when storage operations fail.
    pub fn mark_deleting(&self, folder_ids: &[String]) -> Result<(), AppError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut deleting = write_txn.open_table(FOLDERS_DELETING)?;
            for folder_id in folder_ids {
                deleting.insert(folder_id.as_str(), ())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Remove delete markers for folders.
    ///
    /// # Returns
    /// `Ok(())` when all markers are removed.
    ///
    /// # Errors
    /// Returns an error when storage operations fail.
    pub fn unmark_deleting(&self, folder_ids: &[String]) -> Result<(), AppError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut deleting = write_txn.open_table(FOLDERS_DELETING)?;
            for folder_id in folder_ids {
                let _ = deleting.remove(folder_id.as_str())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Check whether a folder id is currently delete-marked.
    ///
    /// # Returns
    /// `true` when a marker is present, otherwise `false`.
    ///
    /// # Errors
    /// Returns an error when storage operations fail.
    pub fn is_delete_marked(&self, id: &str) -> Result<bool, AppError> {
        let read_txn = self.db.begin_read()?;
        let deleting = read_txn.open_table(FOLDERS_DELETING)?;
        Ok(deleting.get(id)?.is_some())
    }

    /// Remove all folder delete markers.
    ///
    /// # Returns
    /// `Ok(())` when table reset completes.
    ///
    /// # Errors
    /// Returns an error when table reset operations fail.
    pub fn clear_delete_markers(&self) -> Result<(), AppError> {
        let write_txn = self.db.begin_write()?;
        let _ = write_txn.delete_table(FOLDERS_DELETING);
        write_txn.open_table(FOLDERS_DELETING)?;
        write_txn.commit()?;
        Ok(())
    }

    fn update_folder_record<F>(&self, id: &str, mut mutator: F) -> Result<Option<Folder>, AppError>
    where
        F: FnMut(&mut Folder) -> Result<(), AppError>,
    {
        let write_txn = self.db.begin_write()?;
        let result = {
            let mut folders = write_txn.open_table(FOLDERS)?;
            let Some(value) = folders.get(id)? else {
                return Ok(None);
            };

            let mut folder: Folder = bincode::deserialize(value.value())?;
            drop(value);

            mutator(&mut folder)?;
            let encoded = bincode::serialize(&folder)?;
            folders.insert(id, encoded.as_slice())?;
            Some(folder)
        };

        write_txn.commit()?;
        Ok(result)
    }
}
