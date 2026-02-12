//! Paste storage operations backed by sled.

use crate::{error::AppError, models::paste::*};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// Accessor for the `pastes` sled tree.
pub struct PasteDb {
    tree: sled::Tree,
}

impl PasteDb {
    /// Open the `pastes` tree.
    ///
    /// # Returns
    /// A [`PasteDb`] bound to the `pastes` tree.
    ///
    /// # Errors
    /// Returns an error if the tree cannot be opened.
    pub fn new(db: Arc<Db>) -> Result<Self, AppError> {
        let tree = db.open_tree("pastes")?;
        Ok(Self { tree })
    }

    /// Insert a new paste.
    ///
    /// # Returns
    /// `Ok(())` on success.
    ///
    /// # Errors
    /// Returns an error if serialization or insertion fails.
    pub fn create(&self, paste: &Paste) -> Result<(), AppError> {
        let key = paste.id.as_bytes();
        let value = bincode::serialize(paste)?;
        self.tree.insert(key, value)?;
        Ok(())
    }

    /// Fetch a paste by id.
    ///
    /// # Returns
    /// The paste if it exists.
    ///
    /// # Errors
    /// Returns an error if the lookup fails.
    pub fn get(&self, id: &str) -> Result<Option<Paste>, AppError> {
        match self.tree.get(id.as_bytes())? {
            Some(value) => Ok(Some(deserialize_paste(&value)?)),
            None => Ok(None),
        }
    }

