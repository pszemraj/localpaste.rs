//! Paste storage operations backed by sled.

mod helpers;

use crate::{error::AppError, models::paste::*};
use chrono::{DateTime, Utc};
use sled::Db;
use std::collections::HashSet;
use std::sync::Arc;

use self::helpers::{
    apply_update_request, decode_dirty_count, deserialize_meta, deserialize_paste,
    finalize_meta_search_results, finalize_recent_meta_results, folder_matches_expected, index_key,
    language_matches_filter, meta_matches_filters, push_ranked_meta_top_k, push_recent_meta_top_k,
    score_meta_match, score_paste_match,
};

#[cfg(test)]
pub(crate) use self::helpers::set_reconcile_failpoint;

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

struct MetaIndexMutationGuard<'a> {
    db: &'a PasteDb,
    active: bool,
}

impl<'a> MetaIndexMutationGuard<'a> {
    fn begin(db: &'a PasteDb) -> Result<Self, AppError> {
        db.begin_meta_index_mutation()?;
        Ok(Self { db, active: true })
    }

    fn finish(&mut self) {
        if !self.active {
            return;
        }
        self.db.try_end_meta_index_mutation();
        self.active = false;
    }

    fn finish_with_derived_index_write(
        &mut self,
        operation: &str,
        paste_id: &str,
        index_result: Result<(), AppError>,
    ) {
        self.db
            .finalize_derived_index_write(operation, paste_id, index_result);
        self.active = false;
    }
}

