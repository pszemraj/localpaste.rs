//! Database integration tests.

use super::*;
use crate::error::AppError;
use crate::models::{folder::*, paste::*};
pub(super) use crate::test_support::{
    open_test_database, open_test_database_result, setup_temp_db as setup_test_db,
    with_db_init_test_lock,
};
use std::sync::{Arc, Barrier};
use std::thread;

mod basic_ops;
mod concurrency;
mod folder_transactions;
mod search_and_meta;
mod startup_reconcile;
