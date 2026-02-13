//! Paste storage operations backed by sled.

use crate::{error::AppError, models::paste::*};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

const META_STATE_TREE_NAME: &str = "pastes_meta_state";
const META_INDEX_VERSION_KEY: &[u8] = b"version";
const META_INDEX_IN_PROGRESS_COUNT_KEY: &[u8] = b"in_progress_count";
const META_INDEX_FAULTED_KEY: &[u8] = b"faulted";
const META_INDEX_SCHEMA_VERSION: u32 = 3;

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
        if self.meta_index_faulted()? {
            return Ok(true);
        }
        if self.meta_index_in_progress_count()? > 0 {
            return Ok(true);
        }

        let pastes_empty = self.tree.is_empty();
        let meta_empty = self.meta_tree.is_empty();
        let updated_empty = self.updated_tree.is_empty();
        let paste_len = self.tree.len();

        if !pastes_empty && (meta_empty || updated_empty) {
            return Ok(true);
        }
        if pastes_empty && (!meta_empty || !updated_empty) {
            return Ok(true);
        }
        if self.meta_tree.len() != paste_len || self.updated_tree.len() != paste_len {
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
        self.create_inner(paste, |db, paste| {
            db.upsert_meta_and_index_from_paste(paste, None)
        })
    }

    fn create_inner<F>(&self, paste: &Paste, index_writer: F) -> Result<(), AppError>
    where
        F: FnOnce(&Self, &Paste) -> Result<(), AppError>,
    {
        self.begin_meta_index_mutation()?;
        let key = paste.id.as_bytes();
        let value = match bincode::serialize(paste) {
            Ok(value) => value,
            Err(err) => {
                self.try_end_meta_index_mutation();
                return Err(AppError::Serialization(err));
            }
        };
        let _previous = self.tree.insert(key, value)?;
        let index_result = index_writer(self, paste);
        self.finalize_derived_index_write("create", paste.id.as_str(), index_result);
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
        self.update_inner(id, update, |db, paste, previous| {
            db.upsert_meta_and_index_from_paste(paste, previous)
        })
    }

    fn update_inner<F>(
        &self,
        id: &str,
        update: UpdatePasteRequest,
        index_writer: F,
    ) -> Result<Option<Paste>, AppError>
    where
        F: FnOnce(&Self, &Paste, Option<PasteMeta>) -> Result<(), AppError>,
    {
        self.begin_meta_index_mutation()?;
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
            self.try_end_meta_index_mutation();
            return Err(err);
        }

        match result {
            Some(bytes) => {
                let paste = deserialize_paste(&bytes)?;
                let previous = old_meta.borrow_mut().take();
                let index_result = index_writer(self, &paste, previous);
                self.finalize_derived_index_write("update", paste.id.as_str(), index_result);
                Ok(Some(paste))
            }
            None => {
                self.try_end_meta_index_mutation();
                Ok(None)
            }
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
        self.update_if_folder_matches_inner(
            id,
            expected_folder_id,
            update,
            |db, paste, previous| db.upsert_meta_and_index_from_paste(paste, previous),
        )
    }

    fn update_if_folder_matches_inner<F>(
        &self,
        id: &str,
        expected_folder_id: Option<&str>,
        update: UpdatePasteRequest,
        index_writer: F,
    ) -> Result<Option<Paste>, AppError>
    where
        F: FnOnce(&Self, &Paste, Option<PasteMeta>) -> Result<(), AppError>,
    {
        self.begin_meta_index_mutation()?;
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
            self.try_end_meta_index_mutation();
            return Err(err);
        }
        if folder_mismatch.get() {
            self.try_end_meta_index_mutation();
            return Ok(None);
        }

        match result {
            Some(bytes) => {
                let paste = deserialize_paste(&bytes)?;
                let previous = old_meta.borrow_mut().take();
                let index_result = index_writer(self, &paste, previous);
                self.finalize_derived_index_write(
                    "update_if_folder_matches",
                    paste.id.as_str(),
                    index_result,
                );
                Ok(Some(paste))
            }
            None => {
                self.try_end_meta_index_mutation();
                Ok(None)
            }
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
        self.delete_and_return_inner(id, |db, meta| db.remove_meta_and_index(meta))
    }

    fn delete_and_return_inner<F>(
        &self,
        id: &str,
        index_remover: F,
    ) -> Result<Option<Paste>, AppError>
    where
        F: FnOnce(&Self, &PasteMeta) -> Result<(), AppError>,
    {
        self.begin_meta_index_mutation()?;
        match self.tree.remove(id.as_bytes())? {
            Some(value) => {
                let paste = deserialize_paste(&value)?;
                let meta = PasteMeta::from(&paste);
                let index_result = index_remover(self, &meta);
                self.finalize_derived_index_write("delete", paste.id.as_str(), index_result);
                Ok(Some(paste))
            }
            None => {
                self.try_end_meta_index_mutation();
                Ok(None)
            }
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
        if !self.meta_indexes_usable()? {
            tracing::warn!("Metadata indexes are dirty/unavailable; listing from canonical tree");
            return self.list_meta_from_canonical(limit, folder_id);
        }

        let mut metas = Vec::with_capacity(limit);
        let mut seen_ids = HashSet::with_capacity(limit);
        for item in self.updated_tree.iter() {
            let (_, value) = item?;
            let id = match std::str::from_utf8(value.as_ref()) {
                Ok(id) => id,
                Err(_) => {
                    tracing::warn!(
                        "Metadata updated index contains non-UTF8 id; listing from canonical tree"
                    );
                    return self.list_meta_from_canonical(limit, folder_id);
                }
            };
            if !seen_ids.insert(id.to_string()) {
                continue;
            }
            let Some(meta_bytes) = self.meta_tree.get(id.as_bytes())? else {
                tracing::warn!(
                    "Metadata index missing meta row for id '{}'; listing from canonical tree",
                    id
                );
                return self.list_meta_from_canonical(limit, folder_id);
            };
            let meta = match deserialize_meta(&meta_bytes) {
                Ok(meta) => meta,
                Err(err) => {
                    tracing::warn!(
                        "Failed to decode metadata row for id '{}': {}; listing from canonical tree",
                        id,
                        err
                    );
                    return self.list_meta_from_canonical(limit, folder_id);
                }
            };
            if meta.id != id {
                tracing::warn!(
                    "Metadata id mismatch for updated index id '{}'; listing from canonical tree",
                    id
                );
                return self.list_meta_from_canonical(limit, folder_id);
            }
            if self.tree.get(id.as_bytes())?.is_none() {
                tracing::warn!(
                    "Metadata row for id '{}' has no canonical paste; listing from canonical tree",
                    id
                );
                return self.list_meta_from_canonical(limit, folder_id);
            }
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
    /// Matching metadata rows sorted by score and recency.
    ///
    /// # Errors
    /// Returns an error if iteration fails.
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
    ) -> Result<Vec<PasteMeta>, AppError> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let query_lower = query.to_lowercase();
        let mut results: Vec<(i32, DateTime<Utc>, PasteMeta)> = Vec::new();
        let language_filter = normalized_language_filter(language.as_deref());

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
            if !language_matches_filter(paste.language.as_deref(), language_filter.as_deref()) {
                continue;
            }

            let score = score_paste_match(&paste, &query_lower);

            if score > 0 {
                let meta = PasteMeta::from(&paste);
                push_ranked_meta_top_k(&mut results, (score, meta.updated_at, meta), limit);
            }
        }

        Ok(finalize_meta_search_results(results, limit))
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
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        if !self.meta_indexes_usable()? {
            tracing::warn!("Metadata indexes are dirty/unavailable; searching via canonical tree");
            return self.search_meta_from_canonical(query, limit, folder_id, language);
        }

        let query_lower = query.to_lowercase();
        let language_filter = normalized_language_filter(language.as_deref());
        let mut results: Vec<(i32, DateTime<Utc>, PasteMeta)> = Vec::new();
        for item in self.meta_tree.iter() {
            let (_, value) = item?;
            let meta = match deserialize_meta(&value) {
                Ok(meta) => meta,
                Err(err) => {
                    tracing::warn!(
                        "Failed to decode metadata row during search: {}; falling back to canonical tree",
                        err
                    );
                    return self.search_meta_from_canonical(query, limit, folder_id, language);
                }
            };
            if self.tree.get(meta.id.as_bytes())?.is_none() {
                tracing::warn!(
                    "Metadata search encountered ghost row for id '{}'; falling back to canonical tree",
                    meta.id
                );
                return self.search_meta_from_canonical(query, limit, folder_id, language);
            }

            if !meta_matches_filters(&meta, folder_id.as_deref(), language_filter.as_deref()) {
                continue;
            }
            let score = score_meta_match(&meta, &query_lower);
            if score > 0 {
                push_ranked_meta_top_k(&mut results, (score, meta.updated_at, meta), limit);
            }
        }
        Ok(finalize_meta_search_results(results, limit))
    }

    /// Rebuild metadata and recency indexes from the canonical `pastes` tree.
    ///
    /// # Returns
    /// `Ok(())` when indexes are rebuilt successfully.
    ///
    /// # Errors
    /// Returns an error if index rebuild fails.
    pub fn reconcile_meta_indexes(&self) -> Result<(), AppError> {
        self.begin_meta_index_mutation()?;
        self.meta_tree.clear()?;
        self.updated_tree.clear()?;
        for item in self.tree.iter() {
            let (_, value) = item?;
            let paste = deserialize_paste(&value)?;
            self.upsert_meta_and_index_from_paste(&paste, None)?;
        }
        self.meta_tree.flush()?;
        self.updated_tree.flush()?;
        self.write_meta_index_state(META_INDEX_SCHEMA_VERSION, 0, false)?;
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

    fn meta_index_in_progress_count(&self) -> Result<u64, AppError> {
        let Some(raw) = self.meta_state_tree.get(META_INDEX_IN_PROGRESS_COUNT_KEY)? else {
            return Ok(0);
        };
        if raw.len() != std::mem::size_of::<u64>() {
            tracing::warn!(
                "Metadata index in-progress marker has invalid length {}; forcing reconcile",
                raw.len()
            );
            return Ok(1);
        }
        let mut bytes = [0u8; std::mem::size_of::<u64>()];
        bytes.copy_from_slice(raw.as_ref());
        Ok(u64::from_be_bytes(bytes))
    }

    fn meta_index_faulted(&self) -> Result<bool, AppError> {
        let Some(raw) = self.meta_state_tree.get(META_INDEX_FAULTED_KEY)? else {
            return Ok(false);
        };
        if raw.len() != std::mem::size_of::<u8>() {
            tracing::warn!(
                "Metadata index faulted marker has invalid length {}; forcing reconcile",
                raw.len()
            );
            return Ok(true);
        }
        Ok(raw[0] != 0)
    }

    fn write_meta_index_state(
        &self,
        version: u32,
        in_progress_count: u64,
        faulted: bool,
    ) -> Result<(), AppError> {
        self.meta_state_tree
            .insert(META_INDEX_VERSION_KEY, version.to_be_bytes().to_vec())?;
        self.meta_state_tree.insert(
            META_INDEX_IN_PROGRESS_COUNT_KEY,
            in_progress_count.to_be_bytes().to_vec(),
        )?;
        self.meta_state_tree
            .insert(META_INDEX_FAULTED_KEY, vec![u8::from(faulted)])?;
        // Remove v2 marker when upgrading to v3 state.
        self.meta_state_tree.remove(b"dirty_count")?;
        self.meta_state_tree.flush()?;
        Ok(())
    }

    fn begin_meta_index_mutation(&self) -> Result<(), AppError> {
        let _ = self
            .meta_state_tree
            .update_and_fetch(META_INDEX_IN_PROGRESS_COUNT_KEY, |old| {
                let current = decode_dirty_count(old);
                Some(current.saturating_add(1).to_be_bytes().to_vec())
            })?;
        Ok(())
    }

    fn end_meta_index_mutation(&self) -> Result<(), AppError> {
        let _ = self
            .meta_state_tree
            .update_and_fetch(META_INDEX_IN_PROGRESS_COUNT_KEY, |old| {
                let current = decode_dirty_count(old);
                Some(current.saturating_sub(1).to_be_bytes().to_vec())
            })?;
        Ok(())
    }

    fn mark_meta_index_faulted(&self) {
        if let Err(err) = self
            .meta_state_tree
            .insert(META_INDEX_FAULTED_KEY, vec![1u8])
        {
            tracing::warn!("Failed to mark metadata index as faulted: {}", err);
            return;
        }
        if let Err(err) = self.meta_state_tree.flush() {
            tracing::warn!("Failed to flush metadata index fault marker: {}", err);
        }
    }

    fn try_end_meta_index_mutation(&self) {
        if let Err(err) = self.end_meta_index_mutation() {
            tracing::warn!(
                "Failed to clear metadata index in-progress marker after mutation: {}",
                err
            );
        }
    }

    fn finalize_derived_index_write(
        &self,
        operation: &str,
        paste_id: &str,
        index_result: Result<(), AppError>,
    ) {
        match index_result {
            Ok(()) => self.try_end_meta_index_mutation(),
            Err(err) => {
                // Canonical paste writes are the source of truth. If derived metadata/index
                // maintenance fails after the canonical mutation commits, flag indexes as faulted
                // so readers can safely route through canonical fallback until reconcile.
                self.mark_meta_index_faulted();
                self.try_end_meta_index_mutation();
                tracing::warn!(
                    operation,
                    paste_id,
                    error = %err,
                    "Canonical write committed but metadata index update failed; marked index faulted for canonical fallback/reconcile"
                );
            }
        }
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
        let meta_bytes = bincode::serialize(meta)?;
        self.meta_tree.insert(meta.id.as_bytes(), meta_bytes)?;
        let recency_key = index_key(meta.updated_at, meta.id.as_str());
        self.updated_tree
            .insert(recency_key.clone(), meta.id.as_bytes())?;
        if let Some(previous) = previous {
            let previous_key = index_key(previous.updated_at, previous.id.as_str());
            if previous_key != recency_key {
                self.updated_tree.remove(previous_key)?;
            }
        }
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

    fn meta_indexes_usable(&self) -> Result<bool, AppError> {
        if self.meta_index_schema_version()? != Some(META_INDEX_SCHEMA_VERSION) {
            return Ok(false);
        }
        if self.meta_index_faulted()? {
            return Ok(false);
        }
        Ok(true)
    }

    fn list_meta_from_canonical(
        &self,
        limit: usize,
        folder_id: Option<String>,
    ) -> Result<Vec<PasteMeta>, AppError> {
        let mut ranked: Vec<(DateTime<Utc>, PasteMeta)> = Vec::new();
        for item in self.tree.iter() {
            let (_, value) = item?;
            let paste = deserialize_paste(&value)?;
            if let Some(ref fid) = folder_id {
                if paste.folder_id.as_ref() != Some(fid) {
                    continue;
                }
            }
            let meta = PasteMeta::from(&paste);
            push_recent_meta_top_k(&mut ranked, (meta.updated_at, meta), limit);
        }
        Ok(finalize_recent_meta_results(ranked, limit))
    }

    fn search_meta_from_canonical(
        &self,
        query: &str,
        limit: usize,
        folder_id: Option<String>,
        language: Option<String>,
    ) -> Result<Vec<PasteMeta>, AppError> {
        let query_lower = query.to_lowercase();
        let language_filter = normalized_language_filter(language.as_deref());
        let mut results: Vec<(i32, DateTime<Utc>, PasteMeta)> = Vec::new();
        for item in self.tree.iter() {
            let (_, value) = item?;
            let paste = deserialize_paste(&value)?;
            let meta = PasteMeta::from(&paste);

            if !meta_matches_filters(&meta, folder_id.as_deref(), language_filter.as_deref()) {
                continue;
            }
            let score = score_meta_match(&meta, &query_lower);
            if score > 0 {
                push_ranked_meta_top_k(&mut results, (score, meta.updated_at, meta), limit);
            }
        }
        Ok(finalize_meta_search_results(results, limit))
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

fn normalized_language_filter(language: Option<&str>) -> Option<String> {
    language
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn language_matches_filter(language: Option<&str>, filter: Option<&str>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    language
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.eq_ignore_ascii_case(filter))
        .unwrap_or(false)
}

fn meta_matches_filters(
    meta: &PasteMeta,
    folder_filter: Option<&str>,
    language_filter: Option<&str>,
) -> bool {
    if let Some(folder_id) = folder_filter {
        if meta.folder_id.as_deref() != Some(folder_id) {
            return false;
        }
    }
    language_matches_filter(meta.language.as_deref(), language_filter)
}

fn score_meta_match(meta: &PasteMeta, query_lower: &str) -> i32 {
    let mut score = 0;
    if meta.name.to_lowercase().contains(query_lower) {
        score += 10;
    }
    if meta
        .tags
        .iter()
        .any(|tag| tag.to_lowercase().contains(query_lower))
    {
        score += 5;
    }
    if meta
        .language
        .as_ref()
        .map(|lang| lang.to_lowercase().contains(query_lower))
        .unwrap_or(false)
    {
        score += 2;
    }
    score
}

fn score_paste_match(paste: &Paste, query_lower: &str) -> i32 {
    let mut score = 0;
    if contains_case_insensitive(&paste.name, query_lower) {
        score += 10;
    }
    if paste
        .tags
        .iter()
        .any(|tag| contains_case_insensitive(tag, query_lower))
    {
        score += 5;
    }
    if contains_case_insensitive(&paste.content, query_lower) {
        score += 1;
    }
    score
}

fn push_ranked_meta_top_k(
    results: &mut Vec<(i32, DateTime<Utc>, PasteMeta)>,
    candidate: (i32, DateTime<Utc>, PasteMeta),
    limit: usize,
) {
    push_ranked_top_k(results, candidate, limit);
}

fn push_ranked_top_k<T>(
    results: &mut Vec<(i32, DateTime<Utc>, T)>,
    candidate: (i32, DateTime<Utc>, T),
    limit: usize,
) {
    if limit == 0 {
        return;
    }
    if results.len() < limit {
        results.push(candidate);
        return;
    }

    let Some((worst_idx, worst_entry)) = results
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
    else {
        results.push(candidate);
        return;
    };

    let candidate_better = candidate.0 > worst_entry.0
        || (candidate.0 == worst_entry.0 && candidate.1 > worst_entry.1);
    if candidate_better {
        results[worst_idx] = candidate;
    }
}

fn finalize_meta_search_results(
    mut ranked_results: Vec<(i32, DateTime<Utc>, PasteMeta)>,
    limit: usize,
) -> Vec<PasteMeta> {
    ranked_results.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    ranked_results
        .into_iter()
        .take(limit)
        .map(|(_, _, meta)| meta)
        .collect()
}

fn push_recent_meta_top_k(
    results: &mut Vec<(DateTime<Utc>, PasteMeta)>,
    candidate: (DateTime<Utc>, PasteMeta),
    limit: usize,
) {
    if limit == 0 {
        return;
    }
    if results.len() < limit {
        results.push(candidate);
        return;
    }

    let Some((worst_idx, worst_entry)) = results
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.0.cmp(&right.0))
    else {
        results.push(candidate);
        return;
    };
    if candidate.0 > worst_entry.0 {
        results[worst_idx] = candidate;
    }
}

fn finalize_recent_meta_results(
    mut ranked_results: Vec<(DateTime<Utc>, PasteMeta)>,
    limit: usize,
) -> Vec<PasteMeta> {
    ranked_results.sort_by(|a, b| b.0.cmp(&a.0));
    ranked_results
        .into_iter()
        .take(limit)
        .map(|(_, meta)| meta)
        .collect()
}

fn contains_case_insensitive(haystack: &str, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    if query_lower.is_ascii() {
        let needle = query_lower.as_bytes();
        let hay = haystack.as_bytes();
        if needle.len() > hay.len() {
            return false;
        }
        for idx in 0..=hay.len() - needle.len() {
            if hay[idx..idx + needle.len()]
                .iter()
                .map(u8::to_ascii_lowercase)
                .eq(needle.iter().copied())
            {
                return true;
            }
        }
        return false;
    }
    haystack.to_lowercase().contains(query_lower)
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

fn decode_dirty_count(raw: Option<&[u8]>) -> u64 {
    let Some(raw) = raw else {
        return 0;
    };
    if raw.len() != std::mem::size_of::<u64>() {
        return 0;
    }
    let mut bytes = [0u8; std::mem::size_of::<u64>()];
    bytes.copy_from_slice(raw);
    u64::from_be_bytes(bytes)
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
        folder_matches_expected, push_ranked_meta_top_k, PasteDb, META_INDEX_FAULTED_KEY,
        META_INDEX_IN_PROGRESS_COUNT_KEY, META_INDEX_SCHEMA_VERSION, META_INDEX_VERSION_KEY,
    };
    use crate::models::paste::{Paste, UpdatePasteRequest};
    use crate::AppError;
    use chrono::Duration;
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
    fn needs_reconcile_detects_partial_non_empty_metadata_drift() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        let first = Paste::new("first".to_string(), "first".to_string());
        let second = Paste::new("second".to_string(), "second".to_string());
        paste_db.create(&first).expect("create first");
        paste_db.create(&second).expect("create second");

        paste_db
            .meta_tree
            .remove(first.id.as_bytes())
            .expect("remove first meta");
        paste_db
            .updated_tree
            .remove(super::index_key(first.updated_at, first.id.as_str()))
            .expect("remove first updated index");

        assert!(
            !paste_db.meta_tree.is_empty(),
            "meta tree should remain non-empty"
        );
        assert!(
            !paste_db.updated_tree.is_empty(),
            "updated tree should remain non-empty"
        );
        assert!(
            paste_db
                .meta_state_tree
                .get(META_INDEX_VERSION_KEY)
                .expect("state lookup")
                .is_some(),
            "version marker should remain present"
        );
        assert!(paste_db
            .needs_reconcile_meta_indexes(false)
            .expect("needs reconcile"));
    }

    #[test]
    fn needs_reconcile_skips_deep_scan_when_markers_are_clean() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        let paste = Paste::new("hello".to_string(), "hello".to_string());
        paste_db.create(&paste).expect("create paste");

        paste_db
            .meta_tree
            .insert(paste.id.as_bytes(), b"corrupt-meta-row")
            .expect("insert corrupt meta");
        assert_eq!(paste_db.tree.len(), paste_db.meta_tree.len());
        assert_eq!(paste_db.tree.len(), paste_db.updated_tree.len());
        assert!(!paste_db
            .needs_reconcile_meta_indexes(false)
            .expect("needs reconcile"));
    }

    #[test]
    fn list_meta_falls_back_to_canonical_when_index_is_inconsistent() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        let paste = Paste::new("fallback body".to_string(), "fallback-name".to_string());
        let paste_id = paste.id.clone();
        paste_db.create(&paste).expect("create paste");

        paste_db
            .meta_tree
            .remove(paste_id.as_bytes())
            .expect("remove meta row");

        let metas = paste_db
            .list_meta(10, None)
            .expect("list metadata fallback");
        assert!(
            metas.into_iter().any(|meta| meta.id == paste_id),
            "canonical fallback should retain visibility of canonical rows"
        );
    }

    #[test]
    fn list_meta_omits_ghost_rows_when_canonical_row_is_missing() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        let paste = Paste::new("ghost".to_string(), "ghost".to_string());
        let paste_id = paste.id.clone();
        paste_db.create(&paste).expect("create paste");

        paste_db
            .tree
            .remove(paste_id.as_bytes())
            .expect("remove canonical row");
        assert!(
            paste_db
                .meta_tree
                .get(paste_id.as_bytes())
                .expect("meta lookup")
                .is_some(),
            "meta row should remain to simulate ghost entry"
        );

        let metas = paste_db
            .list_meta(10, None)
            .expect("list metadata fallback");
        assert!(
            metas.into_iter().all(|meta| meta.id != paste_id),
            "canonical fallback should hide ghost metadata rows"
        );
    }

    #[test]
    fn list_meta_dedupes_duplicate_updated_index_entries() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        let paste = Paste::new("body".to_string(), "duplicate-index".to_string());
        let paste_id = paste.id.clone();
        let updated_at = paste.updated_at;
        paste_db.create(&paste).expect("create paste");

        let stale_key = super::index_key(updated_at - Duration::seconds(60), paste_id.as_str());
        paste_db
            .updated_tree
            .insert(stale_key, paste_id.as_bytes())
            .expect("inject duplicate updated index entry");

        let metas = paste_db.list_meta(10, None).expect("list metadata");
        let duplicate_count = metas.iter().filter(|meta| meta.id == paste_id).count();
        assert_eq!(
            duplicate_count, 1,
            "duplicate updated entries must dedupe by id"
        );
    }

    #[test]
    fn list_meta_keeps_index_fast_path_when_only_in_progress_marker_is_set() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        let paste = Paste::new("body".to_string(), "indexed-only".to_string());
        let paste_id = paste.id.clone();
        paste_db.create(&paste).expect("create paste");

        paste_db
            .meta_state_tree
            .insert(
                META_INDEX_IN_PROGRESS_COUNT_KEY,
                1u64.to_be_bytes().to_vec(),
            )
            .expect("set in-progress marker");

        // If list_meta falls back to canonical rows, this injected corruption would fail decode.
        paste_db
            .tree
            .insert(paste_id.as_bytes(), b"corrupt-canonical-row")
            .expect("corrupt canonical row");

        let metas = paste_db.list_meta(10, None).expect("list metadata");
        assert!(
            metas.into_iter().any(|meta| meta.id == paste_id),
            "in-progress marker alone must not force canonical fallback"
        );
    }

    #[test]
    fn search_meta_falls_back_to_canonical_when_meta_decode_fails() {
        let (paste_db, _dir) = setup_paste_db();
        paste_db
            .reconcile_meta_indexes()
            .expect("initial reconcile writes marker");
        let paste = Paste::new("body".to_string(), "needle-meta".to_string());
        let paste_id = paste.id.clone();
        paste_db.create(&paste).expect("create paste");

        paste_db
            .meta_tree
            .insert(paste_id.as_bytes(), b"corrupt-meta")
            .expect("corrupt metadata row");

        let metas = paste_db
            .search_meta("needle", 10, None, None)
            .expect("search metadata fallback");
        assert!(
            metas.into_iter().any(|meta| meta.id == paste_id),
            "canonical fallback should retain metadata search results"
        );
    }

    #[test]
    fn metadata_top_k_accumulator_stays_bounded_by_limit() {
        let mut ranked: Vec<(
            i32,
            chrono::DateTime<chrono::Utc>,
            crate::models::paste::PasteMeta,
        )> = Vec::new();
        let limit = 5usize;

        for idx in 0..100 {
            let mut paste = Paste::new(format!("body-{idx}"), format!("name-{idx}"));
            paste.updated_at = chrono::Utc::now() + Duration::seconds(idx);
            let meta = crate::models::paste::PasteMeta::from(&paste);
            let score = if idx % 10 == 0 { 10 } else { 1 };
            push_ranked_meta_top_k(&mut ranked, (score, meta.updated_at, meta), limit);
            assert!(
                ranked.len() <= limit,
                "metadata top-k accumulator must stay bounded by limit"
            );
        }

        assert_eq!(ranked.len(), limit);
    }

    #[test]
    fn create_commits_canonical_row_when_index_write_fails() {
        let (paste_db, _dir) = setup_paste_db();
        let paste = Paste::new("content".to_string(), "name".to_string());
        let id = paste.id.clone();

        let result = paste_db.create_inner(&paste, |_db, _paste| {
            Err(AppError::DatabaseError(
                "injected create index failure".to_string(),
            ))
        });
        assert!(result.is_ok(), "canonical create should still succeed");
        assert!(
            paste_db
                .tree
                .get(id.as_bytes())
                .expect("canonical lookup")
                .is_some(),
            "canonical row should remain committed"
        );
        assert!(
            paste_db
                .meta_tree
                .get(id.as_bytes())
                .expect("meta lookup")
                .is_none(),
            "derived metadata should remain stale until reconcile"
        );
        assert!(
            paste_db.meta_index_faulted().expect("faulted marker"),
            "failed index write should set faulted marker"
        );
        assert_eq!(
            paste_db
                .meta_index_in_progress_count()
                .expect("in-progress count"),
            0,
            "failed index write should still clear in-progress marker"
        );
        assert!(
            paste_db
                .needs_reconcile_meta_indexes(false)
                .expect("needs reconcile"),
            "faulted marker should force reconcile"
        );
        let metas = paste_db
            .list_meta(10, None)
            .expect("list metadata via canonical fallback");
        assert!(
            metas.iter().any(|meta| meta.id == id),
            "canonical fallback should keep committed paste visible"
        );
    }

    #[test]
    fn reconcile_clears_faulted_marker_after_index_write_failure() {
        let (paste_db, _dir) = setup_paste_db();
        let paste = Paste::new("content".to_string(), "name".to_string());

        let result = paste_db.create_inner(&paste, |_db, _paste| {
            Err(AppError::DatabaseError(
                "injected create index failure".to_string(),
            ))
        });
        assert!(result.is_ok(), "canonical create should still succeed");
        assert!(
            paste_db.meta_index_faulted().expect("faulted marker"),
            "failure should mark indexes as faulted"
        );

        paste_db
            .reconcile_meta_indexes()
            .expect("reconcile should clear fault marker");
        assert!(
            !paste_db.meta_index_faulted().expect("faulted marker"),
            "reconcile should clear fault marker"
        );
    }

    #[test]
    fn update_commits_canonical_row_when_index_write_fails() {
        let (paste_db, _dir) = setup_paste_db();
        let paste = Paste::new("before".to_string(), "name".to_string());
        let id = paste.id.clone();
        paste_db.create(&paste).expect("create paste");

        let updated = paste_db
            .update_inner(
                id.as_str(),
                UpdatePasteRequest {
                    content: Some("after".to_string()),
                    name: None,
                    language: None,
                    language_is_manual: None,
                    folder_id: None,
                    tags: None,
                },
                |_db, _paste, _previous| {
                    Err(AppError::DatabaseError(
                        "injected update index failure".to_string(),
                    ))
                },
            )
            .expect("update should still report success despite derived index failure")
            .expect("paste should exist");
        assert_eq!(updated.content, "after");

        let canonical = paste_db
            .get(id.as_str())
            .expect("get paste")
            .expect("paste should remain");
        assert_eq!(canonical.content, "after");
        assert!(
            paste_db.meta_index_faulted().expect("faulted marker"),
            "failed index write should set faulted marker"
        );
        assert_eq!(
            paste_db
                .meta_index_in_progress_count()
                .expect("in-progress count"),
            0
        );
    }

    #[test]
    fn update_if_folder_matches_commits_when_index_write_fails() {
        let (paste_db, _dir) = setup_paste_db();
        let paste = Paste::new("before".to_string(), "name".to_string());
        let id = paste.id.clone();
        paste_db.create(&paste).expect("create paste");

        let moved = paste_db
            .update_if_folder_matches_inner(
                id.as_str(),
                None,
                UpdatePasteRequest {
                    content: None,
                    name: Some("moved".to_string()),
                    language: None,
                    language_is_manual: None,
                    folder_id: Some("folder-x".to_string()),
                    tags: None,
                },
                |_db, _paste, _previous| {
                    Err(AppError::DatabaseError(
                        "injected cas index failure".to_string(),
                    ))
                },
            )
            .expect("cas update should still report success")
            .expect("paste should exist");
        assert_eq!(moved.name, "moved");
        assert_eq!(moved.folder_id.as_deref(), Some("folder-x"));

        let canonical = paste_db
            .get(id.as_str())
            .expect("get paste")
            .expect("paste should remain");
        assert_eq!(canonical.folder_id.as_deref(), Some("folder-x"));
        assert!(
            paste_db.meta_index_faulted().expect("faulted marker"),
            "failed derived index write should set faulted marker"
        );
        assert_eq!(
            paste_db
                .meta_index_in_progress_count()
                .expect("in-progress count"),
            0
        );
    }

    #[test]
    fn delete_commits_canonical_removal_when_index_delete_fails() {
        let (paste_db, _dir) = setup_paste_db();
        let paste = Paste::new("body".to_string(), "name".to_string());
        let id = paste.id.clone();
        paste_db.create(&paste).expect("create paste");

        let deleted = paste_db
            .delete_and_return_inner(id.as_str(), |_db, _meta| {
                Err(AppError::DatabaseError(
                    "injected delete index failure".to_string(),
                ))
            })
            .expect("delete should still report success")
            .expect("paste should be returned");
        assert_eq!(deleted.id, id);
        assert!(
            paste_db.get(id.as_str()).expect("lookup").is_none(),
            "canonical row should remain deleted"
        );
        assert!(
            paste_db.meta_index_faulted().expect("faulted marker"),
            "failed index delete should set faulted marker"
        );
        assert_eq!(
            paste_db
                .meta_index_in_progress_count()
                .expect("in-progress count"),
            0
        );
    }

    #[test]
    fn update_error_does_not_leave_meta_index_state_marked() {
        let (paste_db, _dir) = setup_paste_db();
        let paste = Paste::new("body".to_string(), "name".to_string());
        let paste_id = paste.id.clone();
        paste_db.create(&paste).expect("create paste");

        paste_db
            .tree
            .insert(paste_id.as_bytes(), b"corrupt-paste-row")
            .expect("corrupt canonical row");

        let update = UpdatePasteRequest {
            content: Some("updated".to_string()),
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: None,
        };
        let err = paste_db
            .update(paste_id.as_str(), update)
            .expect_err("corrupt row should fail update");
        assert!(
            matches!(err, AppError::Serialization(_)),
            "expected serialization error for corrupt canonical row"
        );
        assert_eq!(
            paste_db
                .meta_index_in_progress_count()
                .expect("in-progress count"),
            0,
            "update errors without index mutation should not leave in-progress marker set"
        );
        assert!(
            !paste_db.meta_index_faulted().expect("faulted marker"),
            "update errors without index mutation should not mark indexes as faulted"
        );
    }

    #[test]
    fn update_if_folder_mismatch_error_does_not_leave_meta_index_state_marked() {
        let (paste_db, _dir) = setup_paste_db();
        let paste = Paste::new("body".to_string(), "name".to_string());
        let paste_id = paste.id.clone();
        paste_db.create(&paste).expect("create paste");

        paste_db
            .tree
            .insert(paste_id.as_bytes(), b"corrupt-paste-row")
            .expect("corrupt canonical row");

        let update = UpdatePasteRequest {
            content: Some("updated".to_string()),
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: None,
        };
        let err = paste_db
            .update_if_folder_matches(paste_id.as_str(), None, update)
            .expect_err("corrupt row should fail update");
        assert!(
            matches!(err, AppError::Serialization(_)),
            "expected serialization error for corrupt canonical row"
        );
        assert_eq!(
            paste_db
                .meta_index_in_progress_count()
                .expect("in-progress count"),
            0,
            "failed CAS update without index mutation should not leave in-progress marker set"
        );
        assert!(
            !paste_db.meta_index_faulted().expect("faulted marker"),
            "failed CAS update without index mutation should not mark indexes as faulted"
        );
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
        let in_progress = paste_db
            .meta_state_tree
            .get(META_INDEX_IN_PROGRESS_COUNT_KEY)
            .expect("in-progress lookup")
            .expect("in-progress marker");
        assert_eq!(
            u64::from_be_bytes(in_progress.as_ref().try_into().expect("in-progress bytes")),
            0
        );
        let faulted = paste_db
            .meta_state_tree
            .get(META_INDEX_FAULTED_KEY)
            .expect("faulted lookup")
            .expect("faulted marker");
        assert_eq!(faulted.as_ref(), &[0u8]);
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
