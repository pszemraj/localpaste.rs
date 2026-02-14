//! Database integration tests.

use super::*;
use crate::error::AppError;
use crate::models::{folder::*, paste::*};
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::TempDir;

fn setup_test_db() -> (Database, TempDir) {
    let temp_dir = TempDir::new().expect("temp dir");
    let db_path = temp_dir.path().join("test.db");
    let db = Database::new(db_path.to_str().expect("db path")).expect("db");
    (db, temp_dir)
}

mod basic_ops;
mod concurrency;
mod folder_transactions;
mod search_and_meta;
mod startup_reconcile;
