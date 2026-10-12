//! Headless tests for eframe GUI-state persistence.

use super::*;
use localpaste_core::env::{env_lock, EnvGuard};
use std::collections::HashMap;

#[derive(Default)]
struct MemoryStorage {
    values: HashMap<String, String>,
}

impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.values.insert(key.to_string(), value);
    }

    fn flush(&mut self) {}
}

#[test]
fn gui_storage_roundtrip_restores_filters_and_pending_selection() {
    let mut first = make_app();
    first.app.selected_id = Some("beta".to_string());
    first.app.active_collection = SidebarCollection::Code;
    first.app.active_language_filter = Some("rust".to_string());

    let mut storage = MemoryStorage::default();
    first.app.save_gui_storage(&mut storage);

    let mut restored = make_app();
    restored.app.selected_id = None;
    restored.app.selected_paste = None;
    restored.app.pending_selection_id = None;
    restored.app.active_collection = SidebarCollection::All;
    restored.app.active_language_filter = None;
    restored.app.restore_gui_storage(Some(&storage));

    assert_eq!(restored.app.pending_selection_id.as_deref(), Some("beta"));
    assert_eq!(restored.app.active_collection, SidebarCollection::Code);
    assert_eq!(restored.app.active_language_filter.as_deref(), Some("rust"));
}

#[test]
fn pending_restored_selection_applies_after_visible_list_refresh() {
    let mut harness = make_app();
    harness.app.selected_id = None;
    harness.app.selected_paste = None;
    harness.app.pending_selection_id = Some("beta".to_string());
    harness.app.pastes = vec![
        test_summary("alpha", "Alpha", None, 7),
        test_summary("beta", "Beta", None, 4),
    ]
    .into();

    harness.app.ensure_selection_after_list_update();

    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id, .. } => assert_eq!(id, "beta"),
        other => panic!("expected GetPaste command, got {other:?}"),
    }
}

#[test]
fn pending_restored_selection_loads_even_when_current_filters_hide_it() {
    let mut harness = make_app();
    harness.app.selected_id = None;
    harness.app.selected_paste = None;
    harness.app.pending_selection_id = Some("beta".to_string());
    harness.app.active_collection = SidebarCollection::Code;
    harness.app.pastes = vec![test_summary("alpha", "Alpha", Some("rust"), 7)].into();
    harness.app.all_pastes = vec![
        test_summary("alpha", "Alpha", Some("rust"), 7),
        test_summary("beta", "Beta", None, 4),
    ];

    harness.app.ensure_selection_after_list_update();

    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    assert!(harness.app.pending_selection_id.is_none());
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id, .. } => assert_eq!(id, "beta"),
        other => panic!("expected GetPaste command, got {other:?}"),
    }
}

#[test]
fn restored_selection_that_no_longer_exists_clears_without_a_message() {
    let mut storage = MemoryStorage::default();
    eframe::Storage::set_string(&mut storage, STORAGE_SELECTED_ID_KEY, "gone".to_string());
    let mut harness = make_app();
    harness.app.selected_id = None;
    harness.app.selected_paste = None;
    harness.app.restore_gui_storage(Some(&storage));

    harness.app.ensure_selection_after_list_update();
    assert_eq!(harness.app.selected_id.as_deref(), Some("gone"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id, .. } => assert_eq!(id, "gone"),
        other => panic!("expected GetPaste command, got {other:?}"),
    }
    harness.app.apply_event(CoreEvent::PasteSelectionMissing {
        id: "gone".to_string(),
        selection_epoch: harness.app.active_buffer_epoch,
    });
    assert_ne!(harness.app.selected_id.as_deref(), Some("gone"));
    assert!(harness.app.status.is_none());
    assert!(harness.app.toasts.is_empty());

    // Deleting an open paste elsewhere is still reported.
    harness.app.selected_id = Some("alpha".to_string());
    let mut alpha = Paste::new("content".to_string(), "Alpha".to_string());
    alpha.id = "alpha".to_string();
    harness.app.selected_paste = Some(alpha);
    harness.app.apply_event(CoreEvent::PasteMissing {
        id: "alpha".to_string(),
    });
    assert_eq!(
        harness
            .app
            .status
            .as_ref()
            .map(|status| status.text.as_str()),
        Some("Selected paste was deleted; list refreshed.")
    );
}

#[test]
fn native_options_keep_custom_database_gui_state_in_its_directory() {
    // Point the default store into a temp home so the check never touches the real one.
    let _lock = env_lock().lock().expect("env lock");
    let home = TempDir::new().expect("temp home");
    let _home = EnvGuard::set("HOME", home.path().to_str().expect("home path"));
    let harness = make_app();
    let db_dir = std::path::absolute(&harness.app.db_path).expect("absolute db path");

    let options = crate::native_options(&harness.app);
    assert_eq!(
        options.persistence_path,
        Some(db_dir.join("gui-state.ron")),
        "a non-default store must not share eframe's per-user state file"
    );
}

#[test]
fn gui_state_file_does_not_disturb_opening_its_database() {
    let dir = TempDir::new().expect("temp dir");
    let db_dir = dir.path().join("db");
    std::fs::create_dir_all(&db_dir).expect("create db dir");
    let state_file = db_dir.join("gui-state.ron");
    std::fs::write(&state_file, "{}").expect("write state file");

    let db = Database::new(db_dir.to_str().expect("db path"))
        .expect("a GUI state file alone must not read as a legacy store");
    drop(db);
    assert_eq!(std::fs::read_to_string(&state_file).expect("read"), "{}");
}
