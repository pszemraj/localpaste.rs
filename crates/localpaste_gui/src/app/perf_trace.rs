//! Input/perf tracing helpers extracted from `app::mod` to keep core app file under LoC limits.

use super::{EditorMode, InputTraceFrame, LocalPasteApp, VirtualApplyResult, VirtualInputCommand};
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
            mode = ?self.editor_mode,
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
        if !self.perf_log_enabled || self.editor_mode != EditorMode::VirtualEditor {
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
}
