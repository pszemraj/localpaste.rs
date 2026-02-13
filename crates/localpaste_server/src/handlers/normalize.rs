//! Shared normalization helpers for optional string request fields.

use crate::AppError;
use localpaste_core::{folder_ops::ensure_folder_assignable, Database};

/// Normalize optional identifiers for create/list/search semantics.
///
/// Empty or whitespace-only values are treated as absent.
pub(super) fn normalize_optional_for_create(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Normalize optional identifiers for update semantics.
///
/// Empty or whitespace-only values are preserved as explicit clear markers.
pub(super) fn normalize_optional_for_update(value: Option<String>) -> Option<String> {
    value.map(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            String::new()
        } else {
            trimmed.to_string()
        }
    })
}

/// Validate a request-supplied folder id using shared core assignability rules.
///
/// Missing folders are mapped to a `BadRequest` with caller-provided label text.
pub(super) fn validate_assignable_folder_for_request(
    db: &Database,
    folder_id: &str,
    label: &str,
) -> Result<(), AppError> {
    match ensure_folder_assignable(db, folder_id) {
        Ok(()) => Ok(()),
        Err(AppError::NotFound) => Err(AppError::BadRequest(format!(
            "{} with id '{}' does not exist",
            label, folder_id
        ))),
        Err(err) => Err(err),
    }
}
