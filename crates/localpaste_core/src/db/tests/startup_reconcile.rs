//! Startup behavior and invariant repair tests.

use super::*;
use crate::db::tables::REDB_FILE_NAME;
use tempfile::TempDir;

fn setup_temp_db_path(name: &str) -> (TempDir, String) {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join(name);
    let db_path_str = db_path.to_str().expect("db path").to_string();
    (temp_dir, db_path_str)
}

fn open_test_database(path: &str) -> Database {
    Database::new(path).expect("db")
}

#[test]
fn database_new_reports_error_for_non_directory_db_path() {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("not-a-db-file");
    std::fs::write(&db_path, b"not-a-db").expect("seed file");

    let result = Database::new(db_path.to_str().expect("path"));
    assert!(
        matches!(result, Err(AppError::StorageMessage(_))),
        "opening a non-directory DB_PATH should fail"
    );
}

#[test]
fn database_new_repairs_folder_count_drift_on_restart() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("test.db");

    let db = open_test_database(&db_path_str);
    let folder = Folder::new("count-drift-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");

    let mut paste_a = Paste::new("one".to_string(), "one".to_string());
    paste_a.folder_id = Some(folder_id.clone());
    let mut paste_b = Paste::new("two".to_string(), "two".to_string());
    paste_b.folder_id = Some(folder_id.clone());
    TransactionOps::create_paste_with_folder(&db, &paste_a, &folder_id).expect("create");
    TransactionOps::create_paste_with_folder(&db, &paste_b, &folder_id).expect("create");

    db.folders.set_count(&folder_id, 99).expect("drift");
    drop(db);

    let reopened = open_test_database(&db_path_str);
    let folder_after = reopened
        .folders
        .get(&folder_id)
        .expect("get folder")
        .expect("exists");
    let canonical_count = reopened
        .pastes
        .list(100, Some(folder_id.clone()))
        .expect("list")
        .len();
    assert_eq!(folder_after.paste_count, canonical_count);
}

#[test]
fn database_new_repairs_orphan_folder_refs_on_restart() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("test.db");

    let db = open_test_database(&db_path_str);
    let folder = Folder::new("orphan-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");

    let mut paste = Paste::new("orphan body".to_string(), "orphan".to_string());
    paste.folder_id = Some(folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &folder_id).expect("create");

    db.folders.delete(&folder_id).expect("delete folder");
    drop(db);

    let reopened = open_test_database(&db_path_str);
    let repaired = reopened
        .pastes
        .get(&paste_id)
        .expect("get paste")
        .expect("paste exists");
    assert!(repaired.folder_id.is_none());
}

#[test]
fn database_new_repairs_orphan_folder_parent_refs_on_restart() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("test.db");

    let db = open_test_database(&db_path_str);
    let root = Folder::new("root".to_string());
    let root_id = root.id.clone();
    db.folders.create(&root).expect("create root");

    let child = Folder::with_parent("child".to_string(), Some(root_id.clone()));
    let child_id = child.id.clone();
    db.folders.create(&child).expect("create child");

    db.folders.delete(&root_id).expect("delete root");
    drop(db);

    let reopened = open_test_database(&db_path_str);
    let repaired_child = reopened
        .folders
        .get(&child_id)
        .expect("get child")
        .expect("child exists");
    assert!(repaired_child.parent_id.is_none());
}

#[test]
fn database_new_clears_stale_folder_delete_markers() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("test.db");

    let db = open_test_database(&db_path_str);
    let folder = Folder::new("marker-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).expect("create folder");
    db.folders
        .mark_deleting(std::slice::from_ref(&folder_id))
        .expect("mark");
    assert!(db.folders.is_delete_marked(&folder_id).expect("marked"));
    drop(db);

    let reopened = open_test_database(&db_path_str);
    assert!(!reopened
        .folders
        .is_delete_marked(&folder_id)
        .expect("marked"));
}

#[test]
fn database_new_rejects_legacy_sled_layout_when_data_redb_missing() {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("legacy-db");
    std::fs::create_dir_all(&db_path).expect("create dir");
    std::fs::write(db_path.join("pastes"), b"legacy").expect("seed legacy artifact");

    let err = match Database::new(db_path.to_str().expect("path")) {
        Ok(_) => panic!("legacy sled layout without data.redb should fail"),
        Err(err) => err,
    };
    match err {
        AppError::StorageMessage(message) => {
            assert!(
                message.contains("legacy sled"),
                "error should describe legacy sled detection: {}",
                message
            );
            assert!(
                message.contains(REDB_FILE_NAME),
                "error should mention expected redb file: {}",
                message
            );
        }
        other => panic!("unexpected error variant: {:?}", other),
    }
}

#[test]
fn database_new_ignores_unrelated_lock_files_when_data_redb_missing() {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("non-legacy-db");
    std::fs::create_dir_all(&db_path).expect("create dir");
    std::fs::write(db_path.join("random.lock"), b"not-sled").expect("seed lock artifact");

    let db = Database::new(db_path.to_str().expect("path"))
        .expect("unrelated lock file should not block startup");
    drop(db);

    assert!(
        db_path.join(REDB_FILE_NAME).exists(),
        "database should initialize data.redb when no legacy sled markers are present"
    );
}

#[test]
fn database_new_allows_startup_when_data_redb_exists() {
    let (_temp_dir, db_path_str) = setup_temp_db_path("db");
    let db = open_test_database(&db_path_str);
    drop(db);

    // Add legacy-looking artifacts; `data.redb` still exists and should win.
    let db_path = std::path::Path::new(&db_path_str);
    std::fs::write(db_path.join("pastes"), b"legacy").expect("seed legacy artifact");
    open_test_database(&db_path_str);
}
