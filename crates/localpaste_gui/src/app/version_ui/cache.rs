//! Snapshot and diff-preview caches for detached version dialogs.

use super::super::{highlight::hash_bytes, LocalPasteApp};
use super::VersionUiState;
use crate::app::ui::diff_modal::{
    inline_diff_preview_from_response, InlineDiffPreview, MAX_INLINE_DIFF_BYTES,
};
use crate::backend::CoreCmd;
use localpaste_core::diff::DiffResponse;

/// Identity of the active editor snapshot held by the version dialogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActiveSnapshotCacheKey {
    paste_id: String,
    buffer_epoch: u64,
    revision: u64,
    text_len: usize,
}

/// Identity of the loaded historical snapshot preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HistoryPreviewCacheKey {
    paste_id: String,
    version_id_ms: u64,
    len: usize,
}

/// Identity of the active left/right diff preview pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffPreviewCacheKey {
    lhs: ActiveSnapshotCacheKey,
    rhs_paste_id: String,
    rhs_content_len: usize,
    rhs_content_hash: u64,
}

impl VersionUiState {
    /// Clears the cached active editor snapshot and its line index.
    pub(super) fn clear_active_snapshot_cache(&mut self) {
        self.active_snapshot_cache_key = None;
        self.active_snapshot_cache_text.clear();
        self.active_snapshot_preview_lines.reset();
    }

    /// Clears the cached historical snapshot preview and its line index.
    pub(super) fn clear_history_preview_cache(&mut self) {
        self.history_preview_cache_key = None;
        self.history_preview_text.clear();
        self.history_preview_lines.reset();
    }

    /// Clears the cached diff preview and its pending worker request.
    pub(super) fn clear_diff_preview_cache(&mut self) {
        self.diff_preview_cache_key = None;
        self.diff_preview_pending_request_id = None;
        self.diff_preview = None;
    }

    fn next_diff_preview_request_id(&mut self) -> u64 {
        self.diff_preview_request_seq = self.diff_preview_request_seq.wrapping_add(1);
        self.diff_preview_request_seq
    }
}

impl LocalPasteApp {
    /// Advances the active-buffer identity token after a full buffer replacement.
    ///
    /// # Notes
    /// Editor revisions cover incremental edits, but selection changes and load
    /// acks can replace the entire buffer while resetting revision counters.
    pub(in crate::app) fn bump_active_buffer_epoch(&mut self) {
        self.active_buffer_epoch = self.active_buffer_epoch.wrapping_add(1);
        self.version_ui.clear_active_snapshot_cache();
    }

    fn active_snapshot_cache_key(&self) -> Option<ActiveSnapshotCacheKey> {
        Some(ActiveSnapshotCacheKey {
            paste_id: self.selected_id.clone()?,
            buffer_epoch: self.active_buffer_epoch,
            revision: self.active_revision(),
            text_len: self.active_text_len_bytes(),
        })
    }

    /// Refreshes the cached current editor snapshot only when the active buffer identity changes.
    ///
    /// # Returns
    /// `true` when a fresh owned snapshot had to be cloned from the editor buffer.
    pub(in crate::app) fn sync_active_snapshot_cache(&mut self) -> bool {
        let Some(cache_key) = self.active_snapshot_cache_key() else {
            self.version_ui.clear_active_snapshot_cache();
            return false;
        };
        if self.version_ui.active_snapshot_cache_key.as_ref() == Some(&cache_key) {
            return false;
        }
        self.version_ui.active_snapshot_cache_text = self.active_snapshot();
        self.version_ui.active_snapshot_preview_lines.rebuild(
            cache_key.revision,
            self.version_ui.active_snapshot_cache_text.as_str(),
        );
        self.version_ui.active_snapshot_cache_key = Some(cache_key);
        true
    }

