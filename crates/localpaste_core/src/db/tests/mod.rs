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

mod basic_ops;
mod concurrency;
mod folder_transactions;
mod search_and_meta;
mod startup_reconcile;
mod version_compat;
mod version_retention;
