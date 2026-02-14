//! Shared database time conversion helpers.

use crate::error::AppError;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn unix_timestamp_seconds(now: SystemTime) -> Result<u64, AppError> {
    now.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|err| {
            AppError::DatabaseError(format!(
                "Failed to compute backup timestamp from system clock: {}",
                err
            ))
        })
}
