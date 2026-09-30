//! Document projection upgrade uses the ordinary startup backup/rebuild path.

use super::*;
use crate::db::paste::META_SCHEMA_VERSION_KEY;
use crate::db::tables::{PASTES_META, PASTES_META_STATE};
use crate::semantic::PasteKind;

#[test]
fn documents_rebuild_from_version_two_without_changing_canonical_content() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("db");
    let db = open_test_database(path.to_str().unwrap());
    let paste = Paste::new_with_language(
        "# Code notes\n```rust\nfn main() {}\n```\n".into(),
        "old code filter".into(),
        Some("markdown".into()),
        true,
    );
    db.pastes.create(&paste).unwrap();
    let mut stale = PasteMeta::from(&paste);
    stale.derived.kind = PasteKind::Code;
    let txn = db.db.begin_write().unwrap();
    txn.open_table(PASTES_META)
        .unwrap()
        .insert(
            paste.id.as_str(),
            bincode::serialize(&stale).unwrap().as_slice(),
        )
        .unwrap();
    txn.open_table(PASTES_META_STATE)
        .unwrap()
        .insert(
            META_SCHEMA_VERSION_KEY,
            bincode::serialize(&2_u64).unwrap().as_slice(),
        )
        .unwrap();
    txn.commit().unwrap();
    drop(db);

    let reopened = open_test_database(path.to_str().unwrap());
    assert_eq!(
        reopened.pastes.list_meta(10, None).unwrap()[0].derived.kind,
        PasteKind::Document
    );
    assert_eq!(
        reopened.pastes.get(&paste.id).unwrap().unwrap().content,
        paste.content
    );
    assert!(
        std::fs::read_dir(&path).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".backup.")),
        "upgrade must preserve a schema-repair backup"
    );
    drop(reopened);
    let again = open_test_database(path.to_str().unwrap());
    assert_eq!(
        again.pastes.list_meta(10, None).unwrap()[0].derived.kind,
        PasteKind::Document
    );
}
