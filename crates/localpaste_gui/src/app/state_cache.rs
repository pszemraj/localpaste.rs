//! Sidebar cache helpers for authoritative paste-summary updates.

use super::LocalPasteApp;
use crate::backend::PasteSummary;
use localpaste_core::models::paste::Paste;

impl LocalPasteApp {
    fn sort_paste_summaries_by_recency(items: &mut [PasteSummary]) {
        items.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    /// Applies an authoritative paste row to cached sidebar summaries.
    ///
    /// The canonical cache (`all_pastes`) is kept in newest-first order so
    /// collection filtering can preserve the UI's recency contract without
    /// waiting for a backend list refresh.
    pub(super) fn upsert_cached_paste_summary(&mut self, paste: &Paste) {
        let summary = PasteSummary::from_paste(paste);
        if let Some(item) = self
            .all_pastes
            .iter_mut()
            .find(|item| item.id == summary.id)
        {
            *item = summary.clone();
        } else {
            self.all_pastes.push(summary.clone());
        }
        Self::sort_paste_summaries_by_recency(&mut self.all_pastes);

        if let Some(item) = self.pastes.iter_mut().find(|item| item.id == summary.id) {
            *item = summary;
        }
    }

    /// Recomputes the visible sidebar projection from the canonical paste cache.
    pub(super) fn recompute_visible_pastes(&mut self) {
        Self::sort_paste_summaries_by_recency(&mut self.all_pastes);
        self.pastes = self.filter_by_collection(&self.all_pastes);
    }
}
