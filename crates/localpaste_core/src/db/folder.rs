//! Folder storage operations backed by sled.

use crate::{error::AppError, models::folder::*};
use sled::Db;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Accessor for the `folders` sled tree.
pub struct FolderDb {
    tree: sled::Tree,
    delete_markers: sled::Tree,
}

impl FolderDb {
    /// Open the `folders` tree.
    ///
    /// # Returns
    /// A [`FolderDb`] bound to the `folders` tree.
    ///
    /// # Errors
    /// Returns an error if the tree cannot be opened.
    pub fn new(db: Arc<Db>) -> Result<Self, AppError> {
        let tree = db.open_tree("folders")?;
        let delete_markers = db.open_tree("folders_deleting")?;
        Ok(Self {
            tree,
            delete_markers,
        })
    }

    /// Insert a new folder.
    ///
    /// # Returns
    /// `Ok(())` on success.
    ///
    /// # Errors
    /// Returns an error if serialization or insertion fails.
    pub fn create(&self, folder: &Folder) -> Result<(), AppError> {
        let key = folder.id.as_bytes();
        let value = bincode::serialize(folder)?;
        self.tree.insert(key, value)?;
        Ok(())
    }

    /// Fetch a folder by id.
    ///
    /// # Returns
    /// The folder if it exists.
    ///
    /// # Errors
    /// Returns an error if the lookup fails.
    pub fn get(&self, id: &str) -> Result<Option<Folder>, AppError> {
        Ok(self
            .tree
            .get(id.as_bytes())?
            .map(|v| bincode::deserialize(&v))
            .transpose()?)
    }

    /// Update a folder's name and optional parent.
    ///
    /// # Arguments
    /// - `id`: Folder identifier.
    /// - `name`: New folder name.
    /// - `parent_id`: Optional new parent id (empty string normalizes to `None`).
    ///
    /// # Returns
    /// Updated folder if it exists.
    ///
    /// # Errors
    /// Returns an error if serialization or update fails.
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

    /// Delete a folder by id.
    ///
    /// # Returns
    /// `true` if a folder was deleted.
    ///
    /// # Errors
    /// Returns an error if deletion fails.
    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        let removed = self.tree.remove(id.as_bytes())?.is_some();
        let _ = self.delete_markers.remove(id.as_bytes())?;
        Ok(removed)
    }

    /// List all folders.
    ///
    /// # Returns
    /// A sorted list of folders.
    ///
    /// # Errors
    /// Returns an error if iteration fails.
    pub fn list(&self) -> Result<Vec<Folder>, AppError> {
        let mut folders = Vec::new();
        for item in self.tree.iter() {
            let (_, value) = item?;
            let folder: Folder = bincode::deserialize(&value)?;
            folders.push(folder);
        }
        folders.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(folders)
    }

    /// Adjust the paste count for a folder.
    ///
    /// # Arguments
    /// - `id`: Folder identifier.
    /// - `delta`: Count adjustment (positive or negative).
    ///
    /// # Returns
    /// `Ok(())` when the update is applied.
    ///
    /// # Errors
    /// Returns [`AppError::NotFound`] when the folder does not exist, or a storage/serialization
    /// error when the update fails.
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

    /// Set a folder paste count to an exact value.
    ///
    /// # Arguments
    /// - `id`: Folder identifier.
    /// - `count`: Exact canonical paste count.
    ///
    /// # Returns
    /// `Ok(())` when the update is applied.
    ///
    /// # Errors
    /// Returns [`AppError::NotFound`] when the folder does not exist, or a storage/serialization
    /// error when the update fails.
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

    /// Mark folders as in-progress deletion targets.
    ///
    /// # Arguments
    /// - `folder_ids`: Folder ids to mark.
    ///
    /// # Returns
    /// `Ok(())` after all markers are persisted.
    ///
    /// # Errors
    /// Returns an error if the marker tree write fails.
    pub fn mark_deleting(&self, folder_ids: &[String]) -> Result<(), AppError> {
        for folder_id in folder_ids {
            let _ = self.delete_markers.insert(folder_id.as_bytes(), &[1u8])?;
        }
        Ok(())
    }

    /// Remove in-progress deletion markers from folders.
    ///
    /// # Arguments
    /// - `folder_ids`: Folder ids to unmark.
    ///
    /// # Returns
    /// `Ok(())` after marker removal completes.
    ///
    /// # Errors
    /// Returns an error if marker removal fails.
    pub fn unmark_deleting(&self, folder_ids: &[String]) -> Result<(), AppError> {
        for folder_id in folder_ids {
            let _ = self.delete_markers.remove(folder_id.as_bytes())?;
        }
        Ok(())
    }

    /// Check whether a folder is marked as being deleted.
    ///
    /// # Arguments
    /// - `id`: Folder identifier.
    ///
    /// # Returns
    /// `true` when deletion is in progress for the folder id.
    ///
    /// # Errors
    /// Returns an error if marker lookup fails.
    pub fn is_delete_marked(&self, id: &str) -> Result<bool, AppError> {
        Ok(self.delete_markers.get(id.as_bytes())?.is_some())
    }

    /// Clear all in-progress delete markers.
    ///
    /// # Returns
    /// `Ok(())` when marker state is fully reset.
    ///
    /// # Errors
    /// Returns an error if marker tree clear fails.
    pub fn clear_delete_markers(&self) -> Result<(), AppError> {
        self.delete_markers.clear()?;
        Ok(())
    }

    fn update_folder_record<F>(&self, id: &str, mutator: F) -> Result<Option<Folder>, AppError>
    where
        F: FnMut(&mut Folder) -> Result<(), AppError>,
    {
        let update_error = Rc::new(RefCell::new(None));
        let update_error_in = Rc::clone(&update_error);
        let mutator = Rc::new(RefCell::new(mutator));
        let mutator_in = Rc::clone(&mutator);
        let result = self.tree.update_and_fetch(id.as_bytes(), move |old| {
            let bytes = old?;

            let mut folder: Folder = match bincode::deserialize(bytes) {
                Ok(folder) => folder,
                Err(err) => {
                    *update_error_in.borrow_mut() = Some(AppError::Serialization(err));
                    return Some(bytes.to_vec());
                }
            };

            if let Err(err) = (mutator_in.borrow_mut())(&mut folder) {
                *update_error_in.borrow_mut() = Some(err);
                return Some(bytes.to_vec());
            }

            match bincode::serialize(&folder) {
                Ok(encoded) => Some(encoded),
                Err(err) => {
                    *update_error_in.borrow_mut() = Some(AppError::Serialization(err));
                    Some(bytes.to_vec())
                }
            }
        })?;

        if let Some(err) = update_error.borrow_mut().take() {
            return Err(err);
        }

        match result {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }
}
