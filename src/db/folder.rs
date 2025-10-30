use crate::{error::AppError, models::folder::*};
use sled::Db;
use std::sync::Arc;

pub struct FolderDb {
    tree: sled::Tree,
}

impl FolderDb {
    pub fn new(db: Arc<Db>) -> Result<Self, AppError> {
        let tree = db.open_tree("folders")?;
        Ok(Self { tree })
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

    pub fn update(
        &self,
        id: &str,
        name: String,
        parent_id: Option<String>,
    ) -> Result<Option<Folder>, AppError> {
        let result = self.tree.update_and_fetch(id.as_bytes(), move |old| {
            let name = name.clone();
            let parent_id = parent_id.clone();
            old.and_then(|bytes| {
                let mut folder: Folder = bincode::deserialize(bytes).ok()?;
                folder.name = name.clone();
                if let Some(ref pid) = parent_id {
                    folder.parent_id = if pid.is_empty() {
                        None
                    } else {
                        Some(pid.clone())
                    };
                }
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
            let folder: Folder = bincode::deserialize(&value)?;
            folders.push(folder);
        }
        folders.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(folders)
    }

    pub fn update_count(&self, id: &str, delta: i32) -> Result<(), AppError> {
        self.tree.fetch_and_update(id.as_bytes(), |old| {
            old.and_then(|bytes| {
                let mut folder: Folder = bincode::deserialize(bytes).ok()?;
                if delta > 0 {
                    folder.paste_count = folder.paste_count.saturating_add(delta as usize);
                } else {
                    folder.paste_count = folder.paste_count.saturating_sub((-delta) as usize);
                }
                bincode::serialize(&folder).ok()
            })
        })?;
        Ok(())
    }
}
