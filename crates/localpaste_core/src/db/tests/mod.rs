//! Database integration tests.

use super::*;
use crate::db::paste::set_reconcile_failpoint;
use crate::error::AppError;
use crate::models::{folder::*, paste::*};
use chrono::Duration;
use std::collections::HashMap;
use std::sync::{Arc, Barrier, Mutex, OnceLock};
use std::thread;
use tempfile::TempDir;

fn setup_test_db() -> (Database, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(db_path.to_str().unwrap()).unwrap();
    (db, temp_dir)
}

fn transaction_failpoint_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn reconcile_failpoint_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct FailpointGuard;

impl Drop for FailpointGuard {
    fn drop(&mut self) {
        set_transaction_failpoint(None);
        set_move_pause_hooks(None);
    }
}

struct ReconcileFailpointGuard;

impl Drop for ReconcileFailpointGuard {
    fn drop(&mut self) {
        set_reconcile_failpoint(false);
    }
}

fn assert_folder_counts_match_canonical(db: &Database) {
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

mod basic_ops;
mod concurrency;
mod folder_transactions;
mod search_and_meta;
mod startup_reconcile;
