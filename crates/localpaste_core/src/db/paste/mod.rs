//! Paste storage operations backed by redb.

mod helpers;

use crate::{db::tables::*, error::AppError, models::paste::*};
use chrono::{DateTime, Utc};
use redb::{ReadableDatabase, ReadableTable};
use std::sync::Arc;

use self::helpers::{
    deserialize_meta, finalize_meta_search_results, folder_matches_expected,
    language_matches_filter, meta_matches_filters, push_ranked_meta_top_k, score_meta_match,
    score_paste_match,
};

pub(crate) use self::helpers::{apply_update_request, deserialize_paste, reverse_timestamp_key};

/// Accessor for paste-related redb tables.
pub struct PasteDb {
    db: Arc<redb::Database>,
}

impl PasteDb {
    fn reject_direct_create_with_folder(paste: &Paste) -> Result<(), AppError> {
        if paste.folder_id.is_some() {
            return Err(AppError::BadRequest(
                "Direct folder assignment via PasteDb::create is not allowed; use TransactionOps::create_paste_with_folder".to_string(),
            ));
        }
        Ok(())
    }

    fn reject_direct_update_with_folder_change(
        update: &UpdatePasteRequest,
    ) -> Result<(), AppError> {
        if update.folder_id.is_some() {
            return Err(AppError::BadRequest(
                "Direct folder updates via PasteDb::update are not allowed; use TransactionOps::move_paste_between_folders".to_string(),
            ));
        }
        Ok(())
    }

    fn reject_direct_delete_for_foldered_paste(paste: &Paste) -> Result<(), AppError> {
        if paste.folder_id.is_some() {
            return Err(AppError::BadRequest(
                "Direct deletion of foldered pastes via PasteDb::delete is not allowed; use TransactionOps::delete_paste_with_folder".to_string(),
            ));
        }
        Ok(())
    }

    /// Initialize paste tables if they do not exist yet.
    ///
    /// # Returns
    /// A new [`PasteDb`] accessor bound to `db`.
    ///
    /// # Errors
    /// Returns an error when redb transaction/table initialization fails.
    pub fn new(db: Arc<redb::Database>) -> Result<Self, AppError> {
        let write_txn = db.begin_write()?;
        write_txn.open_table(PASTES)?;
        write_txn.open_table(PASTES_META)?;
        write_txn.open_table(PASTES_BY_UPDATED)?;
        write_txn.commit()?;
        Ok(Self { db })
    }