    /// Update a paste by id.
    ///
    /// # Arguments
    /// - `id`: Paste identifier.
    /// - `update`: Update payload to apply.
    ///
    /// # Returns
    /// Updated paste if it exists.
    ///
    /// # Errors
    /// Returns an error if the update fails.
    pub fn update(&self, id: &str, update: UpdatePasteRequest) -> Result<Option<Paste>, AppError> {
        let update_error = Rc::new(RefCell::new(None));
        let update_error_in = Rc::clone(&update_error);
        let result = self.tree.update_and_fetch(id.as_bytes(), move |old| {
            let bytes = old?;

            let mut paste = match deserialize_paste(bytes) {
                Ok(paste) => paste,
                Err(err) => {
                    *update_error_in.borrow_mut() = Some(AppError::Serialization(err));
                    return Some(bytes.to_vec());
                }
            };

            apply_update_request(&mut paste, &update);
            match bincode::serialize(&paste) {
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

    /// Update a paste only when its current folder matches `expected_folder_id`.
    ///
    /// This is used as a compare-and-swap guard for cross-tree folder count updates.
    ///
    /// # Arguments
    /// - `id`: Paste identifier.
    /// - `expected_folder_id`: Folder id expected to be on the current record.
    /// - `update`: Update payload to apply when the expected folder matches.
    ///
    /// # Returns
    /// Updated paste if it exists *and* the expected folder matches.
    ///
    /// # Errors
    /// Returns an error if deserialization, serialization, or storage update fails.
    pub fn update_if_folder_matches(
        &self,
        id: &str,
        expected_folder_id: Option<&str>,
        update: UpdatePasteRequest,
    ) -> Result<Option<Paste>, AppError> {
        let update_error = Rc::new(RefCell::new(None));
        let update_error_in = Rc::clone(&update_error);
        let folder_mismatch = Rc::new(Cell::new(false));
        let folder_mismatch_in = Rc::clone(&folder_mismatch);
        let result = self.tree.update_and_fetch(id.as_bytes(), move |old| {
            let bytes = old?;

            let mut paste = match deserialize_paste(bytes) {
                Ok(paste) => paste,
                Err(err) => {
                    *update_error_in.borrow_mut() = Some(AppError::Serialization(err));
                    return Some(bytes.to_vec());
                }
            };

            if !folder_matches_expected(
                paste.folder_id.as_deref(),
                expected_folder_id,
                &folder_mismatch_in,
            ) {
                return Some(bytes.to_vec());
            }

            apply_update_request(&mut paste, &update);
            match bincode::serialize(&paste) {
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
        if folder_mismatch.get() {
            return Ok(None);
        }

        match result {
            Some(bytes) => Ok(Some(bincode::deserialize(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Delete a paste by id.
    ///
    /// # Returns
    /// Deleted paste if it existed.
    ///
    /// # Errors
    /// Returns an error if deletion fails or the deleted value cannot be decoded.
    pub fn delete_and_return(&self, id: &str) -> Result<Option<Paste>, AppError> {
        match self.tree.remove(id.as_bytes())? {
            Some(value) => Ok(Some(deserialize_paste(&value)?)),
            None => Ok(None),
        }
    }

    /// Delete a paste by id.
    ///
    /// # Returns
    /// `true` if a paste was deleted.
    ///
    /// # Errors
    /// Returns an error if deletion fails.
    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        Ok(self.delete_and_return(id)?.is_some())
    }

    /// List pastes with an optional folder filter.
    ///
    /// # Arguments
    /// - `limit`: Maximum number of pastes to return.
    /// - `folder_id`: Optional folder id to filter by.
    ///
    /// # Returns
    /// Pastes sorted by most recently updated.
    ///
    /// # Errors
    /// Returns an error if iteration fails.
    pub fn list(&self, limit: usize, folder_id: Option<String>) -> Result<Vec<Paste>, AppError> {
        let mut pastes = Vec::new();

        // Collect all pastes (or filtered by folder)
        for item in self.tree.iter() {
            let (_, value) = item?;
            let paste = deserialize_paste(&value)?;

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

    /// Search pastes by query with optional filters.
    ///
    /// # Arguments
    /// - `query`: Search term.
    /// - `limit`: Maximum number of results.
    /// - `folder_id`: Optional folder filter.
    /// - `language`: Optional language filter.
    ///
    /// # Returns
    /// Matching pastes sorted by score.
    ///
    /// # Errors
    /// Returns an error if iteration fails.
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
            let paste = deserialize_paste(&value)?;

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

fn apply_update_request(paste: &mut Paste, update: &UpdatePasteRequest) {
    let mut content_changed = false;

    if let Some(content) = &update.content {
        paste.content = content.clone();
        paste.is_markdown = paste.content.contains("```") || paste.content.contains('#');
        content_changed = true;
    }
    if let Some(name) = &update.name {
        paste.name = name.clone();
    }
    if let Some(language) = &update.language {
        paste.language = Some(language.clone());
        if update.language_is_manual.is_none() {
            paste.language_is_manual = true;
        }
    }
    if let Some(is_manual) = update.language_is_manual {
        paste.language_is_manual = is_manual;
    }
    let should_auto_detect = update.language.is_none()
        && !paste.language_is_manual
        && (content_changed || update.language_is_manual == Some(false));
    if should_auto_detect {
        paste.language = detect_language(&paste.content);
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
}

fn folder_matches_expected(
    current_folder_id: Option<&str>,
    expected_folder_id: Option<&str>,
    folder_mismatch: &Cell<bool>,
) -> bool {
    // update_and_fetch may retry the closure under contention; mismatch tracking
    // must represent only the latest attempt.
    folder_mismatch.set(false);
    if current_folder_id != expected_folder_id {
        folder_mismatch.set(true);
        return false;
    }
    true
}

fn deserialize_paste(bytes: &[u8]) -> Result<Paste, bincode::Error> {
    bincode::deserialize::<Paste>(bytes).or_else(|err| {
        bincode::deserialize::<LegacyPaste>(bytes)
            .map(Paste::from)
            .map_err(|_| err)
    })
}

#[derive(Serialize, Deserialize)]
struct LegacyPaste {
    id: String,
    name: String,
    content: String,
    language: Option<String>,
    folder_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    tags: Vec<String>,
    is_markdown: bool,
}

impl From<LegacyPaste> for Paste {
    fn from(old: LegacyPaste) -> Self {
        Self {
            id: old.id,
            name: old.name,
            content: old.content,
            language: old.language.clone(),
            language_is_manual: old.language.is_some(),
            folder_id: old.folder_id,
            created_at: old.created_at,
            updated_at: old.updated_at,
            tags: old.tags,
            is_markdown: old.is_markdown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::folder_matches_expected;
    use std::cell::Cell;

    #[test]
    fn folder_mismatch_state_tracks_latest_retry_attempt() {
        let mismatch = Cell::new(false);

        // First attempt mismatches expected folder.
        assert!(!folder_matches_expected(
            Some("folder-a"),
            Some("folder-b"),
            &mismatch
        ));
        assert!(mismatch.get());

        // Retry attempt matches; state must be cleared for final evaluation.
        assert!(folder_matches_expected(
            Some("folder-a"),
            Some("folder-a"),
            &mismatch
        ));
        assert!(!mismatch.get());
    }
}
