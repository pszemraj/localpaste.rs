//! Basic database CRUD tests.

use super::*;
use crate::db::tables::{FOLDERS, PASTES};
use redb::ReadableDatabase;

fn update_request(
    content: Option<&str>,
    name: Option<&str>,
    language: Option<&str>,
    language_is_manual: Option<bool>,
) -> UpdatePasteRequest {
    UpdatePasteRequest {
        content: content.map(ToString::to_string),
        name: name.map(ToString::to_string),
        language: language.map(ToString::to_string),
        language_is_manual,
        folder_id: None,
        tags: None,
    }
}

fn update_existing_paste(
    db: &Database,
    paste_id: &str,
    request: UpdatePasteRequest,
    context: &str,
) -> Paste {
    db.pastes
        .update(paste_id, request)
        .expect(context)
        .expect("paste exists")
}

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
        Arc::ptr_eq(&db.folder_txn_lock, &handle_a.folder_txn_lock),
        "Database::new and Database::from_shared handles over the same Arc<Database> must share folder transaction lock"
    );
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

    let update = update_request(Some("Updated"), Some("updated-name"), None, None);
    let updated = update_existing_paste(&db, &paste_id, update, "update");
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
fn language_mode_transitions_cover_auto_and_manual_lock_behaviors() {
    let (db, _temp) = setup_test_db();
    let paste = Paste::new(
        "fn main() {\n    let x = 5;\n    println!(\"hello\");\n}".to_string(),
        "lang".to_string(),
    );
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    let set_manual = update_request(None, None, Some("python"), Some(true));
    let manual = update_existing_paste(&db, &paste_id, set_manual, "manual update");
    assert_eq!(manual.language.as_deref(), Some("python"));
    assert!(
        manual.language_is_manual,
        "language should be manual after override"
    );

    let switch_back_to_auto = update_request(None, None, None, Some(false));
    let auto = update_existing_paste(&db, &paste_id, switch_back_to_auto, "switch to auto");
    assert!(
        auto.language.is_none(),
        "auto toggle should clear resolved language"
    );
    assert!(
        !auto.language_is_manual,
        "language should be pending auto-detect after switch"
    );

    let auto_detect_and_lock = update_request(
        Some("def main():\n    import sys\n    print('hello')"),
        None,
        None,
        None,
    );
    let redetected = update_existing_paste(&db, &paste_id, auto_detect_and_lock, "content update");
    assert_eq!(redetected.language.as_deref(), Some("python"));
    assert!(
        redetected.language_is_manual,
        "auto detect should lock once a concrete language is resolved"
    );
    let lock_paste = Paste::new(
        "name: alpha\nvalue: 1\n".to_string(),
        "lang-lock".to_string(),
    );
    let lock_paste_id = lock_paste.id.clone();
    db.pastes.create(&lock_paste).expect("create");

    let set_manual = update_request(None, None, Some("markdown"), Some(true));
    let manual = update_existing_paste(&db, &lock_paste_id, set_manual, "set manual");
    assert_eq!(manual.language.as_deref(), Some("markdown"));
    assert!(manual.language_is_manual);

    let content_update = update_request(
        Some("---\nkey: value\nnested:\n  - one\n"),
        None,
        None,
        None,
    );
    let updated = update_existing_paste(&db, &lock_paste_id, content_update, "content update");

    assert_eq!(
        updated.language.as_deref(),
        Some("markdown"),
        "manual language should persist across content edits"
    );
    assert!(updated.language_is_manual);
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

    let paste_update = db
        .pastes
        .update("corrupt-paste", update_request(Some("x"), None, None, None));
    assert!(matches!(paste_update, Err(AppError::Serialization(_))));

    let read_txn = db.db.begin_read().expect("begin read");
    let folders = read_txn.open_table(FOLDERS).expect("open folders");
    let pastes = read_txn.open_table(PASTES).expect("open pastes");
    assert!(folders.get("corrupt-folder").expect("folder get").is_some());
    assert!(pastes.get("corrupt-paste").expect("paste get").is_some());
}
