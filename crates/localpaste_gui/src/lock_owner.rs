//! Shared lock-owner id generation for GUI runtime components.

use localpaste_server::LockOwnerId;
use std::sync::atomic::{AtomicU64, Ordering};

/// Generate a process-unique owner id for in-process lock participants.
///
/// # Arguments
/// - `prefix`: Stable component prefix such as `gui` or `gui-backend-worker`.
///
/// # Returns
/// A [`LockOwnerId`] that is unique within this process lifetime.
pub(crate) fn next_lock_owner_id(prefix: &str) -> LockOwnerId {
    static NEXT_OWNER_SEQ: AtomicU64 = AtomicU64::new(1);
    let seq = NEXT_OWNER_SEQ.fetch_add(1, Ordering::Relaxed);
    LockOwnerId::new(format!("{}-{}-{}", prefix, std::process::id(), seq))
}
