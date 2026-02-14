//! Internal metadata/index state helpers for [`PasteDb`].

use super::*;

impl PasteDb {
    pub(super) fn meta_index_schema_version(&self) -> Result<Option<u32>, AppError> {
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

    pub(super) fn meta_index_in_progress_count(&self) -> Result<u64, AppError> {
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

    pub(super) fn meta_index_faulted(&self) -> Result<bool, AppError> {
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

    pub(super) fn write_meta_index_state(
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
                let current = super::helpers::decode_dirty_count(old);
                let next = if increment {
                    current.saturating_add(1)
                } else {
                    current.saturating_sub(1)
                };
                Some(next.to_be_bytes().to_vec())
            })?;
        Ok(())
    }

    pub(super) fn begin_meta_index_mutation(&self) -> Result<(), AppError> {
        self.update_meta_index_in_progress(true)
    }

    fn end_meta_index_mutation(&self) -> Result<(), AppError> {
        self.update_meta_index_in_progress(false)
    }

    pub(super) fn mark_meta_index_faulted(&self) {
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

    pub(super) fn try_end_meta_index_mutation(&self) {
        if let Err(err) = self.end_meta_index_mutation() {
            tracing::warn!(
                "Failed to clear metadata index in-progress marker after mutation: {}",
                err
            );
        }
    }

    pub(super) fn finalize_derived_index_write(
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

    pub(super) fn upsert_meta_and_index_from_paste(
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
        let recency_key = super::helpers::index_key(meta.updated_at, meta.id.as_str());
        self.updated_tree
            .insert(recency_key.clone(), meta.id.as_bytes())?;
        if let Some(previous) = previous {
            let previous_key = super::helpers::index_key(previous.updated_at, previous.id.as_str());
            if previous_key != recency_key {
                self.updated_tree.remove(previous_key)?;
            }
        }
        Ok(())
    }

    pub(super) fn remove_meta_and_index(&self, meta: &PasteMeta) -> Result<(), AppError> {
        self.meta_tree.remove(meta.id.as_bytes())?;
        self.remove_index_entry(meta)?;
        Ok(())
    }

    fn remove_index_entry(&self, meta: &PasteMeta) -> Result<(), AppError> {
        let recency_key = super::helpers::index_key(meta.updated_at, meta.id.as_str());
        self.updated_tree.remove(recency_key)?;
        Ok(())
    }
}
