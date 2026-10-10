//! Database integration tests.

use super::*;
use crate::db::paste::META_SCHEMA_VERSION_KEY;
use crate::db::tables::{
    DELETED_PASTES, DELETED_PASTE_VERSIONS_CONTENT, DELETED_PASTE_VERSIONS_META, PASTES_META,
    PASTES_META_STATE, REDB_FILE_NAME,
};
use crate::error::AppError;
use crate::models::{folder::*, paste::*};
pub(super) use crate::test_support::{
    open_test_database, open_test_database_result, setup_temp_db as setup_test_db,
    with_db_init_test_lock,
};
use redb::{ReadableDatabase, ReadableTable};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;

/// Install stale metadata rows and their schema marker atomically for repair tests.
///
/// # Arguments
/// - `db`: Database whose projection is overwritten.
/// - `metas`: Stale projection rows to install.
/// - `schema_version`: Historical projection schema version.
///
/// # Panics
/// Panics if encoding or the write transaction fails.
pub(super) fn install_stale_meta_projection(
    db: &Database,
    metas: impl IntoIterator<Item = PasteMeta>,
    schema_version: u64,
) {
    let txn = db.db.begin_write().expect("begin stale projection write");
    {
        let mut table = txn.open_table(PASTES_META).expect("open metas");
        for meta in metas {
            let encoded = bincode::serialize(&meta).expect("serialize stale meta");
            table
                .insert(meta.id.as_str(), encoded.as_slice())
                .expect("overwrite stale meta");
        }
        txn.open_table(PASTES_META_STATE)
            .expect("open meta state")
            .insert(
                META_SCHEMA_VERSION_KEY,
                bincode::serialize(&schema_version)
                    .expect("serialize old schema version")
                    .as_slice(),
            )
            .expect("stamp old schema version");
    }
    txn.commit().expect("commit stale projection");
}

/// List startup backups using the database backup filename contract.
///
/// # Arguments
/// - `db_path`: Database directory to inspect.
///
/// # Returns
/// Backup paths in sorted order.
///
/// # Panics
/// Panics if the database directory cannot be read.
pub(super) fn startup_backup_files(db_path: &Path) -> Vec<PathBuf> {
    let prefix = format!("{REDB_FILE_NAME}.backup.");

    let mut paths = std::fs::read_dir(db_path)
        .expect("read db dir")
        .map(|entry| entry.expect("backup entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".redb"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

/// Builds an update request with the common optional fields used by DB tests.
///
/// # Arguments
/// - `content`: Optional replacement content.
/// - `name`: Optional replacement name.
/// - `language`: Optional replacement language.
/// - `language_is_manual`: Optional manual-language flag.
///
/// # Returns
/// Paste update request with folder and tag changes unset.
pub(super) fn update_request(
    content: Option<&str>,
    name: Option<&str>,
    language: Option<&str>,
    language_is_manual: Option<bool>,
) -> UpdatePasteRequest {
    UpdatePasteRequest {
        content: content.map(ToString::to_string),
        name: name.map(ToString::to_string),
        language: language.map(ToString::to_string),
        language_is_manual,
        folder_id: None,
        tags: None,
    }
}

/// Updates an existing paste and unwraps the expected successful result.
///
/// # Arguments
/// - `db`: Database handle under test.
/// - `paste_id`: Paste id expected to exist.
/// - `request`: Update request to apply.
/// - `context`: Panic context for update failures.
///
/// # Returns
/// Updated paste row returned by the database.
///
/// # Panics
/// Panics when the update fails or the paste does not exist.
pub(super) fn update_existing_paste(
    db: &Database,
    paste_id: &str,
    request: UpdatePasteRequest,
    context: &str,
) -> Paste {
    db.pastes
        .update(paste_id, request)
        .expect(context)
        .expect("paste exists")
}

/// Create a paste, update it once, and return its id so staged-delete tests have version rows to preserve.
///
/// # Arguments
/// - `db`: Database handle under test.
/// - `name`: Paste name used for the seeded row.
///
/// # Returns
/// The id of the seeded paste.
///
/// # Panics
/// Panics when paste creation or update fails.
pub(super) fn seed_versioned_paste(db: &Database, name: &str) -> String {
    let paste = Paste::new("initial content".to_string(), name.to_string());
    let paste_id = paste.id.clone();
    db.pastes.create(&paste).expect("create paste");
    db.pastes
        .update(
            &paste_id,
            UpdatePasteRequest {
                content: Some("current content".to_string()),
                name: None,
                language: None,
                language_is_manual: None,
                folder_id: None,
                tags: None,
            },
        )
        .expect("update paste")
        .expect("paste exists");
    paste_id
}

/// Assert that a staged delete-undo token is either present or absent across every deleted-paste staging table.
///
/// # Arguments
/// - `db`: Database handle under test.
/// - `token`: Delete-undo token to inspect.
/// - `expected`: Whether matching staging rows should exist.
///
/// # Panics
/// Panics when the database read fails or the observed staging rows do not match `expected`.
pub(super) fn assert_staged_undo_token(db: &Database, token: &str, expected: bool) {
    let read_txn = db.db.begin_read().expect("begin read");
    let deleted_pastes = read_txn
        .open_table(DELETED_PASTES)
        .expect("open deleted pastes");
    let deleted_versions_meta = read_txn
        .open_table(DELETED_PASTE_VERSIONS_META)
        .expect("open deleted version meta");
    let deleted_versions_content = read_txn
        .open_table(DELETED_PASTE_VERSIONS_CONTENT)
        .expect("open deleted version content");

    assert_eq!(
        deleted_pastes
            .get(token)
            .expect("lookup deleted paste")
            .is_some(),
        expected
    );
    assert_eq!(
        deleted_versions_meta
            .get(token)
            .expect("lookup deleted version meta")
            .is_some(),
        expected
    );
    let has_content = deleted_versions_content
        .iter()
        .expect("iterate deleted version content")
        .any(|row| {
            let (key, _) = row.expect("deleted version content row");
            let (row_token, _) = key.value();
            row_token == token
        });
    assert_eq!(has_content, expected);
}

mod basic_ops;
mod concurrency;
mod delete_undo_lifecycle;
mod document_projection;
mod folder_transactions;
mod row_compat;
mod search_and_meta;
mod startup_reconcile;
mod version_compat;
mod version_retention;