    /// Insert a new paste row and derived metadata/index rows atomically.
    ///
    /// This API only supports unfiled inserts. Use
    /// [`crate::db::TransactionOps::create_paste_with_folder`] for foldered pastes.
    ///
    /// # Arguments
    /// - `paste`: Paste row to persist.
    ///
    /// # Returns
    /// `Ok(())` when insert commits.
    ///
    /// # Errors
    /// Returns an error when serialization fails, id already exists, or storage
    /// operations fail.
    pub fn create(&self, paste: &Paste) -> Result<(), AppError> {
        Self::reject_direct_create_with_folder(paste)?;
        let encoded_paste = bincode::serialize(paste)?;
        let meta = PasteMeta::from(paste);
        let encoded_meta = bincode::serialize(&meta)?;
        let recency_key = reverse_timestamp_key(paste.updated_at);

        let write_txn = self.db.begin_write()?;
        {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;

            if pastes.get(paste.id.as_str())?.is_some() {
                return Err(AppError::StorageMessage(format!(
                    "Paste id '{}' already exists",
                    paste.id
                )));
            }

            pastes.insert(paste.id.as_str(), encoded_paste.as_slice())?;
            metas.insert(paste.id.as_str(), encoded_meta.as_slice())?;
            updated.insert((recency_key, paste.id.as_str()), ())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Fetch a paste by id.
    ///
    /// # Returns
    /// `Ok(Some(paste))` when found, `Ok(None)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn get(&self, id: &str) -> Result<Option<Paste>, AppError> {
        let read_txn = self.db.begin_read()?;
        let pastes = read_txn.open_table(PASTES)?;
        match pastes.get(id)? {
            Some(value) => Ok(Some(deserialize_paste(value.value())?)),
            None => Ok(None),
        }
    }

    /// Update a paste by id.
    ///
    /// This API only supports non-folder metadata/content updates. Use
    /// [`crate::db::TransactionOps::move_paste_between_folders`] for folder moves.
    ///
    /// # Arguments
    /// - `id`: Paste id to update.
    /// - `update`: Update payload.
    ///
    /// # Returns
    /// `Ok(Some(paste))` when updated, `Ok(None)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access or serialization fails.
    pub fn update(&self, id: &str, update: UpdatePasteRequest) -> Result<Option<Paste>, AppError> {
        self.update_inner(id, None, update)
    }

    /// Update a paste only when current folder id matches `expected_folder_id`.
    ///
    /// # Arguments
    /// - `id`: Paste id to update.
    /// - `expected_folder_id`: Expected current folder id.
    /// - `update`: Update payload.
    ///
    /// # Returns
    /// `Ok(Some(paste))` when updated, `Ok(None)` when missing or folder does not match.
    ///
    /// # Errors
    /// Returns an error when storage access or serialization fails.
    pub fn update_if_folder_matches(
        &self,
        id: &str,
        expected_folder_id: Option<&str>,
        update: UpdatePasteRequest,
    ) -> Result<Option<Paste>, AppError> {
        self.update_inner(id, Some(expected_folder_id), update)
    }

    fn update_inner(
        &self,
        id: &str,
        expected_folder: Option<Option<&str>>,
        update: UpdatePasteRequest,
    ) -> Result<Option<Paste>, AppError> {
        Self::reject_direct_update_with_folder_change(&update)?;
        let write_txn = self.db.begin_write()?;
        let updated_paste = {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;

            let Some(old_guard) = pastes.get(id)? else {
                return Ok(None);
            };
            let mut paste = deserialize_paste(old_guard.value())?;
            let old_folder = paste.folder_id.clone();
            let old_recency_key = reverse_timestamp_key(paste.updated_at);
            drop(old_guard);

            if let Some(expected) = expected_folder {
                if !folder_matches_expected(old_folder.as_deref(), expected) {
                    return Ok(None);
                }
            }

            apply_update_request(&mut paste, &update);

            let encoded_paste = bincode::serialize(&paste)?;
            let meta = PasteMeta::from(&paste);
            let encoded_meta = bincode::serialize(&meta)?;
            let new_recency_key = reverse_timestamp_key(paste.updated_at);

            pastes.insert(id, encoded_paste.as_slice())?;
            metas.insert(id, encoded_meta.as_slice())?;
            if old_recency_key != new_recency_key {
                let _ = updated.remove((old_recency_key, id))?;
            }
            updated.insert((new_recency_key, id), ())?;

            Some(paste)
        };

        write_txn.commit()?;
        Ok(updated_paste)
    }

    /// Delete a paste and return the deleted canonical row.
    ///
    /// This API only supports unfiled deletes. Use
    /// [`crate::db::TransactionOps::delete_paste_with_folder`] for foldered rows.
    ///
    /// # Returns
    /// `Ok(Some(paste))` when deleted, `Ok(None)` when missing.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn delete_and_return(&self, id: &str) -> Result<Option<Paste>, AppError> {
        let write_txn = self.db.begin_write()?;
        let deleted = {
            let mut pastes = write_txn.open_table(PASTES)?;
            let mut metas = write_txn.open_table(PASTES_META)?;
            let mut updated = write_txn.open_table(PASTES_BY_UPDATED)?;

            let Some(old_guard) = pastes.get(id)? else {
                return Ok(None);
            };
            let paste = deserialize_paste(old_guard.value())?;
            Self::reject_direct_delete_for_foldered_paste(&paste)?;
            let recency_key = reverse_timestamp_key(paste.updated_at);
            drop(old_guard);

            let _ = updated.remove((recency_key, id))?;
            let _ = pastes.remove(id)?;
            let _ = metas.remove(id)?;
            Some(paste)
        };

        write_txn.commit()?;
        Ok(deleted)
    }

    /// Delete a paste by id.
    ///
    /// # Returns
    /// `true` when a row was deleted, otherwise `false`.
    ///
    /// # Errors
    /// Returns an error when storage or deserialization fails.
    pub fn delete(&self, id: &str) -> Result<bool, AppError> {
        Ok(self.delete_and_return(id)?.is_some())
    }

    /// List canonical paste rows sorted by `updated_at` descending.
    ///
    /// # Arguments
    /// - `limit`: Maximum rows to return.
    /// - `folder_id`: Optional folder filter.
    ///
    /// # Returns
    /// Up to `limit` canonical rows in descending recency order.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn list(&self, limit: usize, folder_id: Option<String>) -> Result<Vec<Paste>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let read_txn = self.db.begin_read()?;
        let updated_table = read_txn.open_table(PASTES_BY_UPDATED)?;
        let pastes_table = read_txn.open_table(PASTES)?;
        let mut pastes = Vec::new();

        for item in updated_table.iter()? {
            let (key, _) = item?;
            let (_, paste_id) = key.value();
            let Some(paste_guard) = pastes_table.get(paste_id)? else {
                continue;
            };
            let paste = deserialize_paste(paste_guard.value())?;
            if let Some(ref fid) = folder_id {
                if paste.folder_id.as_ref() != Some(fid) {
                    continue;
                }
            }
            pastes.push(paste);
            if pastes.len() >= limit {
                break;
            }
        }

