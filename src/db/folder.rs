use crate::{error::AppError, models::folder::*};
use sled::Db;
use std::sync::Arc;

pub struct FolderDb {
    tree: sled::Tree,
    pastes_tree: sled::Tree,
}

impl FolderDb {
    pub fn new(db: Arc<Db>) -> Result<Self, AppError> {
        let tree = db.open_tree("folders")?;
        let pastes_tree = db.open_tree("pastes")?;
        Ok(Self { tree, pastes_tree })
    }

    pub fn create(&self, folder: &Folder) -> Result<(), AppError> {
        let key = folder.id.as_bytes();
        let value = bincode::serialize(folder)?;
        self.tree.insert(key, value)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Result<Option<Folder>, AppError> {
        Ok(self
            .tree
            .get(id.as_bytes())?
            .map(|v| bincode::deserialize(&v))
            .transpose()?)
    }

    pub fn update(&self, id: &str, name: String) -> Result<Option<Folder>, AppError> {
        let result = self.tree.update_and_fetch(id.as_bytes(), move |old| {
            old.and_then(|bytes| {
                let mut folder: Folder = bincode::deserialize(bytes).ok()?;
                folder.name = name.clone();
                bincode::serialize(&folder).ok()
            })
        })?;

        match result {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        Ok(self.tree.remove(id.as_bytes())?.is_some())
    }

    pub fn list(&self) -> Result<Vec<Folder>, AppError> {
        let mut folders = Vec::new();
        for item in self.tree.iter() {
            let (_, value) = item?;
            let mut folder: Folder = bincode::deserialize(&value)?;
            // Calculate paste count on demand
            folder.paste_count = self.get_paste_count(&folder.id)?;
            folders.push(folder);
        }
        folders.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(folders)
    }

    /// Calculate paste count for a folder on demand
    fn get_paste_count(&self, folder_id: &str) -> Result<usize, AppError> {
        let mut count = 0;
        for item in self.pastes_tree.iter() {
            let (_, value) = item?;
            let paste: crate::models::paste::Paste = bincode::deserialize(&value)?;
            if paste.folder_id.as_ref() == Some(&folder_id.to_string()) {
                count += 1;
            }
        }
        Ok(count)
    }

    /// No-op: counts are calculated on demand now
    pub fn update_count(&self, _id: &str, _delta: i32) -> Result<(), AppError> {
        // This is now a no-op since we calculate counts on demand
        Ok(())
    }
}
