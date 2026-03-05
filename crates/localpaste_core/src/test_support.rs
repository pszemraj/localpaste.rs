//! Shared test-only helpers for localpaste_core.

use crate::error::AppError;
use crate::Database;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;

fn global_db_init_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Run a closure while serializing env-sensitive database startup in tests.
///
/// # Panics
/// Panics if the shared startup lock is poisoned.
///
/// # Returns
/// The closure result.
pub(crate) fn with_db_init_test_lock<T>(f: impl FnOnce() -> T) -> T {
    let _lock = global_db_init_test_lock()
        .lock()
        .expect("db init test lock");
    f()
}

/// Open a test database while serializing env-sensitive startup config reads.
///
/// # Arguments
/// - `path`: Database directory path.
///
/// # Errors
/// Propagates any database initialization error returned by [`Database::new`].
///
/// # Returns
/// The result of [`Database::new`] for `path`.
pub(crate) fn open_test_database_result(path: &str) -> Result<Database, AppError> {
    with_db_init_test_lock(|| Database::new(path))
}

/// Open a test database while serializing env-sensitive startup config reads.
///
/// # Arguments
/// - `path`: Database directory path.
///
/// # Returns
/// A ready-to-use [`Database`].
///
/// # Panics
/// Panics if database initialization fails in the test environment.
pub(crate) fn open_test_database(path: &str) -> Database {
    open_test_database_result(path).expect("db")
}

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
    let db = open_test_database(db_path.to_str().expect("db path"));
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
