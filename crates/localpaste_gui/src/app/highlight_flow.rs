//! Highlight request, staging, and apply lifecycle for the editor.

use super::highlight::{HighlightPatch, HighlightRender, HighlightRequest, HighlightRequestMeta};
use super::{
    LocalPasteApp, HIGHLIGHT_APPLY_IDLE, HIGHLIGHT_DEBOUNCE_LARGE, HIGHLIGHT_DEBOUNCE_LARGE_BYTES,
    HIGHLIGHT_DEBOUNCE_MEDIUM, HIGHLIGHT_DEBOUNCE_TINY, HIGHLIGHT_PLAIN_THRESHOLD,
    HIGHLIGHT_TINY_EDIT_MAX_CHARS,
};
use std::ops::Range;
use std::time::{Duration, Instant};
use tracing::info;

enum HighlightGalleyInvalidation {
    None,
    LineRange(Range<usize>),
    All,
}

impl LocalPasteApp {
    fn highlight_galley_invalidation_for_apply(
        previous: Option<&HighlightRender>,
        next: &HighlightRender,
    ) -> HighlightGalleyInvalidation {
        let Some(previous) = previous else {
            return HighlightGalleyInvalidation::All;
        };
        if !previous.matches_context(
            next.paste_id.as_str(),
            next.language_hint.as_str(),
            next.theme_key.as_str(),
        ) || previous.lines.len() != next.lines.len()
        {
            return HighlightGalleyInvalidation::All;
        }

        let mut first_changed: Option<usize> = None;
        let mut last_changed: Option<usize> = None;
        for idx in 0..next.lines.len() {
            if previous.lines.get(idx) != next.lines.get(idx) {
                first_changed.get_or_insert(idx);
                last_changed = Some(idx);
            }
        }

        match (first_changed, last_changed) {
            (Some(start), Some(end)) => HighlightGalleyInvalidation::LineRange(start..(end + 1)),
            _ => HighlightGalleyInvalidation::None,
        }
    }

    fn apply_highlight_galley_invalidation(&mut self, invalidation: HighlightGalleyInvalidation) {
        match invalidation {
            HighlightGalleyInvalidation::None => {}
            HighlightGalleyInvalidation::LineRange(range) => {
                self.virtual_galley_cache.evict_line_range(range);
            }
            HighlightGalleyInvalidation::All => {
                self.virtual_galley_cache.evict_all();
            }
        }
    }

    fn staged_matches_active_snapshot(&mut self, staged: &HighlightRender) -> bool {
        let Some(selected_id) = self.selected_id.as_deref() else {
            return false;
        };
        staged.paste_id == selected_id
            && staged.revision == self.active_revision()
            && staged.text_len == self.active_text_len_bytes()
    }

    pub(super) fn trace_highlight(&self, event: &str, details: &str) {
        if !self.highlight_trace_enabled {
            return;
        }
        info!(
            target: "localpaste_gui::highlight",
            event = event,
            details = details,
            "highlight trace"
        );
    }

    pub(super) fn trace_highlight_lazy<F>(&self, event: &str, details: F)
    where
        F: FnOnce() -> String,
    {
        if !self.highlight_trace_enabled {
            return;
        }
        let details = details();
        info!(
            target: "localpaste_gui::highlight",
            event = event,
            details = details.as_str(),
            "highlight trace"
        );
    }

    pub(super) fn clear_highlight_state(&mut self) {
        self.highlight_pending = None;
        self.highlight_render = None;
        self.highlight_staged = None;
        self.highlight_edit_hint = None;
        self.virtual_galley_cache.evict_all();
        self.highlight_version = self.highlight_version.wrapping_add(1);
        self.trace_highlight("clear", "cleared pending/render/staged");
    }

