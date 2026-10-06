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

#[test]
fn semantic_kinds_rebuild_from_version_six_and_survive_restart() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("db");
    let db = open_test_database(path.to_str().unwrap());
    let cases = [
        (
            "VGhpcyBpcyBhIHNlY3JldCB0b2tlbg==",
            "text",
            PasteKind::Document,
            PasteKind::Other,
        ),
        (
            "debug: true\nname: foo",
            "text",
            PasteKind::Log,
            PasteKind::Config,
        ),
        (
            "Warning: do not touch the deployment settings.",
            "text",
            PasteKind::Log,
            PasteKind::Document,
        ),
        (
            "Hello, Bob\nSee you soon, Alice",
            "text",
            PasteKind::Other,
            PasteKind::Document,
        ),
        (
            "Just a reminder to save your work.",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "```python\nprint('hello')\n```",
            "markdown",
            PasteKind::Document,
            PasteKind::Code,
        ),
        (
            "```json\n{\"a\":1}\n```",
            "markdown",
            PasteKind::Document,
            PasteKind::Config,
        ),
        (
            "sudo systemctl restart nginx",
            "text",
            PasteKind::Document,
            PasteKind::Code,
        ),
        (
            "INFO Starting the server",
            "text",
            PasteKind::Document,
            PasteKind::Log,
        ),
        (
            "[INFO] Server started successfully",
            "text",
            PasteKind::Document,
            PasteKind::Log,
        ),
        (
            "thread 'main' panicked at src/main.rs:12:5",
            "text",
            PasteKind::Document,
            PasteKind::Log,
        ),
        (
            "Git is down for scheduled maintenance.",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "[INFO] Server started\nINFO Starting worker\nWARN Queue full",
            "dockerfile",
            PasteKind::Config,
            PasteKind::Log,
        ),
        (
            "thread 'main' panicked at src/main.rs:12:5",
            "html",
            PasteKind::Code,
            PasteKind::Log,
        ),
    ];
    let pastes: Vec<_> = cases
        .iter()
        .map(|(content, language, _, _)| {
            Paste::new_with_language(
                (*content).into(),
                "review notes".into(),
                Some((*language).into()),
                true,
            )
        })
        .collect();
    for paste in &pastes {
        db.pastes.create(paste).unwrap();
    }
    let txn = db.db.begin_write().unwrap();
    {
        let mut metas = txn.open_table(PASTES_META).unwrap();
        for (paste, (_, _, stale_kind, _)) in pastes.iter().zip(&cases) {
            let mut stale = PasteMeta::from(paste);
            stale.derived.kind = *stale_kind;
            metas
                .insert(
                    paste.id.as_str(),
                    bincode::serialize(&stale).unwrap().as_slice(),
                )
                .unwrap();
        }
    }
    txn.open_table(PASTES_META_STATE)
        .unwrap()
        .insert(
            META_SCHEMA_VERSION_KEY,
            bincode::serialize(&6_u64).unwrap().as_slice(),
        )
        .unwrap();
    txn.commit().unwrap();
    drop(db);

    for _ in 0..2 {
        let reopened = open_test_database(path.to_str().unwrap());
        let metas = reopened.pastes.list_meta(100, None).unwrap();
        for (paste, (_, _, _, expected)) in pastes.iter().zip(&cases) {
            assert_eq!(
                metas
                    .iter()
                    .find(|meta| meta.id == paste.id)
                    .unwrap()
                    .derived
                    .kind,
                *expected
            );
            let stored = reopened.pastes.get(&paste.id).unwrap().unwrap();
            assert_eq!(stored.content, paste.content);
            assert_eq!(stored.language, paste.language);
            assert_eq!(stored.language_is_manual, paste.language_is_manual);
        }
    }
    assert!(std::fs::read_dir(&path).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains(".backup.")));
}
