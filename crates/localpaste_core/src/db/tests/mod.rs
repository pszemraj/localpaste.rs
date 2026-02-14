//! Database integration tests.

use super::*;
use crate::error::AppError;
use crate::models::{folder::*, paste::*};
pub(super) use crate::test_support::setup_temp_db as setup_test_db;
use std::sync::{Arc, Barrier};
use std::thread;

mod basic_ops;
mod concurrency;
mod folder_transactions;
mod search_and_meta;
mod startup_reconcile;
