//! Paste table tests.

use super::PasteDb;
use crate::db::tables::{PASTES, PASTES_BY_UPDATED, PASTES_META, REDB_FILE_NAME};
use crate::models::paste::Paste;
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

    assert!(pastes
        .get(paste.id.as_str())
        .expect("get canonical")
        .is_none());
    assert!(metas.get(paste.id.as_str()).expect("get meta").is_none());
    let has_updated = updated
        .iter()
        .expect("iter updated")
        .any(|entry| entry.expect("entry").0.value().1 == paste.id.as_str());
    assert!(!has_updated);
}
