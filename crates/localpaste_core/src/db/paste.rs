//! Paste storage operations backed by sled.

use crate::{error::AppError, models::paste::*};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

const META_STATE_TREE_NAME: &str = "pastes_meta_state";
const META_INDEX_VERSION_KEY: &[u8] = b"version";
const META_INDEX_SCHEMA_VERSION: u32 = 1;

/// Accessor for the `pastes` sled tree.
pub struct PasteDb {
    tree: sled::Tree,
    meta_tree: sled::Tree,
    updated_tree: sled::Tree,
    meta_state_tree: sled::Tree,
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
        let meta_tree = db.open_tree("pastes_meta")?;
        let updated_tree = db.open_tree("pastes_by_updated")?;
        let meta_state_tree = db.open_tree(META_STATE_TREE_NAME)?;
        Ok(Self {
            tree,
            meta_tree,
            updated_tree,
            meta_state_tree,
        })
    }

    pub(crate) fn needs_reconcile_meta_indexes(
        &self,
        force_reindex: bool,
    ) -> Result<bool, AppError> {
        if force_reindex {
            tracing::warn!("LOCALPASTE_REINDEX is enabled; forcing metadata index reconcile");
            return Ok(true);
        }
        let marker_version = self.meta_index_schema_version()?;
        if marker_version != Some(META_INDEX_SCHEMA_VERSION) {
            return Ok(true);
        }

        let pastes_empty = self.tree.is_empty();
        let meta_empty = self.meta_tree.is_empty();
        let updated_empty = self.updated_tree.is_empty();

        if !pastes_empty && (meta_empty || updated_empty) {
            return Ok(true);
        }
        if pastes_empty && (!meta_empty || !updated_empty) {
            return Ok(true);
        }
        Ok(false)
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
        self.upsert_meta_and_index_from_paste(paste, None)?;
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
        let old_meta = Rc::new(RefCell::new(None));
        let old_meta_in = Rc::clone(&old_meta);
        let result = self.tree.update_and_fetch(id.as_bytes(), move |old| {
            let bytes = old?;

            let mut paste = match deserialize_paste(bytes) {
                Ok(paste) => paste,
                Err(err) => {
                    *update_error_in.borrow_mut() = Some(AppError::Serialization(err));
                    return Some(bytes.to_vec());
                }
            };

            *old_meta_in.borrow_mut() = Some(PasteMeta::from(&paste));
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
            Some(bytes) => {
                let paste = deserialize_paste(&bytes)?;
                self.upsert_meta_and_index_from_paste(&paste, old_meta.borrow_mut().take())?;
                Ok(Some(paste))
            }
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
        let old_meta = Rc::new(RefCell::new(None));
        let old_meta_in = Rc::clone(&old_meta);
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

            *old_meta_in.borrow_mut() = Some(PasteMeta::from(&paste));
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
            Some(bytes) => {
                let paste = deserialize_paste(&bytes)?;
                self.upsert_meta_and_index_from_paste(&paste, old_meta.borrow_mut().take())?;
                Ok(Some(paste))
            }
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
            Some(value) => {
                let paste = deserialize_paste(&value)?;
                let meta = PasteMeta::from(&paste);
                self.remove_meta_and_index(&meta)?;
                Ok(Some(paste))
            }
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

    /// List paste metadata with an optional folder filter.
    ///
    /// # Arguments
    /// - `limit`: Maximum number of metadata rows to return.
    /// - `folder_id`: Optional folder id to filter by.
    ///
    /// # Returns
    /// Metadata rows sorted by most recently updated.
    ///
    /// # Errors
    /// Returns an error if iteration or deserialization fails.
    pub fn list_meta(
        &self,
        limit: usize,
        folder_id: Option<String>,
    ) -> Result<Vec<PasteMeta>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut metas = Vec::with_capacity(limit);
        for item in self.updated_tree.iter() {
            let (_, value) = item?;
            let id = match std::str::from_utf8(value.as_ref()) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let Some(meta_bytes) = self.meta_tree.get(id.as_bytes())? else {
                continue;
            };
            let meta = deserialize_meta(&meta_bytes)?;
            if let Some(ref fid) = folder_id {
                if meta.folder_id.as_ref() != Some(fid) {
                    continue;
                }
            }
            metas.push(meta);
            if metas.len() >= limit {
                break;
            }
        }
        Ok(metas)
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

    /// Search paste metadata by query with optional filters.
    ///
    /// Metadata-only search matches name/tags/language and does not scan content.
    ///
    /// # Arguments
    /// - `query`: Search term.
    /// - `limit`: Maximum number of results.
    /// - `folder_id`: Optional folder filter.
    /// - `language`: Optional language filter.
    ///
    /// # Returns
    /// Matching metadata rows sorted by score and recency.
    ///
    /// # Errors
    /// Returns an error if iteration fails.
    pub fn search_meta(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
    ) -> Result<Vec<PasteMeta>, AppError> {
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let query_lower = query.to_lowercase();
        let mut results: Vec<(i32, DateTime<Utc>, PasteMeta)> = Vec::new();
        for item in self.meta_tree.iter() {
            let (_, value) = item?;
            let meta = deserialize_meta(&value)?;

            if let Some(ref fid) = folder_id {
                if meta.folder_id.as_ref() != Some(fid) {
                    continue;
                }
            }
            if let Some(ref lang_filter) = language {
                if meta.language.as_ref() != Some(lang_filter) {
                    continue;
                }
            }

            let mut score = 0;
            if meta.name.to_lowercase().contains(&query_lower) {
                score += 10;
            }
            if meta
                .tags
                .iter()
                .any(|tag| tag.to_lowercase().contains(&query_lower))
            {
                score += 5;
            }
            if meta
                .language
                .as_ref()
                .map(|lang| lang.to_lowercase().contains(&query_lower))
                .unwrap_or(false)
            {
                score += 2;
            }
            if score > 0 {
                results.push((score, meta.updated_at, meta));
            }
        }
        results.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        Ok(results
            .into_iter()
            .take(limit)
            .map(|(_, _, meta)| meta)
            .collect())
    }

    /// Rebuild metadata and recency indexes from the canonical `pastes` tree.
    ///
    /// # Returns
    /// `Ok(())` when indexes are rebuilt successfully.
    ///
    /// # Errors
    /// Returns an error if index rebuild fails.
    pub fn reconcile_meta_indexes(&self) -> Result<(), AppError> {
        self.meta_tree.clear()?;
        self.updated_tree.clear()?;
        for item in self.tree.iter() {
            let (_, value) = item?;
            let paste = deserialize_paste(&value)?;
            self.upsert_meta_and_index_from_paste(&paste, None)?;
        }
        self.meta_tree.flush()?;
        self.updated_tree.flush()?;
        self.write_meta_index_schema_version()?;
        Ok(())
    }

    fn meta_index_schema_version(&self) -> Result<Option<u32>, AppError> {
        let Some(raw) = self.meta_state_tree.get(META_INDEX_VERSION_KEY)? else {
            return Ok(None);
        };
        if raw.len() != std::mem::size_of::<u32>() {
            tracing::warn!(
                "Metadata index state marker has invalid length {}; forcing reconcile",
                raw.len()
            );
            return Ok(None);
        }
        let mut bytes = [0u8; std::mem::size_of::<u32>()];
        bytes.copy_from_slice(raw.as_ref());
        Ok(Some(u32::from_be_bytes(bytes)))
    }

    fn write_meta_index_schema_version(&self) -> Result<(), AppError> {
        self.meta_state_tree.insert(
            META_INDEX_VERSION_KEY,
            META_INDEX_SCHEMA_VERSION.to_be_bytes().to_vec(),
        )?;
        self.meta_state_tree.flush()?;
        Ok(())
    }

    fn upsert_meta_and_index_from_paste(
        &self,
        paste: &Paste,
        previous: Option<PasteMeta>,
    ) -> Result<(), AppError> {
        let meta = PasteMeta::from(paste);
        self.upsert_meta_and_index(&meta, previous.as_ref())
    }

    fn upsert_meta_and_index(
        &self,
        meta: &PasteMeta,
        previous: Option<&PasteMeta>,
    ) -> Result<(), AppError> {
        if let Some(previous) = previous {
            self.remove_index_entry(previous)?;
        }
        let meta_bytes = bincode::serialize(meta)?;
        self.meta_tree.insert(meta.id.as_bytes(), meta_bytes)?;
        let recency_key = index_key(meta.updated_at, meta.id.as_str());
        self.updated_tree.insert(recency_key, meta.id.as_bytes())?;
        Ok(())
    }

    fn remove_meta_and_index(&self, meta: &PasteMeta) -> Result<(), AppError> {
        self.meta_tree.remove(meta.id.as_bytes())?;
        self.remove_index_entry(meta)?;
        Ok(())
    }

    fn remove_index_entry(&self, meta: &PasteMeta) -> Result<(), AppError> {
        let recency_key = index_key(meta.updated_at, meta.id.as_str());
        self.updated_tree.remove(recency_key)?;
        Ok(())
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

fn deserialize_meta(bytes: &[u8]) -> Result<PasteMeta, bincode::Error> {
    bincode::deserialize(bytes)
}

fn index_key(updated_at: DateTime<Utc>, id: &str) -> Vec<u8> {
    let millis = updated_at.timestamp_millis().max(0) as u64;
    let reverse = u64::MAX.saturating_sub(millis);
    let mut key = Vec::with_capacity(8 + id.len());
    key.extend_from_slice(&reverse.to_be_bytes());
    key.extend_from_slice(id.as_bytes());
    key
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
    use super::{
        folder_matches_expected, PasteDb, META_INDEX_SCHEMA_VERSION, META_INDEX_VERSION_KEY,
    };
    use crate::models::paste::Paste;
    use std::cell::Cell;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_paste_db() -> (PasteDb, TempDir) {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        let db = Arc::new(sled::open(db_path).expect("open sled"));
        let paste_db = PasteDb::new(db).expect("open paste db");
        (paste_db, dir)
    }

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

    #[test]
    fn needs_reconcile_detects_missing_marker() {
        let (paste_db, _dir) = setup_paste_db();
        assert!(paste_db.meta_tree.is_empty());
        assert!(paste_db.updated_tree.is_empty());
        assert!(paste_db
            .meta_state_tree
            .get(META_INDEX_VERSION_KEY)
            .expect("state lookup")
            .is_none());
        assert!(paste_db
            .needs_reconcile_meta_indexes(false)
            .expect("needs reconcile"));
    }

    #[test]
    fn needs_reconcile_detects_missing_index_rows_with_marker() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        let paste = Paste::new("hello".to_string(), "hello".to_string());
        paste_db.create(&paste).expect("create paste");

        paste_db.meta_tree.clear().expect("clear meta");
        assert!(paste_db
            .meta_state_tree
            .get(META_INDEX_VERSION_KEY)
            .expect("state lookup")
            .is_some());
        assert!(paste_db
            .needs_reconcile_meta_indexes(false)
            .expect("needs reconcile"));
    }

    #[test]
    fn needs_reconcile_returns_false_when_marker_and_indexes_are_healthy() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        let marker = paste_db
            .meta_state_tree
            .get(META_INDEX_VERSION_KEY)
            .expect("state lookup")
            .expect("state marker");
        assert_eq!(
            u32::from_be_bytes(marker.as_ref().try_into().expect("version bytes")),
            META_INDEX_SCHEMA_VERSION
        );
        assert!(!paste_db
            .needs_reconcile_meta_indexes(false)
            .expect("needs reconcile"));
    }

    #[test]
    fn needs_reconcile_honors_force_reindex_flag() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        assert!(paste_db
            .needs_reconcile_meta_indexes(true)
            .expect("needs reconcile"));
    }
}
