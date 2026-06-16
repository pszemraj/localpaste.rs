//! Helpers for workflows that queue an action behind content/metadata saves.

use super::{LocalPasteApp, SaveStatus};
use std::time::Instant;

/// Rolls back local in-flight flags after a multi-save deferred workflow fails.
///
/// # Arguments
/// - `app`: App state containing the save flags to restore.
/// - `content_save_needed`: Whether the workflow attempted to dispatch a content save.
/// - `metadata_save_needed`: Whether the workflow attempted to dispatch a metadata save.
pub(super) fn rollback_deferred_save_dispatches(
    app: &mut LocalPasteApp,
    content_save_needed: bool,
    metadata_save_needed: bool,
) {
    if content_save_needed && app.save_in_flight {
        app.save_in_flight = false;
        app.save_status = SaveStatus::Dirty;
        app.save_request_revision = None;
        if app.last_edit_at.is_none() {
            app.last_edit_at = Some(Instant::now());
        }
    }
    if metadata_save_needed && app.metadata_save_in_flight {
        app.metadata_save_in_flight = false;
        app.metadata_dirty = true;
        app.metadata_save_request = None;
    }
}