    pub(super) fn queue_highlight_render(&mut self, render: HighlightRender) {
        let Some(selected_id) = self.selected_id.as_deref() else {
            self.trace_highlight("drop", "render ignored: no selected paste");
            return;
        };
        if render.paste_id != selected_id {
            self.trace_highlight("drop", "render ignored: paste id mismatch");
            return;
        }
        let active_revision = self.active_revision();
        if let Some(current) = &self.highlight_render {
            if current.matches_context(
                render.paste_id.as_str(),
                render.language_hint.as_str(),
                render.theme_key.as_str(),
            ) && (current.revision > render.revision
                || (current.revision == render.revision && current.text_len >= render.text_len))
            {
                self.trace_highlight(
                    "drop",
                    "render ignored: older/equal than current highlighted revision",
                );
                return;
            }
        }
        if render.revision < active_revision && self.highlight_render.is_some() {
            self.trace_highlight(
                "drop",
                "render ignored: revision older than active text and stale render exists",
            );
            return;
        }
        if let Some(staged) = &self.highlight_staged {
            if staged.matches_context(
                render.paste_id.as_str(),
                render.language_hint.as_str(),
                render.theme_key.as_str(),
            ) && (staged.revision > render.revision
                || (staged.revision == render.revision && staged.text_len >= render.text_len))
            {
                self.trace_highlight("drop", "render ignored: older/equal than staged revision");
                return;
            }
        }
        if let Some(pending) = &self.highlight_pending {
            if pending.matches_render(&render) {
                self.highlight_pending = None;
                self.trace_highlight("pending_clear", "pending request matched worker render");
            }
        }
        self.trace_highlight_lazy("queue", || {
            format!(
                "queued staged render revision={} text_len={}",
                render.revision, render.text_len
            )
        });
        self.highlight_staged = Some(render);
    }

    pub(super) fn queue_highlight_patch(&mut self, patch: HighlightPatch) {
        let Some(selected_id) = self.selected_id.as_deref() else {
            self.trace_highlight("drop", "patch ignored: no selected paste");
            return;
        };
        if patch.paste_id != selected_id {
            self.trace_highlight("drop", "patch ignored: paste id mismatch");
            return;
        }
        let Some(base) = [
            self.highlight_staged.as_ref(),
            self.highlight_render.as_ref(),
        ]
        .into_iter()
        .flatten()
        .find(|render| {
            render.matches_context(
                patch.paste_id.as_str(),
                patch.language_hint.as_str(),
                patch.theme_key.as_str(),
            ) && render.revision == patch.base_revision
                && render.text_len == patch.base_text_len
        })
        .cloned() else {
            self.trace_highlight("drop", "patch ignored: no matching active render to merge");
            return;
        };
        if base.revision > patch.revision
            || (base.revision == patch.revision && base.text_len >= patch.text_len)
        {
            self.trace_highlight(
                "drop",
                "patch ignored: stale/duplicate against current base",
            );
            return;
        }
        if base.lines.len() != patch.total_lines {
            self.trace_highlight(
                "drop",
                "patch ignored: line-count mismatch; waiting for full render",
            );
            return;
        }
        let range = patch.line_range.clone();
        if range.end > patch.total_lines || range.start > range.end {
            self.trace_highlight("drop", "patch ignored: invalid line range");
            return;
        }
        if patch.lines.len() != range.len() {
            self.trace_highlight("drop", "patch ignored: range and patch line count mismatch");
            return;
        }

        let mut merged_lines = base.lines.clone();
        merged_lines.splice(range, patch.lines);
        self.queue_highlight_render(HighlightRender {
            paste_id: patch.paste_id,
            revision: patch.revision,
            text_len: patch.text_len,
            language_hint: patch.language_hint,
            theme_key: patch.theme_key,
            lines: merged_lines,
        });
    }

    pub(super) fn apply_staged_highlight(&mut self) {
        let Some(render) = self.highlight_staged.take() else {
            return;
        };
        let invalidation =
            Self::highlight_galley_invalidation_for_apply(self.highlight_render.as_ref(), &render);
        self.trace_highlight_lazy("apply", || {
            format!(
                "applied staged render revision={} text_len={}",
                render.revision, render.text_len
            )
        });
        self.apply_highlight_galley_invalidation(invalidation);
        self.highlight_render = Some(render);
        self.highlight_version = self.highlight_version.wrapping_add(1);
    }

    pub(super) fn maybe_apply_staged_highlight(&mut self, now: Instant) {
        let Some(staged) = self.highlight_staged.as_ref().cloned() else {
            return;
        };
        if !self.staged_matches_active_snapshot(&staged) {
            let active_revision = self.active_revision();
            let active_text_len = self.active_text_len_bytes();
            self.trace_highlight_lazy("drop_stale_staged", || {
                format!(
                    "staged rev={} len={} active rev={} len={}",
                    staged.revision, staged.text_len, active_revision, active_text_len
                )
            });
            self.highlight_staged = None;
            return;
        }
        if let Some(current) = &self.highlight_render {
            if current.matches_context(
                staged.paste_id.as_str(),
                staged.language_hint.as_str(),
                staged.theme_key.as_str(),
            ) && (current.revision > staged.revision
                || (current.revision == staged.revision && current.text_len >= staged.text_len))
            {
                self.trace_highlight("drop", "staged render superseded by current render");
                self.highlight_staged = None;
                return;
            }
        }
        if self.highlight_render.is_none() {
            self.trace_highlight("apply_now", "no current render; apply staged immediately");
            self.apply_staged_highlight();
            return;
        }
        let idle = self
            .last_interaction_at
            .map(|last| now.duration_since(last) >= HIGHLIGHT_APPLY_IDLE)
            .unwrap_or(true);
        if idle {
            self.trace_highlight("apply_idle", "applying staged render after idle");
            self.apply_staged_highlight();
        } else {
            self.trace_highlight("hold", "staged render waiting for idle threshold");
        }
    }

