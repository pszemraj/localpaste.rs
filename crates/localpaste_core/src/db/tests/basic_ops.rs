//! Basic database CRUD and corruption-handling tests.

use super::*;

#[test]
fn test_create_database() {
    let (db, _temp) = setup_test_db();
    assert!(db.flush().is_ok());
}

#[test]
fn test_paste_create_and_get() {
    let (db, _temp) = setup_test_db();

    let paste = Paste::new("Test content".to_string(), "test-paste".to_string());
    let paste_id = paste.id.clone();

    // Create
    assert!(db.pastes.create(&paste).is_ok());

    // Get
    let retrieved = db.pastes.get(&paste_id).unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.content, "Test content");
    assert_eq!(retrieved.id, paste_id);
}

#[test]
fn test_paste_update() {
    let (db, _temp) = setup_test_db();

    let paste = Paste::new("Original".to_string(), "test".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).unwrap();

    // Update
    let update = UpdatePasteRequest {
        content: Some("Updated".to_string()),
        name: Some("updated-name".to_string()),
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };

    let _updated = db.pastes.update(&paste_id, update).unwrap();
    assert!(_updated.is_some(), "Update should return Some");

    // Verify the update by retrieving the paste
    let retrieved = db.pastes.get(&paste_id).unwrap().unwrap();
    assert_eq!(retrieved.content, "Updated");
    assert_eq!(retrieved.name, "updated-name");
}

#[test]
fn test_manual_language_switches_to_auto_and_redetects() {
    let (db, _temp) = setup_test_db();

    let mut paste = Paste::new(
        "def main():\n    print('hello')".to_string(),
        "script".to_string(),
    );
    paste.language = Some("rust".to_string());
    paste.language_is_manual = true;
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).unwrap();

    let to_auto = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: Some(false),
        folder_id: None,
        tags: None,
    };
    let updated = db.pastes.update(&paste_id, to_auto).unwrap().unwrap();

    assert!(!updated.language_is_manual);
    assert_eq!(updated.language.as_deref(), Some("python"));
}

#[test]
fn test_content_update_redetects_language_in_auto_mode() {
    let (db, _temp) = setup_test_db();

    let paste = Paste::new(
        "fn main() {\n    let x = 1;\n}".to_string(),
        "script".to_string(),
    );
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).unwrap();

    let update = UpdatePasteRequest {
        content: Some("def main():\n    print('hello')".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };
    let updated = db.pastes.update(&paste_id, update).unwrap().unwrap();

    assert!(!updated.language_is_manual);
    assert_eq!(updated.language.as_deref(), Some("python"));
}

#[test]
fn test_language_update_without_manual_flag_sets_manual_override() {
    let (db, _temp) = setup_test_db();

    let paste = Paste::new(
        "def main():\n    print('hello')".to_string(),
        "script".to_string(),
    );
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).unwrap();

    let set_language = UpdatePasteRequest {
        content: None,
        name: None,
        language: Some("rust".to_string()),
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };
    let updated = db.pastes.update(&paste_id, set_language).unwrap().unwrap();
    assert_eq!(updated.language.as_deref(), Some("rust"));
    assert!(updated.language_is_manual);

    let content_update = UpdatePasteRequest {
        content: Some("def another():\n    print('world')".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };
    let after_content_update = db
        .pastes
        .update(&paste_id, content_update)
        .unwrap()
        .unwrap();
    assert_eq!(after_content_update.language.as_deref(), Some("rust"));
    assert!(after_content_update.language_is_manual);
}

#[test]
fn test_paste_delete() {
    let (db, _temp) = setup_test_db();

    let paste = Paste::new("To delete".to_string(), "test".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).unwrap();

    // Delete
    assert!(db.pastes.delete(&paste_id).is_ok());

    // Verify deleted
    let result = db.pastes.get(&paste_id).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_paste_list() {
    let (db, _temp) = setup_test_db();

    // Create multiple pastes
    for i in 0..5 {
        let paste = Paste::new(format!("Content {}", i), format!("paste-{}", i));
        db.pastes.create(&paste).unwrap();
    }

    // List
    let list = db.pastes.list(10, None).unwrap();
    assert_eq!(list.len(), 5);
}

#[test]
fn test_folder_crud() {
    let (db, _temp) = setup_test_db();

    let folder = Folder::new("Test Folder".to_string());
    let folder_id = folder.id.clone();

    // Create
    assert!(db.folders.create(&folder).is_ok());

    // Get
    let retrieved = db.folders.get(&folder_id).unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name, "Test Folder");

    // List
    let list = db.folders.list().unwrap();
    assert_eq!(list.len(), 1);

    // Delete
    assert!(db.folders.delete(&folder_id).is_ok());
    assert!(db.folders.get(&folder_id).unwrap().is_none());
}

#[test]
fn test_database_flush() {
    let (db, _temp) = setup_test_db();

    let paste = Paste::new("Test".to_string(), "test".to_string());
    db.pastes.create(&paste).unwrap();

    // Flush should succeed
    assert!(db.flush().is_ok());
}

#[test]
fn test_update_count_returns_not_found_for_missing_folder() {
    let (db, _temp) = setup_test_db();

    let result = db.folders.update_count("missing-folder-id", 1);
    assert!(
        matches!(result, Err(AppError::NotFound)),
        "missing folder should return NotFound"
    );
}

#[test]
fn test_folder_update_preserves_corrupt_record_on_error() {
    let (db, _temp) = setup_test_db();
    let tree = db.db.open_tree("folders").unwrap();
    let folder_id = "corrupt-folder-update-id";
    tree.insert(folder_id.as_bytes(), b"not-a-folder").unwrap();

    let result = db
        .folders
        .update(folder_id, "renamed".to_string(), Some(String::new()));
    assert!(
        matches!(result, Err(AppError::Serialization(_))),
        "corrupt folder value should surface serialization error"
    );
    assert!(
        tree.get(folder_id.as_bytes()).unwrap().is_some(),
        "corrupt record should not be deleted by failed folder update"
    );
}

#[test]
fn test_folder_update_count_preserves_corrupt_record_on_error() {
    let (db, _temp) = setup_test_db();
    let tree = db.db.open_tree("folders").unwrap();
    let folder_id = "corrupt-folder-id";
    tree.insert(folder_id.as_bytes(), b"not-a-folder").unwrap();

    let result = db.folders.update_count(folder_id, 1);
    assert!(
        matches!(result, Err(AppError::Serialization(_))),
        "corrupt folder value should surface serialization error"
    );
    assert!(
        tree.get(folder_id.as_bytes()).unwrap().is_some(),
        "corrupt record should not be deleted by failed update_count"
    );
}

#[test]
fn test_paste_update_preserves_corrupt_record_on_error() {
    let (db, _temp) = setup_test_db();
    let tree = db.db.open_tree("pastes").unwrap();
    let paste_id = "corrupt-paste-id";
    tree.insert(paste_id.as_bytes(), b"not-a-paste").unwrap();

    let update = UpdatePasteRequest {
        content: Some("new".to_string()),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };

    let result = db.pastes.update(paste_id, update);
    assert!(
        matches!(result, Err(AppError::Serialization(_))),
        "corrupt paste value should surface serialization error"
    );
    assert!(
        tree.get(paste_id.as_bytes()).unwrap().is_some(),
        "corrupt record should not be deleted by failed update"
    );
}
