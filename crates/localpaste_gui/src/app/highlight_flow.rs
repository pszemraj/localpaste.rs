//! Highlight request, staging, and apply lifecycle for the editor.

use super::highlight::{
    HighlightPatch, HighlightRender, HighlightRequest, HighlightRequestMeta, HighlightRequestText,
};
use super::{
    LocalPasteApp, StagedHighlightInvalidation, HIGHLIGHT_APPLY_IDLE, HIGHLIGHT_DEBOUNCE_LARGE,
    HIGHLIGHT_DEBOUNCE_LARGE_BYTES, HIGHLIGHT_DEBOUNCE_MEDIUM, HIGHLIGHT_DEBOUNCE_TINY,
    HIGHLIGHT_PLAIN_THRESHOLD, HIGHLIGHT_TINY_EDIT_MAX_CHARS,
};
use std::ops::Range;
use std::time::{Duration, Instant};
use tracing::info;

enum HighlightGalleyInvalidation {
    None,
    LineRange(Range<usize>),
    LineRanges(Vec<Range<usize>>),
    All,
}

impl LocalPasteApp {
    fn insert_line_range(ranges: &mut Vec<Range<usize>>, mut incoming: Range<usize>) {
        if incoming.is_empty() {
            return;
        }
        let mut idx = 0usize;
        while idx < ranges.len() {
            let existing = &ranges[idx];
            if incoming.end < existing.start {
                break;
            }
            if incoming.start > existing.end {
                idx = idx.saturating_add(1);
                continue;
            }
            incoming = incoming.start.min(existing.start)..incoming.end.max(existing.end);
            ranges.remove(idx);
        }
        ranges.insert(idx, incoming);
    }

    fn merge_staged_invalidation_with_patch(
        existing: Option<StagedHighlightInvalidation>,
        patch_base_revision: u64,
        patch_base_text_len: usize,
        patch_line_range: Range<usize>,
    ) -> StagedHighlightInvalidation {
        if let Some(existing) = existing {
            let mut ranges = existing.line_ranges;
            Self::insert_line_range(&mut ranges, patch_line_range);
            StagedHighlightInvalidation {
                base_revision: existing.base_revision,
                base_text_len: existing.base_text_len,
                line_ranges: ranges,
            }
        } else {
            let mut ranges = Vec::new();
            Self::insert_line_range(&mut ranges, patch_line_range);
            StagedHighlightInvalidation {
                base_revision: patch_base_revision,
                base_text_len: patch_base_text_len,
                line_ranges: ranges,
            }
        }
    }

    fn highlight_galley_invalidation_for_apply(
        previous: Option<&HighlightRender>,
        next: &HighlightRender,
        staged_invalidation: Option<&StagedHighlightInvalidation>,
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
        if let Some(staged_invalidation) = staged_invalidation {
            if staged_invalidation.base_revision == previous.revision
                && staged_invalidation.base_text_len == previous.text_len
            {
                let mut clipped = Vec::new();
                for range in &staged_invalidation.line_ranges {
                    let start = range.start.min(next.lines.len());
                    let end = range.end.min(next.lines.len());
                    if start < end {
                        clipped.push(start..end);
                    }
                }
                return match clipped.len() {
                    0 => HighlightGalleyInvalidation::None,
                    1 => HighlightGalleyInvalidation::LineRange(
                        clipped
                            .into_iter()
                            .next()
                            .expect("single clipped range exists"),
                    ),
                    _ => HighlightGalleyInvalidation::LineRanges(clipped),
                };
            }
        }
        if next.base_revision == Some(previous.revision)
            && next.base_text_len == Some(previous.text_len)
        {
            if let Some(range) = next.changed_line_range.as_ref() {
                let start = range.start.min(next.lines.len());
                let end = range.end.min(next.lines.len());
                if start < end {
                    return HighlightGalleyInvalidation::LineRange(start..end);
                }
                return HighlightGalleyInvalidation::None;
            }
        }
        HighlightGalleyInvalidation::All
    }

