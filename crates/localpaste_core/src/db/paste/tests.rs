//! Paste table tests.

use super::PasteDb;
use crate::db::tables::{PASTES, PASTES_BY_UPDATED, PASTES_META, REDB_FILE_NAME};
use crate::diff::{DiffRef, DiffRequest};
use crate::models::paste::{Paste, UpdatePasteRequest};
use crate::{AppError, MAX_DIFF_INPUT_BYTES};
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

#[test]
fn diff_resolution_uses_one_read_snapshot_for_both_refs() {
    let (_db, paste_db, _dir) = setup_paste_db();
    let paste = Paste::new("v1\n".to_string(), "diff-snapshot".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    let read_txn = paste_db.db.begin_read().expect("begin read");
    let updated = paste_db
        .update(
            &paste_id,
            UpdatePasteRequest {
                content: Some("v2\n".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: None,
                tags: None,
            },
        )
        .expect("update")
        .expect("paste exists");
    assert_eq!(updated.content, "v2\n");

    let diff = paste_db
        .diff_in_txn(
            &read_txn,
            &DiffRequest {
                left: DiffRef {
                    paste_id: paste_id.clone(),
                    version_id_ms: None,
                },
                right: DiffRef {
                    paste_id: paste_id.clone(),
                    version_id_ms: None,
                },
            },
        )
        .expect("diff in txn")
        .expect("resolved");
    assert!(
        diff.equal,
        "same-ref diff must stay equal within one snapshot"
    );
    assert!(
        diff.unified.is_empty(),
        "same-ref diff should not report changes from a later write"
    );

    let equal = paste_db
        .equal_in_txn(
            &read_txn,
            &DiffRequest {
                left: DiffRef {
                    paste_id: paste_id.clone(),
                    version_id_ms: None,
                },
                right: DiffRef {
                    paste_id,
                    version_id_ms: None,
                },
            },
        )
        .expect("equal in txn")
        .expect("resolved");
    assert!(
        equal.equal,
        "same-ref equality must stay true within one snapshot"
    );
}

#[test]
fn diff_rejects_oversized_requests_before_rendering_response() {
    let (_db, paste_db, _dir) = setup_paste_db();
    let oversized = "x".repeat((MAX_DIFF_INPUT_BYTES / 2) + 1);
    let left = Paste::new(oversized.clone(), "left".to_string());
    let right = Paste::new(oversized, "right".to_string());
    let left_id = left.id.clone();
    let right_id = right.id.clone();
    paste_db.create(&left).expect("create left");
    paste_db.create(&right).expect("create right");

    let err = paste_db
        .diff(&DiffRequest {
            left: DiffRef {
                paste_id: left_id,
                version_id_ms: None,
            },
            right: DiffRef {
                paste_id: right_id,
                version_id_ms: None,
            },
        })
        .expect_err("oversized diff should be rejected");

    assert!(
        matches!(err, AppError::PayloadTooLarge(ref message) if message.contains("Combined diff input exceeds")),
        "expected payload-too-large diff error, got {err:?}"
    );
}

#[test]
fn equal_rejects_oversized_requests_before_loading_compare_payloads() {
    let (_db, paste_db, _dir) = setup_paste_db();
    let oversized = "x".repeat((MAX_DIFF_INPUT_BYTES / 2) + 1);
    let left = Paste::new(oversized.clone(), "left".to_string());
    let right = Paste::new(oversized, "right".to_string());
    let left_id = left.id.clone();
    let right_id = right.id.clone();
    paste_db.create(&left).expect("create left");
    paste_db.create(&right).expect("create right");

    let err = paste_db
        .equal(&DiffRequest {
            left: DiffRef {
                paste_id: left_id,
                version_id_ms: None,
            },
            right: DiffRef {
                paste_id: right_id,
                version_id_ms: None,
            },
        })
        .expect_err("oversized equality check should be rejected");

    assert!(
        matches!(err, AppError::PayloadTooLarge(ref message) if message.contains("Combined diff input exceeds")),
        "expected payload-too-large equality error, got {err:?}"
    );
}

#[test]
fn identical_large_refs_bypass_combined_diff_size_gating() {
    let (_db, paste_db, _dir) = setup_paste_db();
    let oversized = "x".repeat((MAX_DIFF_INPUT_BYTES / 2) + 1);
    let paste = Paste::new(oversized, "same-large".to_string());
    let paste_id = paste.id.clone();
    paste_db.create(&paste).expect("create paste");

    let request = DiffRequest {
        left: DiffRef {
            paste_id: paste_id.clone(),
            version_id_ms: None,
        },
        right: DiffRef {
            paste_id,
            version_id_ms: None,
        },
    };

    let diff = paste_db
        .diff(&request)
        .expect("same-ref diff should not hit combined-size cap")
        .expect("resolved same-ref diff");
    assert!(diff.equal, "same-ref diff must report equality");
    assert!(
        diff.unified.is_empty(),
        "same-ref diff must not synthesize diff output"
    );

    let equal = paste_db
        .equal(&request)
        .expect("same-ref equality should not hit combined-size cap")
        .expect("resolved same-ref equality");
    assert!(equal.equal, "same-ref equality must stay true");
}
