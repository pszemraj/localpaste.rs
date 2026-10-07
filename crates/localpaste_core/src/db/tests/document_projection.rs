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
fn semantic_kinds_rebuild_from_version_eleven_and_survive_restart() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("db");
    let db = open_test_database(path.to_str().unwrap());
    let cases = [
        (
            "set timer for 10 minutes\r\n",
            "batch",
            PasteKind::Other,
            PasteKind::Document,
        ),
        (
            "source code is at https://example.com\r\n",
            "batch",
            PasteKind::Other,
            PasteKind::Document,
        ),
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
        (
            "info about the current project",
            "text",
            PasteKind::Log,
            PasteKind::Document,
        ),
        (
            "echo chamber is a common metaphor",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "Dear team,\nsudo is required only in some environments.",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "info () { printf hello; }",
            "shell",
            PasteKind::Log,
            PasteKind::Code,
        ),
        (
            "ERROR\tcount\tname\n404\t2\twidget",
            "tsv",
            PasteKind::Log,
            PasteKind::Other,
        ),
        (
            "```\nhello world this is a plain note\n```",
            "markdown",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "```text\nINFO Starting server\n```",
            "markdown",
            PasteKind::Document,
            PasteKind::Log,
        ),
        (
            "fn main() {}\n// thread 'main' panicked at src/main.rs:12:5",
            "rust",
            PasteKind::Log,
            PasteKind::Code,
        ),
        (
            "ERROR\tcount\tname",
            "tsv",
            PasteKind::Log,
            PasteKind::Other,
        ),
        (
            "```\nERROR\tcount\tname\n404\t2\twidget\n```",
            "markdown",
            PasteKind::Document,
            PasteKind::Other,
        ),
        (
            "make sure to save your work before leaving",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "git history helps explain this change",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "cd app\nnpm install\nnpm run dev",
            "text",
            PasteKind::Document,
            PasteKind::Code,
        ),
        (
            "just wanted to say thanks for your help",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "INFO (main) Starting the server",
            "text",
            PasteKind::Document,
            PasteKind::Log,
        ),
        (
            "info Resolving packages\nwarning Retrying request\nsuccess Saved lockfile",
            "text",
            PasteKind::Document,
            PasteKind::Log,
        ),
        (
            "INFO\tStarting worker\nERROR\tWorker stopped",
            "tsv",
            PasteKind::Other,
            PasteKind::Log,
        ),
        (
            "$ cargo run\nthread 'main' panicked at src/main.rs:12:5",
            "text",
            PasteKind::Document,
            PasteKind::Log,
        ),
        (
            "set timer for 10 minutes",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "set the table for 6 people before dinner",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "export markets are down 5% this quarter",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "source code is at https://example.com",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "set oven to 180C",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "set default timeout to 30s",
            "text",
            PasteKind::Code,
            PasteKind::Document,
        ),
        (
            "git rev-parse HEAD",
            "text",
            PasteKind::Document,
            PasteKind::Code,
        ),
        (
            "git blame README.md",
            "text",
            PasteKind::Document,
            PasteKind::Code,
        ),
        (
            "cd repo\r\ngit submodule update",
            "text",
            PasteKind::Document,
            PasteKind::Code,
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
            stale.derived.handle = Some("old projection handle".into());
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
            bincode::serialize(&11_u64).unwrap().as_slice(),
        )
        .unwrap();
    txn.commit().unwrap();
    drop(db);

    let assert_rebuilt = || {
        let reopened = open_test_database(path.to_str().unwrap());
        let metas = reopened.pastes.list_meta(100, None).unwrap();
        for (paste, (_, _, _, expected)) in pastes.iter().zip(&cases) {
            let rebuilt = metas.iter().find(|meta| meta.id == paste.id).unwrap();
            assert_eq!(
                rebuilt.derived.kind, *expected,
                "content: {}",
                paste.content
            );
            assert_eq!(rebuilt.derived, PasteMeta::from(paste).derived);
            let stored = reopened.pastes.get(&paste.id).unwrap().unwrap();
            assert_eq!(stored.content, paste.content);
            assert_eq!(stored.language, paste.language);
            assert_eq!(stored.language_is_manual, paste.language_is_manual);
        }
        drop(reopened);
    };

    assert_rebuilt();
    let backup_count = || {
        std::fs::read_dir(&path)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".backup.")
            })
            .count()
    };
    assert_eq!(backup_count(), 1, "v11 upgrade must create one backup");

    assert_rebuilt();
    assert_eq!(
        backup_count(),
        1,
        "current v12 restart must not create another backup"
    );
}

#[test]
fn auto_detected_shell_sequence_persists_as_code_projection() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("db");
    let db = open_test_database(path.to_str().unwrap());
    let paste = Paste::new(
        "# setup environment\nexport MODE=dev\nsource .env\nmkdir -p out\ncargo build --release\n"
            .into(),
        "setup".into(),
    );
    assert_eq!(paste.language.as_deref(), Some("shell"));
    assert!(paste.language_is_manual);
    db.pastes.create(&paste).unwrap();
    assert_eq!(
        db.pastes.list_meta(10, None).unwrap()[0].derived.kind,
        PasteKind::Code
    );
    drop(db);

    let reopened = open_test_database(path.to_str().unwrap());
    let stored = reopened.pastes.get(&paste.id).unwrap().unwrap();
    assert_eq!(stored.language.as_deref(), Some("shell"));
    assert!(stored.language_is_manual);
    assert_eq!(
        reopened.pastes.list_meta(10, None).unwrap()[0].derived.kind,
        PasteKind::Code
    );
}