    /// Refreshes the cached read-only history preview body for the selected stored snapshot.
    ///
    /// # Returns
    /// `true` when the snapshot body had to be cloned into preview storage.
    pub(in crate::app) fn sync_history_preview_cache(&mut self) -> bool {
        let Some(snapshot) = self.version_ui.history_snapshot.as_ref() else {
            self.version_ui.clear_history_preview_cache();
            return false;
        };
        let cache_key = HistoryPreviewCacheKey {
            paste_id: snapshot.paste_id.clone(),
            version_id_ms: snapshot.version_id_ms,
            len: snapshot.len,
        };
        if self.version_ui.history_preview_cache_key.as_ref() == Some(&cache_key) {
            return false;
        }
        self.version_ui.history_preview_text = snapshot.content.clone();
        self.version_ui.history_preview_lines.rebuild(
            snapshot.version_id_ms,
            self.version_ui.history_preview_text.as_str(),
        );
        self.version_ui.history_preview_cache_key = Some(cache_key);
        true
    }

    /// Refreshes detached diff preview state when the left or right side changes.
    ///
    /// At most one worker diff request stays in flight. While a request is pending,
    /// newer editor revisions wait for that result to drain and then enqueue the
    /// freshest preview on the next repaint.
    ///
    /// # Returns
    /// `true` when preview state changed or a fresh worker request was queued.
    pub(in crate::app) fn sync_diff_preview_cache(&mut self) -> bool {
        let Some(lhs_cache_key) = self.active_snapshot_cache_key() else {
            self.version_ui.clear_active_snapshot_cache();
            self.version_ui.clear_diff_preview_cache();
            return false;
        };
        let Some(cache_key) =
            self.version_ui
                .diff_target_paste
                .as_ref()
                .map(|rhs| DiffPreviewCacheKey {
                    lhs: lhs_cache_key,
                    rhs_paste_id: rhs.id.clone(),
                    rhs_content_len: rhs.content.len(),
                    // Diff output depends on the exact right-hand content, not
                    // on incidental metadata like timestamps.
                    rhs_content_hash: hash_bytes(rhs.content.as_bytes()),
                })
        else {
            self.version_ui.clear_diff_preview_cache();
            return false;
        };
        if self.version_ui.diff_preview_cache_key.as_ref() == Some(&cache_key) {
            return false;
        }

        let lhs_bytes = self.active_text_len_bytes();
        let rhs_bytes = cache_key.rhs_content_len;
        if lhs_bytes.saturating_add(rhs_bytes) > MAX_INLINE_DIFF_BYTES {
            self.version_ui.diff_preview = Some(InlineDiffPreview::TooLarge {
                lhs_bytes,
                rhs_bytes,
            });
            self.version_ui.diff_preview_cache_key = Some(cache_key);
            self.version_ui.diff_preview_pending_request_id = None;
            return true;
        }

        if self.version_ui.diff_preview_pending_request_id.is_some() {
            return false;
        }

        let _recomputed_snapshot = self.sync_active_snapshot_cache();
        let Some(right_text) = self
            .version_ui
            .diff_target_paste
            .as_ref()
            .map(|rhs| rhs.content.clone())
        else {
            self.version_ui.clear_diff_preview_cache();
            return false;
        };
        let request_id = self.version_ui.next_diff_preview_request_id();
        if !self.dispatch_backend_cmd(CoreCmd::ComputeDiffPreview {
            request_id,
            left_text: self.version_ui.active_snapshot_cache_text.clone(),
            right_text,
        }) {
            self.version_ui.clear_diff_preview_cache();
            self.set_status("Diff preview failed: backend unavailable.");
            return false;
        }
        self.version_ui.diff_preview_cache_key = Some(cache_key);
        self.version_ui.diff_preview_pending_request_id = Some(request_id);
        self.version_ui.diff_preview = None;
        true
    }

    /// Applies a completed detached diff preview response for the active request.
    ///
    /// Stale worker replies are ignored so background diff jobs cannot overwrite
    /// a newer modal request after the target or editor snapshot changed.
    ///
    /// # Arguments
    /// - `request_id`: Worker request id associated with the currently active modal preview.
    /// - `diff`: Worker-computed unified diff payload for that request.
    pub(in crate::app) fn apply_diff_preview_response(
        &mut self,
        request_id: u64,
        diff: DiffResponse,
    ) {
        if self.version_ui.diff_preview_pending_request_id != Some(request_id) {
            return;
        }
        self.version_ui.diff_preview_pending_request_id = None;
        self.version_ui.diff_preview = Some(inline_diff_preview_from_response(diff));
    }
}
