//! Folder transaction behavior tests.

use super::*;

#[test]
fn create_with_folder_rejects_when_folder_is_marked_for_delete() {
    let (db, _temp) = setup_test_db();

    let folder = Folder::new("target-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");
    db.folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .expect("mark deleting");

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(folder_id.clone());
    let paste_id = paste.id.clone();

    let result = TransactionOps::create_paste_with_folder(&db, &paste, &folder_id);
    assert!(matches!(result, Err(AppError::BadRequest(_))));
    assert!(db.pastes.get(&paste_id).expect("lookup").is_none());
}

#[test]
fn create_with_folder_duplicate_id_keeps_counts_consistent() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).expect("create old");

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).expect("create new");

    let mut existing = Paste::new("original".to_string(), "name".to_string());
    existing.folder_id = Some(old_folder_id.clone());
    let paste_id = existing.id.clone();
    TransactionOps::create_paste_with_folder(&db, &existing, &old_folder_id).expect("create");

    let mut duplicate = Paste::new("conflicting".to_string(), "name".to_string());
    duplicate.id = paste_id.clone();
    duplicate.folder_id = Some(new_folder_id.clone());

    let result = TransactionOps::create_paste_with_folder(&db, &duplicate, &new_folder_id);
    assert!(
        matches!(result, Err(AppError::StorageMessage(ref message)) if message.contains("already exists")),
        "duplicate id create should fail without count drift: {:?}",
        result
    );

    let old_after = db.folders.get(&old_folder_id).expect("old").expect("row");
    let new_after = db.folders.get(&new_folder_id).expect("new").expect("row");
    assert_eq!(old_after.paste_count, 1);
    assert_eq!(new_after.paste_count, 0);
}

#[test]
fn move_between_folders_updates_counts_and_assignment() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).expect("create old");

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).expect("create new");

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(old_folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).expect("create");

    let update = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(new_folder_id.clone()),
        tags: None,
    };

    let moved = TransactionOps::move_paste_between_folders(
        &db,
        &paste_id,
        Some(new_folder_id.as_str()),
        update,
    )
    .expect("move")
    .expect("paste exists");

    assert_eq!(moved.folder_id.as_deref(), Some(new_folder_id.as_str()));
    let old_after = db.folders.get(&old_folder_id).expect("old").expect("row");
    let new_after = db.folders.get(&new_folder_id).expect("new").expect("row");
    assert_eq!(old_after.paste_count, 0);
    assert_eq!(new_after.paste_count, 1);
}

#[test]
fn move_missing_paste_returns_none_without_count_drift() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).expect("create old");

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).expect("create new");

    let update = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(new_folder_id.clone()),
        tags: None,
    };

    let result = TransactionOps::move_paste_between_folders(
        &db,
        "missing-paste-id",
        Some(new_folder_id.as_str()),
        update,
    )
    .expect("move");
    assert!(result.is_none());

    let old_after = db.folders.get(&old_folder_id).expect("old").expect("row");
    let new_after = db.folders.get(&new_folder_id).expect("new").expect("row");
    assert_eq!(old_after.paste_count, 0);
    assert_eq!(new_after.paste_count, 0);
}

#[test]
fn delete_uses_folder_from_deleted_record_not_stale_context() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).expect("create old");

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).expect("create new");

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(old_folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).expect("create");

    let move_req = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(new_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        &db,
        &paste_id,
        Some(new_folder_id.as_str()),
        move_req,
    )
    .expect("move")
    .expect("paste exists");

    let deleted = TransactionOps::delete_paste_with_folder(&db, &paste_id).expect("delete");
    assert!(deleted);

    let old_after = db.folders.get(&old_folder_id).expect("old").expect("row");
    let new_after = db.folders.get(&new_folder_id).expect("new").expect("row");
    assert_eq!(old_after.paste_count, 0);
    assert_eq!(new_after.paste_count, 0);
}
