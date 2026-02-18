//! Shared database time conversion helpers.

use crate::error::AppError;
use std::time::{SystemTime, UNIX_EPOCH};

/// Converts a wall-clock timestamp into seconds since the Unix epoch.
///
/// Returns an [`AppError`] when the provided time is before `UNIX_EPOCH`.
///
/// # Returns
/// Whole seconds elapsed since `UNIX_EPOCH`.
///
/// # Errors
/// Returns [`AppError::StorageMessage`] when `now` is earlier than epoch.
pub(super) fn unix_timestamp_seconds(now: SystemTime) -> Result<u64, AppError> {
    now.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|err| {
            AppError::StorageMessage(format!(
                "Failed to compute backup timestamp from system clock: {}",
                err
            ))
        })
}
