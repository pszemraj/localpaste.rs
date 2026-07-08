//! Deleted-paste undo lifecycle tests.

use super::*;
use crate::db::tables::{
    DELETED_PASTE_VERSIONS_CONTENT, PASTE_VERSIONS_CONTENT, PASTE_VERSIONS_META,
};
use crate::db::versioning::encode_version_meta_list;
use chrono::Utc;

fn seed_paste_with_version_rows(db: &Database, name: &str, version_rows: &[(u64, &str)]) -> String {
    let paste = Paste::new("current head".to_string(), name.to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create paste");

    let created_at = Utc::now();
    let version_items = version_rows
        .iter()
        .map(|(version_id_ms, content)| VersionMeta {
            version_id_ms: *version_id_ms,
            created_at,
            content_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
            len: content.len(),
            language: None,
            language_is_manual: false,
        })
        .collect::<Vec<_>>();
    let encoded_meta = encode_version_meta_list(&version_items).expect("encode version metadata");

    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut versions_meta = write_txn
            .open_table(PASTE_VERSIONS_META)
            .expect("open version meta");
        let mut versions_content = write_txn
            .open_table(PASTE_VERSIONS_CONTENT)
            .expect("open version content");
        versions_meta
            .insert(paste_id.as_str(), encoded_meta.as_slice())
            .expect("insert version metadata");
        for ((version_id_ms, content), _) in version_rows.iter().zip(version_items.iter()) {
            let encoded_content =
                bincode::serialize(&content.to_string()).expect("encode version content");
            versions_content
                .insert(
                    (paste_id.as_str(), *version_id_ms),
                    encoded_content.as_slice(),
                )
                .expect("insert version content");
        }
    }
    write_txn.commit().expect("commit version rows");
    paste_id
}

fn insert_extra_staged_version_content(db: &Database, token: &str, version_id_ms: u64) {
    let encoded_content =
        bincode::serialize(&"extra staged snapshot".to_string()).expect("encode staged content");
    let write_txn = db.db.begin_write().expect("begin write");
    {
        let mut deleted_versions_content = write_txn
            .open_table(DELETED_PASTE_VERSIONS_CONTENT)
            .expect("open deleted version content");
        deleted_versions_content
            .insert((token, version_id_ms), encoded_content.as_slice())
            .expect("insert extra staged version content");
    }
    write_txn.commit().expect("commit extra staged content");
}

#[test]
fn expired_restore_consumes_all_staged_rows_without_restoring_paste() {
    let (db, _temp) = setup_test_db();
    let paste_id = seed_versioned_paste(&db, "expired restore");
    let token = "expired-restore-token";

    assert!(
        TransactionOps::delete_paste_with_folder_staged_undo(&db, &paste_id, token, 0,)
            .expect("stage expired delete undo")
    );
    assert_staged_undo_token(&db, token, true);
    assert!(db
        .pastes
        .get(&paste_id)
        .expect("lookup after staged delete")
        .is_none());

    let restored = TransactionOps::restore_deleted_paste_by_token(&db, token)
        .expect("expired restore should be handled without storage failure");
    assert!(restored.is_none());
    assert_staged_undo_token(&db, token, false);
    assert!(db
        .pastes
        .get(&paste_id)
        .expect("lookup after expired restore")
        .is_none());
}

#[test]
fn discard_deleted_paste_undo_consumes_all_staged_rows() {
    let (db, _temp) = setup_test_db();
    let paste_id = seed_versioned_paste(&db, "discard undo");
    let token = "discard-token";

    assert!(
        TransactionOps::delete_paste_with_folder_staged_undo(&db, &paste_id, token, i64::MAX,)
            .expect("stage delete undo")
    );
    assert_staged_undo_token(&db, token, true);

    assert!(
        TransactionOps::discard_deleted_paste_undo(&db, token).expect("discard staged undo"),
        "first discard should remove the staged paste row"
    );
    assert_staged_undo_token(&db, token, false);
    assert!(
        !TransactionOps::discard_deleted_paste_undo(&db, token).expect("discard missing token"),
        "discard should be idempotent for already-consumed tokens"
    );
    assert!(db
        .pastes
        .get(&paste_id)
        .expect("lookup after discard")
        .is_none());
    assert!(TransactionOps::restore_deleted_paste_by_token(&db, token)
        .expect("restore discarded token")
        .is_none());
}

