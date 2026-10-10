//! eframe storage adapter for lightweight GUI state.

use super::{
    LocalPasteApp, SidebarCollection, STORAGE_ACTIVE_COLLECTION_KEY, STORAGE_ACTIVE_LANGUAGE_KEY,
    STORAGE_SELECTED_ID_KEY,
};
use localpaste_core::text::normalize_optional_nonempty;

fn normalize_storage_language(value: Option<String>) -> Option<String> {
    normalize_optional_nonempty(value)
        .map(|value| localpaste_core::detection::canonical::canonicalize(value.as_str()))
}

impl LocalPasteApp {
    /// Restore lightweight GUI state from eframe persistence.
    ///
    /// Paste loading still flows through the normal sidebar/list path: the
    /// restored paste id is queued as a pending selection and resolved after
    /// the first visible list refresh.
    pub(crate) fn restore_gui_storage(&mut self, storage: Option<&dyn eframe::Storage>) {
        let Some(storage) = storage else {
            return;
        };

        if let Some(collection) = storage
            .get_string(STORAGE_ACTIVE_COLLECTION_KEY)
            .as_deref()
            .and_then(SidebarCollection::from_storage_value)
        {
            // Unknown collection tags are ignored so older/newer persisted UI state
            // falls back to the constructor default instead of an invalid value.
            self.active_collection = collection;
        }
        self.active_language_filter =
            normalize_storage_language(storage.get_string(STORAGE_ACTIVE_LANGUAGE_KEY));
        self.pending_selection_id =
            normalize_optional_nonempty(storage.get_string(STORAGE_SELECTED_ID_KEY));
        // A remembered paste may be gone (another database, or deleted while
        // closed); that restore clears without reporting a deletion.
        self.restored_selection_id = self.pending_selection_id.clone();
    }

    /// Consumes the session-restore marker when `id` is the remembered selection.
    ///
    /// # Returns
    /// `true` when `id` came from session restore, so its absence is not news.
    pub(super) fn take_restored_selection(&mut self, id: &str) -> bool {
        let restored = self.restored_selection_id.as_deref() == Some(id);
        if restored {
            self.restored_selection_id = None;
        }
        restored
    }

    /// Persist lightweight GUI state through eframe storage.
    pub(crate) fn save_gui_storage(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string(
            STORAGE_SELECTED_ID_KEY,
            self.selected_id.clone().unwrap_or_default(),
        );
        storage.set_string(
            STORAGE_ACTIVE_COLLECTION_KEY,
            self.active_collection.storage_value().to_string(),
        );
        storage.set_string(
            STORAGE_ACTIVE_LANGUAGE_KEY,
            self.active_language_filter.clone().unwrap_or_default(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_storage_language;

    #[test]
    fn storage_language_is_canonicalized() {
        assert_eq!(
            normalize_storage_language(Some(" Rust ".to_string())),
            Some("rust".to_string())
        );
        assert_eq!(normalize_storage_language(Some("   ".to_string())), None);
    }
}
