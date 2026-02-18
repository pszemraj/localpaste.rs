//! Shared test-only helpers for localpaste_core.

use crate::Database;
use std::collections::HashMap;
use tempfile::TempDir;

/// Creates an isolated temporary database and returns it with the temp dir.
///
/// Keep the [`TempDir`] alive for the full test to preserve the backing files.
///
/// # Returns
/// A ready-to-use [`Database`] and its owning [`TempDir`].
///
/// # Panics
/// Panics if temp-dir creation, path conversion, or database initialization
/// fails in the test environment.
pub(crate) fn setup_temp_db() -> (Database, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(db_path.to_str().expect("db path")).expect("db");
    (db, temp_dir)
}

/// Asserts that folder `paste_count` values match canonical metadata rows.
///
/// This catches drift between denormalized folder counters and source-of-truth
/// paste metadata.
///
/// # Panics
/// Panics when metadata scanning fails or when folder counts are inconsistent.
pub(crate) fn assert_folder_counts_match_canonical(db: &Database) {
    let mut canonical_counts: HashMap<String, usize> = HashMap::new();
    db.pastes
        .scan_canonical_meta(|meta| {
            if let Some(folder_id) = meta.folder_id {
                *canonical_counts.entry(folder_id).or_insert(0) += 1;
            }
            Ok(())
        })
        .expect("scan canonical meta");

    for folder in db.folders.list().expect("list folders") {
        let expected = canonical_counts.remove(folder.id.as_str()).unwrap_or(0);
        assert_eq!(
            folder.paste_count, expected,
            "folder count drift for folder {}",
            folder.id
        );
    }
    assert!(
        canonical_counts.is_empty(),
        "canonical rows must not reference missing folders: {:?}",
        canonical_counts.keys().collect::<Vec<_>>()
    );
}