    fn apply_highlight_galley_invalidation(&mut self, invalidation: HighlightGalleyInvalidation) {
        match invalidation {
            HighlightGalleyInvalidation::None => {}
            HighlightGalleyInvalidation::LineRange(range) => {
                self.virtual_galley_cache.evict_line_range(range);
            }
            HighlightGalleyInvalidation::LineRanges(ranges) => {
                for range in ranges {
                    self.virtual_galley_cache.evict_line_range(range);
                }
            }
            HighlightGalleyInvalidation::All => {
                self.virtual_galley_cache.evict_all();
            }
        }
    }

    fn staged_matches_active_snapshot(
        &self,
        staged_paste_id: &str,
        staged_revision: u64,
        staged_text_len: usize,
    ) -> bool {
        let Some(selected_id) = self.selected_id.as_deref() else {
            return false;
        };
        staged_paste_id == selected_id
            && staged_revision == self.active_revision()
            && staged_text_len == self.active_text_len_bytes()
    }

    /// Emits an eager highlight trace event when tracing is enabled.
    ///
    /// # Arguments
    /// - `event`: Short event label.
    /// - `details`: Preformatted event detail string.
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

    /// Emits a lazily formatted highlight trace event.
    ///
    /// # Arguments
    /// - `event`: Short event label.
    /// - `details`: Lazy formatter invoked only when tracing is enabled.
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

    /// Clears pending/staged/current highlight state and invalidates galley cache.
    pub(super) fn clear_highlight_state(&mut self) {
        self.highlight_pending = None;
        self.highlight_render = None;
        self.highlight_staged = None;
        self.highlight_staged_invalidation = None;
        self.highlight_edit_hint = None;
        self.virtual_galley_cache.evict_all();
        self.highlight_version = self.highlight_version.wrapping_add(1);
        self.trace_highlight("clear", "cleared pending/render/staged");
    }

    fn queue_highlight_render_with_invalidation(
        &mut self,
        render: HighlightRender,
        staged_invalidation: Option<StagedHighlightInvalidation>,
    ) {
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
        self.highlight_staged_invalidation = staged_invalidation;
    }

    /// Queues a full render result from the highlight worker.
    pub(super) fn queue_highlight_render(&mut self, render: HighlightRender) {
        self.queue_highlight_render_with_invalidation(render, None);
    }

