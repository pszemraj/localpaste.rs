//! Basic database CRUD tests.

use super::*;
use crate::db::tables::{FOLDERS, PASTES};
use redb::ReadableDatabase;

#[test]
fn test_create_database_and_flush_noop() {
    let (db, _temp) = setup_test_db();
    assert!(db.flush().is_ok());
}

#[test]
fn from_shared_reuses_folder_transaction_lock_for_same_shared_db() {
    let (db, _temp) = setup_test_db();
    let shared = db.db.clone();

    let handle_a = Database::from_shared(shared.clone()).expect("from_shared handle A");
    let handle_b = Database::from_shared(shared).expect("from_shared handle B");

    assert!(
        Arc::ptr_eq(&handle_a.folder_txn_lock, &handle_b.folder_txn_lock),
        "from_shared handles over the same Arc<Database> must share folder transaction lock"
    );
}

#[test]
fn paste_create_get_update_delete_roundtrip() {
    let (db, _temp) = setup_test_db();

    let paste = Paste::new("Test content".to_string(), "test-paste".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    let retrieved = db
        .pastes
        .get(&paste_id)
        .expect("get")
        .expect("paste should exist");
    assert_eq!(retrieved.content, "Test content");
    assert_eq!(retrieved.id, paste_id);

    let update = UpdatePasteRequest {
        content: Some("Updated".to_string()),
        name: Some("updated-name".to_string()),
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };
    let updated = db
        .pastes
        .update(&paste_id, update)
        .expect("update")
        .expect("updated");
    assert_eq!(updated.content, "Updated");
    assert_eq!(updated.name, "updated-name");

    assert!(db.pastes.delete(&paste_id).expect("delete"));
    assert!(db.pastes.get(&paste_id).expect("get").is_none());
}

#[test]
fn paste_create_rejects_duplicate_id_without_overwrite() {
    let (db, _temp) = setup_test_db();

    let mut original = Paste::new("original".to_string(), "first".to_string());
    original.id = "duplicate-create-id".to_string();
    db.pastes.create(&original).expect("create original");

    let mut conflicting = Paste::new("conflicting".to_string(), "second".to_string());
    conflicting.id = original.id.clone();
    let err = db
        .pastes
        .create(&conflicting)
        .expect_err("duplicate id create must fail");
    assert!(
        matches!(err, AppError::StorageMessage(ref message) if message.contains("already exists")),
        "unexpected duplicate-create error: {}",
        err
    );

    let stored = db
        .pastes
        .get(&original.id)
        .expect("lookup")
        .expect("existing paste should remain");
    assert_eq!(stored.content, "original");
    assert_eq!(stored.name, "first");
}

#[test]
fn folder_crud_and_duplicate_rejection() {
    let (db, _temp) = setup_test_db();

    let folder = Folder::new("Test Folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create");

    let retrieved = db.folders.get(&folder_id).expect("get");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.expect("folder").name, "Test Folder");

    let list = db.folders.list().expect("list");
    assert_eq!(list.len(), 1);

    let mut duplicate = Folder::new("Other".to_string());
    duplicate.id = folder_id.clone();
    let err = db
        .folders
        .create(&duplicate)
        .expect_err("duplicate folder id create must fail");
    assert!(matches!(err, AppError::StorageMessage(_)));

    assert!(db.folders.delete(&folder_id).expect("delete"));
    assert!(db.folders.get(&folder_id).expect("get").is_none());
}

#[test]
fn update_count_returns_not_found_for_missing_folder() {
    let (db, _temp) = setup_test_db();

    let result = db.folders.update_count("missing-folder-id", 1);
    assert!(
        matches!(result, Err(AppError::NotFound)),
        "missing folder should return NotFound"
    );
}

#[test]
fn clear_delete_markers_resets_table_and_allows_reuse() {
    let (db, _temp) = setup_test_db();
    let folder = Folder::new("marker-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");

    db.folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .expect("mark deleting");
    assert!(
        db.folders.is_delete_marked(&folder_id).expect("is marked"),
        "marker should exist before clear"
    );

    db.folders.clear_delete_markers().expect("clear markers");
    assert!(
        !db.folders
            .is_delete_marked(&folder_id)
            .expect("is unmarked"),
        "marker should be cleared"
    );

    db.folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .expect("re-mark deleting after clear");
    assert!(
        db.folders.is_delete_marked(&folder_id).expect("re-marked"),
        "delete-marker table should remain usable after clear"
    );
}

#[test]
fn update_language_transitions_between_manual_and_auto_modes() {
    let (db, _temp) = setup_test_db();
    let paste = Paste::new(
        "fn main() {\n    let x = 5;\n    println!(\"hello\");\n}".to_string(),
        "lang".to_string(),
    );
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    let set_manual = UpdatePasteRequest {
        content: None,
        name: None,
        language: Some("python".to_string()),
        language_is_manual: Some(true),
        folder_id: None,
        tags: None,
    };
    let manual = db
        .pastes
        .update(&paste_id, set_manual)
        .expect("manual update")
        .expect("paste exists");
    assert_eq!(manual.language.as_deref(), Some("python"));
    assert!(
        manual.language_is_manual,
        "language should be manual after override"
    );

    let switch_back_to_auto = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: Some(false),
        folder_id: None,
        tags: None,
    };
    let auto = db
        .pastes
        .update(&paste_id, switch_back_to_auto)
        .expect("switch to auto")
        .expect("paste exists");
    assert_eq!(auto.language.as_deref(), Some("rust"));
    assert!(
        !auto.language_is_manual,
        "language should be auto-managed after switch"
    );

    let auto_redetect_on_content_change = UpdatePasteRequest {
        content: Some("def main():\n    import sys\n    print('hello')".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };
    let redetected = db
        .pastes
        .update(&paste_id, auto_redetect_on_content_change)
        .expect("content update")
        .expect("paste exists");
    assert_eq!(redetected.language.as_deref(), Some("python"));
    assert!(
        !redetected.language_is_manual,
        "auto mode should keep manual flag disabled after content redetect"
    );
}

#[test]
fn corrupt_rows_surface_serialization_errors_without_removal() {
    let (db, _temp) = setup_test_db();

    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut folders = write_txn.open_table(FOLDERS).expect("open folders");
        folders
            .insert("corrupt-folder", b"not-a-folder".as_slice())
            .expect("insert corrupt folder");

        let mut pastes = write_txn.open_table(PASTES).expect("open pastes");
        pastes
            .insert("corrupt-paste", b"not-a-paste".as_slice())
            .expect("insert corrupt paste");
    }
    write_txn.commit().expect("commit");

    let folder_update =
        db.folders
            .update("corrupt-folder", "renamed".to_string(), Some(String::new()));
    assert!(matches!(folder_update, Err(AppError::Serialization(_))));

    let paste_update = db.pastes.update(
        "corrupt-paste",
        UpdatePasteRequest {
            content: Some("x".to_string()),
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: None,
            tags: None,
        },
    );
    assert!(matches!(paste_update, Err(AppError::Serialization(_))));

    let read_txn = db.db.begin_read().expect("begin read");
    let folders = read_txn.open_table(FOLDERS).expect("open folders");
    let pastes = read_txn.open_table(PASTES).expect("open pastes");
    assert!(folders.get("corrupt-folder").expect("folder get").is_some());
    assert!(pastes.get("corrupt-paste").expect("paste get").is_some());
}
