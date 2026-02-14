use super::PasteDb;
use crate::db::tables::{PASTES, PASTES_BY_UPDATED, PASTES_META, REDB_FILE_NAME};
use crate::models::paste::{Paste, UpdatePasteRequest};
use redb::{ReadableDatabase, ReadableTable};
use std::sync::Arc;
use tempfile::TempDir;

fn setup_paste_db() -> (Arc<redb::Database>, PasteDb, TempDir) {
    let dir = TempDir::new().expect("temp dir");
    let db_file = dir.path().join(REDB_FILE_NAME);
    let db = Arc::new(redb::Database::create(db_file).expect("create redb"));
    let paste_db = PasteDb::new(db.clone()).expect("paste db");
    (db, paste_db, dir)
}

#[test]
fn create_get_update_delete_roundtrip() {
    let (_db, paste_db, _dir) = setup_paste_db();

    let paste = Paste::new("hello".to_string(), "name".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create");

    let loaded = paste_db.get(&paste_id).expect("get").expect("exists");
    assert_eq!(loaded.content, "hello");
    assert_eq!(loaded.name, "name");

    let update = UpdatePasteRequest {
        content: Some("updated".to_string()),
        name: Some("renamed".to_string()),
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: Some(vec!["tag".to_string()]),
    };
    let updated = paste_db
        .update(&paste_id, update)
        .expect("update")
        .expect("updated");
    assert_eq!(updated.content, "updated");
    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.tags, vec!["tag".to_string()]);

    assert!(paste_db.delete(&paste_id).expect("delete"));
    assert!(paste_db.get(&paste_id).expect("lookup").is_none());
}

#[test]
fn list_meta_orders_by_updated_desc() {
    let (_db, paste_db, _dir) = setup_paste_db();
    let now = chrono::Utc::now();

    let mut older = Paste::new("old".to_string(), "old".to_string());
    older.updated_at = now - chrono::Duration::minutes(5);
    let older_id = older.id.clone();

    let mut newer = Paste::new("new".to_string(), "new".to_string());
    newer.updated_at = now;
    let newer_id = newer.id.clone();

    paste_db.create(&older).expect("create older");
    paste_db.create(&newer).expect("create newer");

    let metas = paste_db.list_meta(10, None).expect("list meta");
    assert_eq!(metas.len(), 2);
    assert_eq!(metas[0].id, newer_id);
    assert_eq!(metas[1].id, older_id);
}

#[test]
fn create_rejects_duplicate_id() {
    let (_db, paste_db, _dir) = setup_paste_db();

    let mut first = Paste::new("one".to_string(), "one".to_string());
    first.id = "duplicate-id".to_string();
    paste_db.create(&first).expect("create first");

    let mut second = Paste::new("two".to_string(), "two".to_string());
    second.id = "duplicate-id".to_string();
    let err = paste_db.create(&second).expect_err("duplicate create should fail");
    assert!(
        matches!(err, crate::AppError::StorageMessage(ref msg) if msg.contains("already exists")),
        "unexpected error: {}",
        err
    );
}

#[test]
fn aborted_write_transaction_leaves_no_partial_rows() {
    let (db, _paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("atomic".to_string(), "atomic".to_string());
    let encoded = bincode::serialize(&paste).expect("serialize");

    {
        let write_txn = db.begin_write().expect("begin write");
        let mut pastes = write_txn.open_table(PASTES).expect("open pastes");
        pastes
            .insert(paste.id.as_str(), encoded.as_slice())
            .expect("insert canonical");
        // Intentionally do not commit.
    }

    let read_txn = db.begin_read().expect("begin read");
    let pastes = read_txn.open_table(PASTES).expect("open pastes");
    let metas = read_txn.open_table(PASTES_META).expect("open metas");
    let updated = read_txn
        .open_table(PASTES_BY_UPDATED)
        .expect("open updated");

    assert!(pastes.get(paste.id.as_str()).expect("get canonical").is_none());
    assert!(metas.get(paste.id.as_str()).expect("get meta").is_none());
    let has_updated = updated
        .iter()
        .expect("iter updated")
        .any(|entry| entry.expect("entry").0.value().1 == paste.id.as_str());
    assert!(!has_updated);
}

#[test]
fn search_distinguishes_content_vs_meta_queries() {
    let (_db, paste_db, _dir) = setup_paste_db();

    let by_name = Paste::new("body".to_string(), "needle-name".to_string());
    let content_only = Paste::new("needle-in-content".to_string(), "plain".to_string());
    paste_db.create(&by_name).expect("create by-name");
    paste_db.create(&content_only).expect("create content-only");

    let content_results = paste_db.search("needle", 10, None, None).expect("search");
    assert!(
        content_results.iter().any(|meta| meta.id == by_name.id),
        "name match should be found in canonical search"
    );
    assert!(
        content_results.iter().any(|meta| meta.id == content_only.id),
        "content match should be found in canonical search"
    );

    let meta_results = paste_db.search_meta("needle", 10, None, None).expect("meta");
    assert!(
        meta_results.iter().any(|meta| meta.id == by_name.id),
        "name match should be found in metadata search"
    );
    assert!(
        !meta_results.iter().any(|meta| meta.id == content_only.id),
        "content-only match should not be found in metadata search"
    );
}
