//! eframe storage adapter for lightweight GUI state.

use super::{
    LocalPasteApp, SidebarCollection, STORAGE_ACTIVE_COLLECTION_KEY, STORAGE_ACTIVE_LANGUAGE_KEY,
    STORAGE_SELECTED_ID_KEY,
};

fn non_empty_storage_value(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_storage_language(value: Option<String>) -> Option<String> {
    non_empty_storage_value(value)
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
            self.active_collection = collection;
        }
        self.active_language_filter =
            normalize_storage_language(storage.get_string(STORAGE_ACTIVE_LANGUAGE_KEY));
        self.pending_selection_id =
            non_empty_storage_value(storage.get_string(STORAGE_SELECTED_ID_KEY));
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
    use super::{non_empty_storage_value, normalize_storage_language};

    #[test]
    fn storage_values_trim_and_drop_blanks() {
        assert_eq!(
            non_empty_storage_value(Some(" abc ".to_string())),
            Some("abc".to_string())
        );
        assert_eq!(non_empty_storage_value(Some("   ".to_string())), None);
        assert_eq!(non_empty_storage_value(None), None);
    }

    #[test]
    fn storage_language_is_canonicalized() {
        assert_eq!(
            normalize_storage_language(Some(" Rust ".to_string())),
            Some("rust".to_string())
        );
        assert_eq!(normalize_storage_language(Some("   ".to_string())), None);
    }
}
