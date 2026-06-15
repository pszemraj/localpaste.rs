//! Headless tests for eframe GUI-state persistence.

use super::*;
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
    ];

    harness.app.ensure_selection_after_list_update();

    assert_eq!(harness.app.selected_id.as_deref(), Some("beta"));
    match recv_cmd(&harness.cmd_rx) {
        CoreCmd::GetPaste { id } => assert_eq!(id, "beta"),
        other => panic!("expected GetPaste command, got {other:?}"),
    }
}
