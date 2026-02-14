//! Highlight request, staging, and apply lifecycle for the editor.

use super::highlight::{
    hash_bytes, hash_text_chunks, HighlightRender, HighlightRequest, HighlightRequestMeta,
};
use super::{LocalPasteApp, HIGHLIGHT_APPLY_IDLE, HIGHLIGHT_PLAIN_THRESHOLD};
use std::time::Instant;
use tracing::info;

impl LocalPasteApp {
    fn staged_matches_active_snapshot(&self, staged: &HighlightRender) -> bool {
        let Some(selected_id) = self.selected_id.as_deref() else {
            return false;
        };
        staged.paste_id == selected_id
            && staged.revision == self.active_revision()
            && staged.text_len == self.active_text_len_bytes()
            && staged.content_hash == self.active_text_hash()
    }

    fn active_text_hash(&self) -> u64 {
        if self.is_virtual_editor_mode() {
            hash_text_chunks(self.virtual_editor_buffer.rope().chunks())
        } else {
            hash_bytes(self.selected_content.as_str().as_bytes())
        }
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

    pub(super) fn clear_highlight_state(&mut self) {
        self.highlight_pending = None;
        self.highlight_render = None;
        self.highlight_staged = None;
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
                || (current.revision == render.revision
                    && current.content_hash == render.content_hash))
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
                || (staged.revision == render.revision
                    && staged.content_hash == render.content_hash))
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
        self.trace_highlight(
            "queue",
            format!(
                "queued staged render revision={} text_len={}",
                render.revision, render.text_len
            )
            .as_str(),
        );
        self.highlight_staged = Some(render);
    }

    pub(super) fn apply_staged_highlight(&mut self) {
        let Some(render) = self.highlight_staged.take() else {
            return;
        };
        self.trace_highlight(
            "apply",
            format!(
                "applied staged render revision={} text_len={}",
                render.revision, render.text_len
            )
            .as_str(),
        );
        self.highlight_render = Some(render);
        self.highlight_version = self.highlight_version.wrapping_add(1);
    }

    pub(super) fn maybe_apply_staged_highlight(&mut self, now: Instant) {
        let Some(staged) = self.highlight_staged.as_ref() else {
            return;
        };
        if !self.staged_matches_active_snapshot(staged) {
            let active_revision = self.active_revision();
            let active_text_len = self.active_text_len_bytes();
            self.trace_highlight(
                "drop_stale_staged",
                format!(
                    "staged rev={} len={} active rev={} len={}",
                    staged.revision, staged.text_len, active_revision, active_text_len
                )
                .as_str(),
            );
            self.highlight_staged = None;
            return;
        }
        if let Some(current) = &self.highlight_render {
            if current.matches_context(
                staged.paste_id.as_str(),
                staged.language_hint.as_str(),
                staged.theme_key.as_str(),
            ) && (current.revision > staged.revision
                || (current.revision == staged.revision
                    && current.content_hash == staged.content_hash))
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
        if idle || self.is_virtual_editor_mode() {
            if idle {
                self.trace_highlight("apply_idle", "applying staged render after idle");
            } else {
                self.trace_highlight(
                    "apply_now",
                    "virtual editor mode; applying staged render immediately",
                );
            }
            self.apply_staged_highlight();
        } else {
            self.trace_highlight("hold", "staged render waiting for idle threshold");
        }
    }

    pub(super) fn should_request_highlight(
        &self,
        revision: u64,
        text_len: usize,
        language_hint: &str,
        theme_key: &str,
        debounce_active: bool,
        paste_id: &str,
    ) -> bool {
        if text_len >= HIGHLIGHT_PLAIN_THRESHOLD {
            self.trace_highlight("skip_request", "plain threshold exceeded");
            return false;
        }
        let content_hash = self.active_text_hash();
        if let Some(pending) = &self.highlight_pending {
            if pending.matches(
                revision,
                text_len,
                content_hash,
                language_hint,
                theme_key,
                paste_id,
            ) {
                self.trace_highlight("skip_request", "matching highlight request already pending");
                return false;
            }
        }
        if let Some(render) = &self.highlight_render {
            if render.matches_exact(
                revision,
                text_len,
                content_hash,
                language_hint,
                theme_key,
                paste_id,
            ) {
                self.trace_highlight("skip_request", "exact render already available");
                return false;
            }
        }
        if let Some(render) = &self.highlight_staged {
            if render.matches_exact(
                revision,
                text_len,
                content_hash,
                language_hint,
                theme_key,
                paste_id,
            ) {
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

    pub(super) fn dispatch_highlight_request(
        &mut self,
        revision: u64,
        text: String,
        language_hint: &str,
        theme_key: &str,
        paste_id: &str,
    ) {
        let text_len = text.len();
        let content_hash = hash_bytes(text.as_bytes());
        self.trace_highlight(
            "request",
            format!(
                "dispatch revision={} text_len={} hash={} lang={} theme={} paste={}",
                revision, text_len, content_hash, language_hint, theme_key, paste_id
            )
            .as_str(),
        );
        let request = HighlightRequest {
            paste_id: paste_id.to_string(),
            revision,
            text,
            content_hash,
            language_hint: language_hint.to_string(),
            theme_key: theme_key.to_string(),
        };
        self.highlight_pending = Some(HighlightRequestMeta {
            paste_id: paste_id.to_string(),
            revision,
            text_len,
            content_hash,
            language_hint: language_hint.to_string(),
            theme_key: theme_key.to_string(),
        });
        let _ = self.highlight_worker.tx.send(request);
    }
}
