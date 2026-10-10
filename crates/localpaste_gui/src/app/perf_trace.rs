//! Input/perf tracing helpers extracted from `app::mod` to keep core app file under LoC limits.

use super::{
    InputTraceFrame, LocalPasteApp, VirtualApplyResult, VirtualInputCommand, PERF_LOG_INTERVAL,
    PERF_SAMPLE_CAP,
};
use std::time::Instant;
use tracing::info;

#[derive(Debug, Clone, Copy)]
/// Timing/counter snapshot for one virtual-editor input frame.
pub(super) struct VirtualInputPerfStats {
    pub(super) input_route_ms: f32,
    pub(super) apply_ms: f32,
    pub(super) apply_result: VirtualApplyResult,
}

impl LocalPasteApp {
    /// Emits detailed input-routing traces when editor input tracing is enabled.
    pub(super) fn trace_input(&self, frame: InputTraceFrame<'_>) {
        if !self.editor_input_trace_enabled {
            return;
        }
        info!(
            target: "localpaste_gui::input",
            focus_active_pre = frame.focus_active_pre,
            focus_active_post = frame.focus_active_post,
            egui_focus_pre = frame.egui_focus_pre,
            egui_focus_post = frame.egui_focus_post,
            copy_ready_post = frame.copy_ready_post,
            selection_chars = frame.selection_chars,
            command_count = frame.commands.len(),
            commands = ?frame.commands,
            changed = frame.apply_result.changed,
            copied = frame.apply_result.copied,
            cut = frame.apply_result.cut,
            pasted = frame.apply_result.pasted,
            "virtual input frame"
        );
    }

    /// Emits virtual-editor input performance metrics for observability logs.
    ///
    /// # Arguments
    /// - `commands`: Commands extracted and applied by the focused editor widget.
    /// - `stats`: Timing snapshot and aggregate apply results.
    pub(super) fn trace_virtual_input_perf(
        &self,
        commands: &[VirtualInputCommand],
        stats: VirtualInputPerfStats,
    ) {
        if !self.perf_log_enabled {
            return;
        }
        info!(
            target: "localpaste_gui::perf",
            event = "virtual_input_frame",
            commands = commands.len(),
            input_route_ms = stats.input_route_ms,
            apply_ms = stats.apply_ms,
            changed = stats.apply_result.changed,
            copied = stats.apply_result.copied,
            cut = stats.apply_result.cut,
            pasted = stats.apply_result.pasted,
            "virtual editor input ownership + apply timings"
        );
    }

    /// Samples frame times and periodically logs percentile snapshots when perf logging is on.
    ///
    /// # Panics
    /// Does not panic: the percentile math is floating point and only divides
    /// by a non-empty sample count or a clamped average.
    pub(super) fn track_frame_metrics(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame_at {
            let dt = now
                .saturating_duration_since(last)
                .as_secs_f32()
                .max(0.000_1);
            let frame_ms = dt * 1000.0;
            self.frame_samples.push_back(frame_ms);
            while self.frame_samples.len() > PERF_SAMPLE_CAP {
                self.frame_samples.pop_front();
            }
        }
        self.last_frame_at = Some(now);

        if !self.perf_log_enabled
            || now.saturating_duration_since(self.last_perf_log_at) < PERF_LOG_INTERVAL
        {
            return;
        }
        self.last_perf_log_at = now;
        if self.frame_samples.is_empty() {
            return;
        }
        let mut sorted: Vec<f32> = self.frame_samples.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let avg_ms = sorted.iter().sum::<f32>() / sorted.len() as f32;
        let p50_idx = (sorted.len().saturating_sub(1)) / 2;
        let p95_idx = ((sorted.len() as f32 * 0.95).ceil() as usize).saturating_sub(1);
        let p99_idx = ((sorted.len() as f32 * 0.99).ceil() as usize).saturating_sub(1);
        let p50_ms = sorted.get(p50_idx).copied().unwrap_or(avg_ms);
        let p95_ms = sorted.get(p95_idx).copied().unwrap_or(avg_ms);
        let p99_ms = sorted.get(p99_idx).copied().unwrap_or(avg_ms);
        let worst_ms = sorted.last().copied().unwrap_or(avg_ms);
        let slow_frames = sorted.iter().filter(|value| **value > 16.7).count();
        let fps = 1000.0 / avg_ms.max(0.001);
        let history = self.virtual_editor_history.perf_stats();
        info!(
            target: "localpaste_gui::perf",
            avg_fps = fps,
            avg_ms = avg_ms,
            p50_ms = p50_ms,
            p95_ms = p95_ms,
            p99_ms = p99_ms,
            worst_ms = worst_ms,
            slow_frames = slow_frames,
            samples = sorted.len(),
            list_sent = self.query_perf.list_requests_sent,
            list_applied = self.query_perf.list_results_applied,
            list_last_ms = self.query_perf.list_last_roundtrip_ms.unwrap_or(0.0),
            search_sent = self.query_perf.search_requests_sent,
            search_applied = self.query_perf.search_results_applied,
            search_stale_drops = self.query_perf.search_stale_drops,
            search_skipped_cached = self.query_perf.search_skipped_cached,
            search_skipped_debounce = self.query_perf.search_skipped_debounce,
            search_last_ms = self.query_perf.search_last_roundtrip_ms.unwrap_or(0.0),
            undo_len = history.undo_len,
            redo_len = history.redo_len,
            undo_bytes = history.undo_bytes,
            redo_invalidations = history.redo_invalidations,
            redo_hits = history.redo_hits,
            redo_misses = history.redo_misses,
            coalesced_edits = history.coalesced_edits,
            trim_evictions = history.trim_evictions,
            "local perf snapshot"
        );
    }
}