    /// Queues and merges a highlight line-range patch from the worker.
    ///
    /// # Panics
    /// Panics if staged-patch merge invariants fail after prior guard checks.
    pub(super) fn queue_highlight_patch(&mut self, patch: HighlightPatch) {
        let Some(selected_id) = self.selected_id.as_deref() else {
            self.trace_highlight("drop", "patch ignored: no selected paste");
            return;
        };
        if patch.paste_id != selected_id {
            self.trace_highlight("drop", "patch ignored: paste id mismatch");
            return;
        }
        let staged_matches_base = self
            .highlight_staged
            .as_ref()
            .map(|render| {
                render.matches_context(
                    patch.paste_id.as_str(),
                    patch.language_hint.as_str(),
                    patch.theme_key.as_str(),
                ) && render.revision == patch.base_revision
                    && render.text_len == patch.base_text_len
            })
            .unwrap_or(false);
        if staged_matches_base {
            // Fast path: patch can be applied directly to staged render without cloning all lines.
            let range = patch.line_range.clone();
            {
                let staged = self
                    .highlight_staged
                    .as_mut()
                    .expect("staged render should exist");
                if staged.revision > patch.revision
                    || (staged.revision == patch.revision && staged.text_len >= patch.text_len)
                {
                    self.trace_highlight(
                        "drop",
                        "patch ignored: stale/duplicate against current staged base",
                    );
                    return;
                }
                if staged.lines.len() != patch.total_lines {
                    self.trace_highlight(
                        "drop",
                        "patch ignored: line-count mismatch; waiting for full render",
                    );
                    return;
                }
                if range.end > patch.total_lines || range.start > range.end {
                    self.trace_highlight("drop", "patch ignored: invalid line range");
                    return;
                }
                if patch.lines.len() != range.len() {
                    self.trace_highlight(
                        "drop",
                        "patch ignored: range and patch line count mismatch",
                    );
                    return;
                }
                let hint_range = range.clone();
                staged.lines.splice(range, patch.lines);
                staged.revision = patch.revision;
                staged.text_len = patch.text_len;
                staged.base_revision = Some(patch.base_revision);
                staged.base_text_len = Some(patch.base_text_len);
                staged.language_hint = patch.language_hint;
                staged.theme_key = patch.theme_key;
                staged.changed_line_range = match staged.changed_line_range.take() {
                    Some(existing) => {
                        Some(existing.start.min(hint_range.start)..existing.end.max(hint_range.end))
                    }
                    None => Some(hint_range.clone()),
                };
                self.highlight_staged_invalidation =
                    Some(Self::merge_staged_invalidation_with_patch(
                        self.highlight_staged_invalidation.take(),
                        patch.base_revision,
                        patch.base_text_len,
                        hint_range,
                    ));
            }
            if let Some(staged) = self.highlight_staged.as_ref() {
                if let Some(pending) = &self.highlight_pending {
                    if pending.matches(
                        staged.revision,
                        staged.text_len,
                        staged.language_hint.as_str(),
                        staged.theme_key.as_str(),
                        staged.paste_id.as_str(),
                    ) {
                        self.highlight_pending = None;
                        self.trace_highlight(
                            "pending_clear",
                            "pending request matched staged patch",
                        );
                    }
                }
                self.trace_highlight_lazy("queue", || {
                    format!(
                        "merged patch into staged render revision={} text_len={}",
                        staged.revision, staged.text_len
                    )
                });
            }
            return;
        }
        enum PatchBaseSource {
            Staged,
            Current,
        }
        let staged_base = self.highlight_staged.as_ref().filter(|render| {
            render.matches_context(
                patch.paste_id.as_str(),
                patch.language_hint.as_str(),
                patch.theme_key.as_str(),
            ) && render.revision == patch.base_revision
                && render.text_len == patch.base_text_len
        });
        let current_base = self.highlight_render.as_ref().filter(|render| {
            render.matches_context(
                patch.paste_id.as_str(),
                patch.language_hint.as_str(),
                patch.theme_key.as_str(),
            ) && render.revision == patch.base_revision
                && render.text_len == patch.base_text_len
        });
        let Some((base, base_source)) = staged_base
            .map(|render| (render.clone(), PatchBaseSource::Staged))
            .or_else(|| current_base.map(|render| (render.clone(), PatchBaseSource::Current)))
        else {
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

        let mut merged_lines = base.lines;
        let hint_range = range.clone();
        merged_lines.splice(range, patch.lines);
        let staged_invalidation = match base_source {
            PatchBaseSource::Staged => Some(Self::merge_staged_invalidation_with_patch(
                self.highlight_staged_invalidation.clone(),
                patch.base_revision,
                patch.base_text_len,
                hint_range.clone(),
            )),
            PatchBaseSource::Current => Some(StagedHighlightInvalidation {
                base_revision: patch.base_revision,
                base_text_len: patch.base_text_len,
                line_ranges: vec![hint_range.clone()],
            }),
        };
        self.queue_highlight_render_with_invalidation(
            HighlightRender {
                paste_id: patch.paste_id,
                revision: patch.revision,
                text_len: patch.text_len,
                base_revision: Some(patch.base_revision),
                base_text_len: Some(patch.base_text_len),
                language_hint: patch.language_hint,
                theme_key: patch.theme_key,
                changed_line_range: Some(hint_range.clone()),
                lines: merged_lines,
            },
            staged_invalidation,
        );
    }

    /// Applies currently staged highlight render into active render state.
    pub(super) fn apply_staged_highlight(&mut self) {
        let Some(render) = self.highlight_staged.take() else {
            return;
        };
        let staged_invalidation = self.highlight_staged_invalidation.take();
        let invalidation = Self::highlight_galley_invalidation_for_apply(
            self.highlight_render.as_ref(),
            &render,
            staged_invalidation.as_ref(),
        );
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

    /// Applies staged highlight when it still matches active text and app is idle.
    pub(super) fn maybe_apply_staged_highlight(&mut self, now: Instant) {
        let Some((
            staged_paste_id,
            staged_revision,
            staged_text_len,
            staged_language_hint,
            staged_theme_key,
        )) = self.highlight_staged.as_ref().map(|staged| {
            (
                staged.paste_id.as_str(),
                staged.revision,
                staged.text_len,
                staged.language_hint.as_str(),
                staged.theme_key.as_str(),
            )
        })
        else {
            return;
        };
        if !self.staged_matches_active_snapshot(staged_paste_id, staged_revision, staged_text_len) {
            let active_revision = self.active_revision();
            let active_text_len = self.active_text_len_bytes();
            self.trace_highlight_lazy("drop_stale_staged", || {
                format!(
                    "staged rev={} len={} active rev={} len={}",
                    staged_revision, staged_text_len, active_revision, active_text_len
                )
            });
            self.highlight_staged = None;
            self.highlight_staged_invalidation = None;
            return;
        }
        if let Some(current) = &self.highlight_render {
            if current.matches_context(staged_paste_id, staged_language_hint, staged_theme_key)
                && (current.revision > staged_revision
                    || (current.revision == staged_revision && current.text_len >= staged_text_len))
            {
                self.trace_highlight("drop", "staged render superseded by current render");
                self.highlight_staged = None;
                self.highlight_staged_invalidation = None;
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

    /// Returns whether a new highlight request should be dispatched.
    ///
    /// # Arguments
    /// - `language_hint`: Canonical language hint for active content.
    /// - `theme_key`: Syntect theme key.
    /// - `debounce_active`: Whether debounce window is currently active.
    /// - `paste_id`: Active paste id.
    ///
    /// # Returns
    /// `true` when no equivalent pending/current/staged render already exists.
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

    /// Computes highlight request debounce window for current buffer state.
    ///
    /// # Arguments
    /// - `text_len`: Active text length in bytes.
    /// - `async_mode`: Whether async highlighting is active.
    ///
    /// # Returns
    /// Debounce duration to apply before dispatching worker requests.
    pub(super) fn highlight_debounce_window(&self, text_len: usize, async_mode: bool) -> Duration {
        if !async_mode {
            return Duration::ZERO;
        }
        let size_window = if text_len >= HIGHLIGHT_DEBOUNCE_LARGE_BYTES {
            HIGHLIGHT_DEBOUNCE_LARGE
        } else {
            HIGHLIGHT_DEBOUNCE_MEDIUM
        };
        if let Some(edit_hint) = self.highlight_edit_hint {
            let changed_chars = edit_hint
                .inserted_chars
                .saturating_add(edit_hint.deleted_chars);
            // Tiny-edit fast debounce is only safe on sub-large buffers; large
            // buffers should keep the larger window to avoid per-keystroke
            // snapshot churn on the UI thread.
            if changed_chars <= HIGHLIGHT_TINY_EDIT_MAX_CHARS
                && edit_hint.touched_lines <= 2
                && text_len < HIGHLIGHT_DEBOUNCE_LARGE_BYTES
            {
                return HIGHLIGHT_DEBOUNCE_TINY;
            }
        }
        size_window
    }

    /// Dispatches a highlight request to the worker and tracks pending metadata.
    ///
    /// # Arguments
    /// - `revision`: Active text revision.
    /// - `text`: Request text payload.
    /// - `language_hint`: Canonical language hint.
    /// - `theme_key`: Syntect theme key.
    /// - `paste_id`: Active paste id.
    pub(super) fn dispatch_highlight_request(
        &mut self,
        revision: u64,
        text: HighlightRequestText,
        language_hint: &str,
        theme_key: &str,
        paste_id: &str,
    ) {
        let text_len = text.len_bytes();
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