impl Drop for MetaIndexMutationGuard<'_> {
    fn drop(&mut self) {
        self.finish();
    }
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
        let mut mutation_guard = MetaIndexMutationGuard::begin(self)?;
        let key = paste.id.as_bytes();
        let value = bincode::serialize(paste)?;
        let inserted = self
            .tree
            .compare_and_swap(key, None as Option<&[u8]>, Some(value))?;
        if inserted.is_err() {
            return Err(AppError::StorageMessage(format!(
                "Paste id '{}' already exists",
                paste.id
            )));
        }
        let index_result = index_writer(self, paste);
        mutation_guard.finish_with_derived_index_write("create", paste.id.as_str(), index_result);
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
        let mut mutation_guard = MetaIndexMutationGuard::begin(self)?;
        // `update_and_fetch` retries the closure under contention but cannot return
        // a typed error. Capture parse/encode failures and surface them after the call.
        let mut update_error: Option<AppError> = None;
        let mut old_meta: Option<PasteMeta> = None;
        let result = self.tree.update_and_fetch(id.as_bytes(), |old| {
            let bytes = old?;

            let mut paste = match deserialize_paste(bytes) {
                Ok(paste) => paste,
                Err(err) => {
                    update_error = Some(AppError::Serialization(err));
                    return Some(bytes.to_vec());
                }
            };

            old_meta = Some(PasteMeta::from(&paste));
            apply_update_request(&mut paste, &update);
            match bincode::serialize(&paste) {
                Ok(encoded) => Some(encoded),
                Err(err) => {
                    update_error = Some(AppError::Serialization(err));
                    Some(bytes.to_vec())
                }
            }
        })?;
        if let Some(err) = update_error.take() {
            return Err(err);
        }

        match result {
            Some(bytes) => {
                let paste = deserialize_paste(&bytes)?;
                let previous = old_meta.take();
                let index_result = index_writer(self, &paste, previous);
                mutation_guard.finish_with_derived_index_write(
                    "update",
                    paste.id.as_str(),
                    index_result,
                );
                Ok(Some(paste))
            }
            None => {
                mutation_guard.finish();
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
        let mut mutation_guard = MetaIndexMutationGuard::begin(self)?;
        // `update_and_fetch` retries the closure under contention but cannot return
        // a typed error. Capture parse/encode failures and surface them after the call.
        let mut update_error: Option<AppError> = None;
        let mut old_meta: Option<PasteMeta> = None;
        let mut folder_mismatch = false;
        let result = self.tree.update_and_fetch(id.as_bytes(), |old| {
            let bytes = old?;

            let mut paste = match deserialize_paste(bytes) {
                Ok(paste) => paste,
                Err(err) => {
                    update_error = Some(AppError::Serialization(err));
                    return Some(bytes.to_vec());
                }
            };

            if !folder_matches_expected(
                paste.folder_id.as_deref(),
                expected_folder_id,
                &mut folder_mismatch,
            ) {
                return Some(bytes.to_vec());
            }

            old_meta = Some(PasteMeta::from(&paste));
            apply_update_request(&mut paste, &update);
            match bincode::serialize(&paste) {
                Ok(encoded) => Some(encoded),
                Err(err) => {
                    update_error = Some(AppError::Serialization(err));
                    Some(bytes.to_vec())
                }
            }
        })?;
        if let Some(err) = update_error.take() {
            return Err(err);
        }
        if folder_mismatch {
            mutation_guard.finish();
            return Ok(None);
        }

        match result {
            Some(bytes) => {
                let paste = deserialize_paste(&bytes)?;
                let previous = old_meta.take();
                let index_result = index_writer(self, &paste, previous);
                mutation_guard.finish_with_derived_index_write(
                    "update_if_folder_matches",
                    paste.id.as_str(),
                    index_result,
                );
                Ok(Some(paste))
            }
            None => {
                mutation_guard.finish();
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
        let mut mutation_guard = MetaIndexMutationGuard::begin(self)?;
        let mut delete_error: Option<AppError> = None;
        let mut deleted_paste: Option<Paste> = None;
        let _ = self.tree.update_and_fetch(id.as_bytes(), |old| {
            delete_error = None;
            deleted_paste = None;
            let bytes = old?;
            match deserialize_paste(bytes) {
                Ok(paste) => {
                    deleted_paste = Some(paste);
                    None
                }
                Err(err) => {
                    delete_error = Some(AppError::Serialization(err));
                    Some(bytes.to_vec())
                }
            }
        })?;
        if let Some(err) = delete_error.take() {
            return Err(err);
        }

        match deleted_paste {
            Some(paste) => {
                let meta = PasteMeta::from(&paste);
                let index_result = index_remover(self, &meta);
                mutation_guard.finish_with_derived_index_write(
                    "delete",
                    paste.id.as_str(),
                    index_result,
                );
                Ok(Some(paste))
            }
            None => {
                mutation_guard.finish();
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

    /// List canonical paste ids in a bounded batch.
    ///
    /// This helper avoids materializing a large `Vec<Paste>` when callers only need ids.
    ///
    /// # Arguments
    /// - `limit`: Maximum number of ids to return.
    /// - `folder_id`: Optional folder filter.
    ///
    /// # Returns
    /// Up to `limit` canonical paste ids.
    ///
    /// # Errors
    /// Returns an error if iteration or deserialization fails.
    pub fn list_canonical_ids_batch(
        &self,
        limit: usize,
        folder_id: Option<&str>,
    ) -> Result<Vec<String>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut ids = Vec::with_capacity(limit);
        for item in self.tree.iter() {
            let (_, value) = item?;
            let paste = deserialize_paste(&value)?;
            if let Some(fid) = folder_id {
                if paste.folder_id.as_deref() != Some(fid) {
                    continue;
                }
            }
            ids.push(paste.id);
            if ids.len() >= limit {
                break;
            }
        }
        Ok(ids)
    }

    /// Scan canonical rows and stream metadata to a callback.
    ///
    /// This helper bounds memory usage to one deserialized canonical row at a time.
    ///
    /// # Arguments
    /// - `on_meta`: Called for each canonical row's [`PasteMeta`].
    ///
    /// # Returns
    /// `Ok(())` after all canonical rows are scanned.
    ///
    /// # Errors
    /// Returns iteration/deserialization errors or callback errors.
    pub fn scan_canonical_meta<F>(&self, mut on_meta: F) -> Result<(), AppError>
    where
        F: FnMut(PasteMeta) -> Result<(), AppError>,
    {
        for item in self.tree.iter() {
            let (_, value) = item?;
            let paste = deserialize_paste(&value)?;
            on_meta(PasteMeta::from(&paste))?;
        }
        Ok(())
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
        let language_filter = normalize_language_filter(language.as_deref());

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
        let language_filter = normalize_language_filter(language.as_deref());
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

        let rebuild_result = (|| -> Result<(), AppError> {
            #[cfg(test)]
            helpers::maybe_inject_reconcile_failpoint()?;
            self.meta_tree.clear()?;
            self.updated_tree.clear()?;
            for item in self.tree.iter() {
                let (_, value) = item?;
                let paste = deserialize_paste(&value)?;
                self.upsert_meta_and_index_from_paste(&paste, None)?;
            }
            self.meta_tree.flush()?;
            self.updated_tree.flush()?;
            Ok(())
        })();

        if let Err(err) = rebuild_result {
            // Reconcile failed mid-flight: preserve degraded fallback by marking faulted
            // and clear in-progress best-effort so startup/runtime does not get stuck.
            self.mark_meta_index_faulted();
            self.try_end_meta_index_mutation();
            return Err(err);
        }

        if let Err(err) = self.write_meta_index_state(META_INDEX_SCHEMA_VERSION, 0, false) {
            // State write failed after rebuild. Keep canonical reads safe and mark
            // indexes faulted so callers continue through bounded canonical fallback.
            self.mark_meta_index_faulted();
            self.try_end_meta_index_mutation();
            return Err(err);
        }

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

    fn update_meta_index_in_progress(&self, increment: bool) -> Result<(), AppError> {
        let _ = self
            .meta_state_tree
            .update_and_fetch(META_INDEX_IN_PROGRESS_COUNT_KEY, |old| {
                let current = decode_dirty_count(old);
                let next = if increment {
                    current.saturating_add(1)
                } else {
                    current.saturating_sub(1)
                };
                Some(next.to_be_bytes().to_vec())
            })?;
        Ok(())
    }

    fn begin_meta_index_mutation(&self) -> Result<(), AppError> {
        self.update_meta_index_in_progress(true)
    }

    fn end_meta_index_mutation(&self) -> Result<(), AppError> {
        self.update_meta_index_in_progress(false)
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
        let folder_filter = folder_id.as_deref();
        self.scan_canonical_meta(|meta| {
            if let Some(fid) = folder_filter {
                if meta.folder_id.as_deref() != Some(fid) {
                    return Ok(());
                }
            }
            push_recent_meta_top_k(&mut ranked, (meta.updated_at, meta), limit);
            Ok(())
        })?;
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
        let language_filter = normalize_language_filter(language.as_deref());
        let mut results: Vec<(i32, DateTime<Utc>, PasteMeta)> = Vec::new();
        let folder_filter = folder_id.as_deref();
        self.scan_canonical_meta(|meta| {
            if !meta_matches_filters(&meta, folder_filter, language_filter.as_deref()) {
                return Ok(());
            }
            let score = score_meta_match(&meta, &query_lower);
            if score > 0 {
                push_ranked_meta_top_k(&mut results, (score, meta.updated_at, meta), limit);
            }
            Ok(())
        })?;
        Ok(finalize_meta_search_results(results, limit))
    }
}

#[cfg(test)]
mod tests;