#[test]
fn discard_deleted_paste_undo_removes_unlisted_content_rows_with_valid_metadata() {
    let (db, _temp) = setup_test_db();
    let paste_id =
        seed_paste_with_version_rows(&db, "discard with extra content", &[(3_000, "snapshot")]);
    let token = "discard-extra-content-token";

    assert!(
        TransactionOps::delete_paste_with_folder_staged_undo(&db, &paste_id, token, i64::MAX,)
            .expect("stage delete undo")
    );
    insert_extra_staged_version_content(&db, token, 9_999);
    assert_staged_undo_token(&db, token, true);

    assert!(
        TransactionOps::discard_deleted_paste_undo(&db, token).expect("discard staged undo"),
        "discard should consume the staged paste row"
    );
    assert_staged_undo_token(&db, token, false);
}

#[test]
fn discard_all_deleted_paste_undo_consumes_all_staged_rows() {
    let (db, _temp) = setup_test_db();
    let expired_paste_id = seed_versioned_paste(&db, "discard all expired");
    let live_paste_id = seed_versioned_paste(&db, "discard all live");

    assert!(TransactionOps::delete_paste_with_folder_staged_undo(
        &db,
        &expired_paste_id,
        "discard-all-expired",
        10,
    )
    .expect("stage expired undo"));
    assert!(TransactionOps::delete_paste_with_folder_staged_undo(
        &db,
        &live_paste_id,
        "discard-all-live",
        i64::MAX,
    )
    .expect("stage live undo"));

    let mut discarded =
        TransactionOps::discard_all_deleted_paste_undo(&db).expect("discard all staged undo");
    discarded.sort();
    assert_eq!(
        discarded,
        vec![
            "discard-all-expired".to_string(),
            "discard-all-live".to_string()
        ]
    );
    assert_staged_undo_token(&db, "discard-all-expired", false);
    assert_staged_undo_token(&db, "discard-all-live", false);
    assert!(db
        .pastes
        .get(&expired_paste_id)
        .expect("lookup expired paste")
        .is_none());
    assert!(db
        .pastes
        .get(&live_paste_id)
        .expect("lookup live paste")
        .is_none());
}

#[test]
fn restore_deleted_paste_preserves_multiple_version_rows() {
    let (db, _temp) = setup_test_db();
    let version_rows = [(3_000, "newer snapshot"), (2_000, "older snapshot")];
    let paste_id = seed_paste_with_version_rows(&db, "multi-version undo", &version_rows);
    let before_versions = db
        .pastes
        .list_versions(&paste_id, None)
        .expect("list versions before delete")
        .expect("paste exists before delete");
    let before_snapshots = before_versions
        .iter()
        .map(|version| {
            db.pastes
                .get_version(&paste_id, version.version_id_ms)
                .expect("load version before delete")
                .expect("version exists before delete")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        before_versions
            .iter()
            .map(|version| version.version_id_ms)
            .collect::<Vec<_>>(),
        vec![3_000, 2_000]
    );

    let token = "multi-version-restore-token";
    assert!(
        TransactionOps::delete_paste_with_folder_staged_undo(&db, &paste_id, token, i64::MAX,)
            .expect("stage delete undo")
    );
    assert!(db
        .pastes
        .list_versions(&paste_id, None)
        .expect("list versions while deleted")
        .is_none());

    let restored = TransactionOps::restore_deleted_paste_by_token(&db, token)
        .expect("restore staged delete")
        .expect("paste restored");
    assert_eq!(restored.id, paste_id);
    assert_eq!(restored.content, "current head");
    assert_staged_undo_token(&db, token, false);

    let after_versions = db
        .pastes
        .list_versions(&paste_id, None)
        .expect("list versions after restore")
        .expect("paste exists after restore");
    assert_eq!(after_versions, before_versions);

    for before in before_snapshots {
        let after = db
            .pastes
            .get_version(&paste_id, before.version_id_ms)
            .expect("load version after restore")
            .expect("version exists after restore");
        assert_eq!(after, before);
    }
}
