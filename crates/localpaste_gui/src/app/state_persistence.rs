//! eframe storage adapter for lightweight GUI state.

use super::{
    LocalPasteApp, SidebarCollection, STORAGE_ACTIVE_COLLECTION_KEY, STORAGE_ACTIVE_LANGUAGE_KEY,
    STORAGE_SELECTED_ID_KEY,
};
use localpaste_core::config::is_default_db_path;
use localpaste_core::text::normalize_optional_nonempty;
use std::path::{Path, PathBuf};

/// File that holds the GUI state of a non-default database, inside its `DB_PATH` directory.
const GUI_STATE_FILE_NAME: &str = "gui-state.ron";

fn normalize_storage_language(value: Option<String>) -> Option<String> {
    normalize_optional_nonempty(value)
        .map(|value| localpaste_core::detection::canonical::canonicalize(value.as_str()))
}

/// Chooses the eframe state file for the database at `db_path`.
///
/// GUI state includes the remembered paste id, which only exists in the
/// database that saved it, so every database keeps its own state. The default
/// store keeps eframe's per-user location, so its existing state carries over;
/// any other store keeps the state in its own directory, beside `.api-addr`.
///
/// # Arguments
/// - `db_path`: Database directory, `~`-expanded as `Config` resolves it.
///
/// # Returns
/// `None` for the default store, which leaves eframe on its standard location;
/// otherwise the absolute path of [`GUI_STATE_FILE_NAME`] inside `db_path`.
fn gui_state_path_for_db_path(db_path: &str) -> Option<PathBuf> {
    if is_default_db_path(db_path) {
        return None;
    }
    let db_dir = Path::new(db_path);
    let db_dir = std::path::absolute(db_dir).unwrap_or_else(|_| db_dir.to_path_buf());
    Some(db_dir.join(GUI_STATE_FILE_NAME))
}

impl LocalPasteApp {
    /// Returns the eframe state file for the database this app opened.
    ///
    /// # Returns
    /// The location chosen by [`gui_state_path_for_db_path`].
    pub(crate) fn gui_state_path(&self) -> Option<PathBuf> {
        gui_state_path_for_db_path(&self.db_path)
    }

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
        // A remembered paste may be gone (deleted through the API while the
        // app was closed); that restore clears without reporting a deletion.
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
    use super::{gui_state_path_for_db_path, normalize_storage_language};
    use localpaste_core::env::{env_lock, EnvGuard};
    use localpaste_core::Config;
    use tempfile::TempDir;

    #[test]
    fn storage_language_is_canonicalized() {
        assert_eq!(
            normalize_storage_language(Some(" Rust ".to_string())),
            Some("rust".to_string())
        );
        assert_eq!(normalize_storage_language(Some("   ".to_string())), None);
    }

    /// Runs `run` with the default store pointed into a fresh temp home.
    fn with_temp_home(run: impl FnOnce()) {
        let _lock = env_lock().lock().expect("env lock");
        let home_dir = TempDir::new().expect("temp home");
        let home = home_dir.path().to_str().expect("home path");
        let _home = EnvGuard::set("HOME", home);
        let _local_app_data = EnvGuard::set("LOCALAPPDATA", home);
        let _db_path = EnvGuard::remove("DB_PATH");
        run();
    }

    #[test]
    fn default_database_keeps_eframe_default_gui_state_location() {
        with_temp_home(|| {
            let db_path = Config::from_env().db_path;
            assert_eq!(gui_state_path_for_db_path(&db_path), None);
            #[cfg(unix)]
            {
                let _db_path = EnvGuard::set("DB_PATH", "~/.cache/localpaste/db/");
                let db_path = Config::from_env().db_path;
                assert_eq!(gui_state_path_for_db_path(&db_path), None);
            }
        });
    }

    #[test]
    fn custom_database_gui_state_path_is_absolute_inside_db_dir() {
        with_temp_home(|| {
            let dir = TempDir::new().expect("temp dir");
            let db_dir = dir.path().join("db");
            let db_str = db_dir.to_str().expect("db path");
            let expected = Some(db_dir.join("gui-state.ron"));
            assert_eq!(gui_state_path_for_db_path(db_str), expected);
            let trailing = format!("{db_str}{}", std::path::MAIN_SEPARATOR);
            assert_eq!(gui_state_path_for_db_path(&trailing), expected);

            let relative = gui_state_path_for_db_path("relative-db").expect("custom store");
            let cwd = std::env::current_dir().expect("cwd");
            assert_eq!(relative, cwd.join("relative-db").join("gui-state.ron"));
        });
    }
}
