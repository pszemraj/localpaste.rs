//! Folder storage operations backed by sled.

use crate::{error::AppError, models::folder::*};
use sled::Db;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// Accessor for the `folders` sled tree.
pub struct FolderDb {
    tree: sled::Tree,
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
        Ok(Self { tree })
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
        let update_error = Rc::new(RefCell::new(None));
        let update_error_in = Rc::clone(&update_error);
        let result = self.tree.update_and_fetch(id.as_bytes(), move |old| {
            let name = name.clone();
            let parent_id = parent_id.clone();
            let bytes = old?;

            let mut folder: Folder = match bincode::deserialize(bytes) {
                Ok(folder) => folder,
                Err(err) => {
                    *update_error_in.borrow_mut() = Some(AppError::Serialization(err));
                    return Some(bytes.to_vec());
                }
            };
            folder.name = name;
            if let Some(ref pid) = parent_id {
                folder.parent_id = if pid.is_empty() {
                    None
                } else {
                    Some(pid.clone())
                };
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

    /// Delete a folder by id.
    ///
    /// # Returns
    /// `true` if a folder was deleted.
    ///
    /// # Errors
    /// Returns an error if deletion fails.
    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        Ok(self.tree.remove(id.as_bytes())?.is_some())
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
        let missing = Rc::new(Cell::new(false));
        let missing_in = Rc::clone(&missing);
        let update_error = Rc::new(RefCell::new(None));
        let update_error_in = Rc::clone(&update_error);
        self.tree.fetch_and_update(id.as_bytes(), |old| {
            let Some(bytes) = old else {
                missing_in.set(true);
                return None;
            };

            let mut folder: Folder = match bincode::deserialize(bytes) {
                Ok(folder) => folder,
                Err(err) => {
                    *update_error_in.borrow_mut() = Some(AppError::Serialization(err));
                    return Some(bytes.to_vec());
                }
            };

            if delta > 0 {
                folder.paste_count = folder.paste_count.saturating_add(delta as usize);
            } else {
                folder.paste_count = folder.paste_count.saturating_sub((-delta) as usize);
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
        if missing.get() {
            return Err(AppError::NotFound);
        }

        Ok(())
    }
}
