use crate::{error::AppError, models::paste::*};
use sled::Db;
use std::sync::Arc;

pub struct PasteDb {
    tree: sled::Tree,
}

impl PasteDb {
    pub fn new(db: Arc<Db>) -> Result<Self, AppError> {
        let tree = db.open_tree("pastes")?;
        Ok(Self { tree })
    }

    pub fn create(&self, paste: &Paste) -> Result<(), AppError> {
        let key = paste.id.as_bytes();
        let value = bincode::serialize(paste)?;
        self.tree.insert(key, value)?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Paste>, AppError> {
        Ok(self
            .tree
            .get(id.as_bytes())?
            .map(|v| bincode::deserialize(&v))
            .transpose()?)
    }

    pub fn update(&self, id: &str, update: UpdatePasteRequest) -> Result<Option<Paste>, AppError> {
        let result = self.tree.fetch_and_update(id.as_bytes(), |old| {
            old.and_then(|bytes| {
                let mut paste: Paste = bincode::deserialize(bytes).ok()?;

                if let Some(content) = &update.content {
                    paste.content = content.clone();
                    paste.is_markdown =
                        paste.content.contains("```") || paste.content.contains('#');
                }
                if let Some(name) = &update.name {
                    paste.name = name.clone();
                }
                if update.language.is_some() {
                    paste.language = update.language.clone();
                }
                // Normalize folder_id: empty string becomes None
                if let Some(ref fid) = update.folder_id {
                    paste.folder_id = if fid.is_empty() {
                        None
                    } else {
                        Some(fid.clone())
                    };
                }
                if let Some(tags) = &update.tags {
                    paste.tags = tags.clone();
                }

                paste.updated_at = chrono::Utc::now();
                bincode::serialize(&paste).ok()
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

    pub fn list(&self, limit: usize, folder_id: Option<String>) -> Result<Vec<Paste>, AppError> {
        let mut pastes = Vec::new();

        // Collect all pastes (or filtered by folder)
        for item in self.tree.iter() {
            let (_, value) = item?;
            let paste: Paste = bincode::deserialize(&value)?;

            if let Some(ref fid) = folder_id {
                if paste.folder_id.as_ref() != Some(fid) {
                    continue;
                }
            }
            pastes.push(paste);
        }

        // Sort by updated_at in descending order (newest first)
        pastes.sort_by_key(|p| std::cmp::Reverse(p.updated_at));

        // Truncate to limit
        pastes.truncate(limit);

        Ok(pastes)
    }

    pub fn search(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
    ) -> Result<Vec<Paste>, AppError> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for item in self.tree.iter() {
            let (_, value) = item?;
            let paste: Paste = bincode::deserialize(&value)?;

            // Apply folder filter
            if let Some(ref fid) = folder_id {
                if paste.folder_id.as_ref() != Some(fid) {
                    continue;
                }
            }

            // Apply language filter
            if let Some(ref lang) = language {
                if paste.language.as_ref() != Some(lang) {
                    continue;
                }
            }

            let mut score = 0;
            if paste.name.to_lowercase().contains(&query_lower) {
                score += 10;
            }
            if paste
                .tags
                .iter()
                .any(|t| t.to_lowercase().contains(&query_lower))
            {
                score += 5;
            }
            if paste.content.to_lowercase().contains(&query_lower) {
                score += 1;
            }

            if score > 0 {
                results.push((score, paste));
            }
        }

        results.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(results
            .into_iter()
            .take(limit)
            .map(|(_, paste)| paste)
            .collect())
    }
}