        Ok(pastes)
    }

    /// Return up to `limit` canonical paste ids, optionally filtered by folder.
    ///
    /// # Arguments
    /// - `limit`: Maximum ids to return.
    /// - `folder_id`: Optional folder filter.
    ///
    /// # Returns
    /// Up to `limit` canonical paste ids.
    ///
    /// Order is based on canonical key iteration and is intentionally not a recency
    /// sort. This helper is used by destructive maintenance paths that must walk
    /// canonical rows directly even if derived metadata/index tables drift.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn list_canonical_ids_batch(
        &self,
        limit: usize,
        folder_id: Option<&str>,
    ) -> Result<Vec<String>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let read_txn = self.db.begin_read()?;
        let pastes_table = read_txn.open_table(PASTES)?;
        let mut ids = Vec::with_capacity(limit);

        for item in pastes_table.iter()? {
            let (_, value) = item?;
            let paste = deserialize_paste(value.value())?;
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

    /// Scan canonical paste rows and invoke `on_meta` for each derived [`PasteMeta`].
    ///
    /// # Returns
    /// `Ok(())` when scan completes.
    ///
    /// # Errors
    /// Returns an error when storage access, deserialization, or callback execution fails.
    pub fn scan_canonical_meta<F>(&self, mut on_meta: F) -> Result<(), AppError>
    where
        F: FnMut(PasteMeta) -> Result<(), AppError>,
    {
        let read_txn = self.db.begin_read()?;
        let pastes_table = read_txn.open_table(PASTES)?;
        for item in pastes_table.iter()? {
            let (_, value) = item?;
            let paste = deserialize_paste(value.value())?;
            on_meta(PasteMeta::from(&paste))?;
        }
        Ok(())
    }

    /// List paste metadata using the recency index.
    ///
    /// # Arguments
    /// - `limit`: Maximum rows to return.
    /// - `folder_id`: Optional folder filter.
    ///
    /// # Returns
    /// Up to `limit` metadata rows in index order.
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
    pub fn list_meta(
        &self,
        limit: usize,
        folder_id: Option<String>,
    ) -> Result<Vec<PasteMeta>, AppError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let read_txn = self.db.begin_read()?;
        let updated_table = read_txn.open_table(PASTES_BY_UPDATED)?;
        let meta_table = read_txn.open_table(PASTES_META)?;

        let mut metas = Vec::with_capacity(limit);
        for item in updated_table.iter()? {
            let (key, _) = item?;
            let (_, paste_id) = key.value();
            let Some(meta_guard) = meta_table.get(paste_id)? else {
                continue;
            };
            let meta = deserialize_meta(meta_guard.value())?;
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

    /// Search canonical paste data and return ranked metadata rows.
    ///
    /// # Arguments
    /// - `query`: Search query string.
    /// - `limit`: Maximum rows to return.
    /// - `folder_id`: Optional folder filter.
    /// - `language`: Optional language filter.
    ///
    /// # Returns
    /// Ranked metadata matches (name/tags/content scoring).
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
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
        let language_filter = normalize_language_filter(language.as_deref());
        let read_txn = self.db.begin_read()?;
        let pastes_table = read_txn.open_table(PASTES)?;
        let mut results: Vec<(i32, DateTime<Utc>, PasteMeta)> = Vec::new();

        for item in pastes_table.iter()? {
            let (_, value) = item?;
            let paste = deserialize_paste(value.value())?;

            if let Some(ref fid) = folder_id {
                if paste.folder_id.as_ref() != Some(fid) {
                    continue;
                }
            }
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

    /// Search metadata-only fields and return ranked rows.
    ///
    /// # Arguments
    /// - `query`: Search query string.
    /// - `limit`: Maximum rows to return.
    /// - `folder_id`: Optional folder filter.
    /// - `language`: Optional language filter.
    ///
    /// # Returns
    /// Ranked metadata matches (name/tags/language scoring).
    ///
    /// # Errors
    /// Returns an error when storage access or deserialization fails.
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

        let query_lower = query.to_lowercase();
        let language_filter = normalize_language_filter(language.as_deref());
        let read_txn = self.db.begin_read()?;
        let meta_table = read_txn.open_table(PASTES_META)?;
        let mut results: Vec<(i32, DateTime<Utc>, PasteMeta)> = Vec::new();

        for item in meta_table.iter()? {
            let (_, value) = item?;
            let meta = deserialize_meta(value.value())?;
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

#[cfg(test)]
mod tests;
