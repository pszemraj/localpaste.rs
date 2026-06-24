//! Database compatibility tests for version-history row shapes.

use super::*;
use crate::db::tables::{PASTE_VERSIONS_CONTENT, PASTE_VERSIONS_META};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct LegacyVersionMetaForTest {
    version_id_ms: u64,
    created_at: DateTime<Utc>,
    content_hash: String,
    len: usize,
}

#[test]
fn list_and_load_versions_decode_legacy_metadata_without_language_fields() {
    let (db, _temp) = setup_test_db();
    let paste = Paste::new("current".to_string(), "legacy-version-meta".to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create");

    let archived_content = "legacy snapshot";
    let version_id_ms = Utc::now().timestamp_millis().max(0) as u64;
    let legacy_meta = vec![LegacyVersionMetaForTest {
        version_id_ms,
        created_at: Utc::now(),
        content_hash: blake3::hash(archived_content.as_bytes())
            .to_hex()
            .to_string(),
        len: archived_content.len(),
    }];
    let encoded_meta = bincode::serialize(&legacy_meta).expect("serialize legacy metadata");
    let encoded_content = bincode::serialize(&archived_content.to_string()).expect("content");
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
            .expect("insert legacy metadata");
        versions_content
            .insert(
                (paste_id.as_str(), version_id_ms),
                encoded_content.as_slice(),
            )
            .expect("insert legacy content");
    }
    write_txn.commit().expect("commit legacy metadata");

    let versions = db
        .pastes
        .list_versions(&paste_id, Some(10))
        .expect("list legacy versions")
        .expect("paste exists");
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].version_id_ms, version_id_ms);
    assert_eq!(versions[0].language, None);
    assert!(!versions[0].language_is_manual);

    let snapshot = db
        .pastes
        .get_version(&paste_id, version_id_ms)
        .expect("load legacy version")
        .expect("version exists");
    assert_eq!(snapshot.content, archived_content);
    assert_eq!(snapshot.language, None);
    assert!(!snapshot.language_is_manual);
}