    pub(super) fn should_request_highlight(
        &self,
        language_hint: &str,
        theme_key: &str,
        debounce_active: bool,
        paste_id: &str,
    ) -> bool {
        let revision = self.active_revision();
        let text_len = self.active_text_len_bytes();
        if text_len >= HIGHLIGHT_PLAIN_THRESHOLD {
            self.trace_highlight("skip_request", "plain threshold exceeded");
            return false;
        }
        if let Some(pending) = &self.highlight_pending {
            if pending.matches(revision, text_len, language_hint, theme_key, paste_id) {
                self.trace_highlight("skip_request", "matching highlight request already pending");
                return false;
            }
        }
        if let Some(render) = &self.highlight_render {
            if render.matches_exact(revision, text_len, language_hint, theme_key, paste_id) {
                self.trace_highlight("skip_request", "exact render already available");
                return false;
            }
        }
        if let Some(render) = &self.highlight_staged {
            if render.matches_exact(revision, text_len, language_hint, theme_key, paste_id) {
                self.trace_highlight("skip_request", "exact render already staged");
                return false;
            }
        }
        if debounce_active && (self.highlight_pending.is_some() || self.highlight_render.is_some())
        {
            self.trace_highlight(
                "skip_request",
                "debounce active with pending/current render",
            );
            return false;
        }
        true
    }

    pub(super) fn highlight_debounce_window(&self, text_len: usize, async_mode: bool) -> Duration {
        if !async_mode {
            return Duration::ZERO;
        }
        if let Some(edit_hint) = self.highlight_edit_hint {
            let changed_chars = edit_hint
                .inserted_chars
                .saturating_add(edit_hint.deleted_chars);
            if changed_chars <= HIGHLIGHT_TINY_EDIT_MAX_CHARS && edit_hint.touched_lines <= 2 {
                return HIGHLIGHT_DEBOUNCE_TINY;
            }
        }
        if text_len >= HIGHLIGHT_DEBOUNCE_LARGE_BYTES {
            HIGHLIGHT_DEBOUNCE_LARGE
        } else {
            HIGHLIGHT_DEBOUNCE_MEDIUM
        }
    }

    pub(super) fn dispatch_highlight_request(
        &mut self,
        revision: u64,
        text: String,
        language_hint: &str,
        theme_key: &str,
        paste_id: &str,
    ) {
        let text_len = text.len();
        let edit_hint = if self.is_virtual_editor_mode() {
            self.highlight_edit_hint.take()
        } else {
            None
        };
        let patch_base = [
            self.highlight_staged.as_ref(),
            self.highlight_render.as_ref(),
        ]
        .into_iter()
        .flatten()
        .filter(|render| render.matches_context(paste_id, language_hint, theme_key))
        .max_by(|left, right| {
            left.revision
                .cmp(&right.revision)
                .then_with(|| left.text_len.cmp(&right.text_len))
        })
        .map(|render| (render.revision, render.text_len));
        self.trace_highlight_lazy("request", || {
            format!(
                "dispatch revision={} text_len={} lang={} theme={} paste={}",
                revision, text_len, language_hint, theme_key, paste_id
            )
        });
        let request = HighlightRequest {
            paste_id: paste_id.to_string(),
            revision,
            text,
            language_hint: language_hint.to_string(),
            theme_key: theme_key.to_string(),
            edit_hint,
            patch_base_revision: patch_base.map(|(base_revision, _)| base_revision),
            patch_base_text_len: patch_base.map(|(_, base_text_len)| base_text_len),
        };
        self.highlight_pending = Some(HighlightRequestMeta {
            paste_id: paste_id.to_string(),
            revision,
            text_len,
            language_hint: language_hint.to_string(),
            theme_key: theme_key.to_string(),
        });
        let _ = self.highlight_worker.tx.send(request);
    }
}
