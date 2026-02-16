//! Input/perf tracing helpers extracted from `app::mod` to keep core app file under LoC limits.

use super::{EditorMode, InputTraceFrame, LocalPasteApp, VirtualApplyResult, VirtualInputCommand};
use tracing::info;

#[derive(Debug, Clone, Copy)]
pub(super) struct VirtualInputPerfStats {
    pub(super) input_route_ms: f32,
    pub(super) immediate_apply_ms: f32,
    pub(super) deferred_focus_apply_ms: f32,
    pub(super) deferred_copy_apply_ms: f32,
    pub(super) apply_result: VirtualApplyResult,
}

impl LocalPasteApp {
    pub(super) fn trace_input(&self, frame: InputTraceFrame<'_>) {
        if !self.editor_input_trace_enabled {
            return;
        }
        info!(
            target: "localpaste_gui::input",
            mode = ?self.editor_mode,
            editor_active = self.virtual_editor_active,
            focus_active_pre = frame.focus_active_pre,
            focus_active_post = frame.focus_active_post,
            egui_focus_pre = frame.egui_focus_pre,
            egui_focus_post = frame.egui_focus_post,
            copy_ready_post = frame.copy_ready_post,
            selection_chars = frame.selection_chars,
            immediate_focus_count = frame.immediate_focus_commands.len(),
            deferred_focus_count = frame.deferred_focus_commands.len(),
            deferred_copy_count = frame.deferred_copy_commands.len(),
            immediate_focus = ?frame.immediate_focus_commands,
            deferred_focus = ?frame.deferred_focus_commands,
            deferred_copy = ?frame.deferred_copy_commands,
            changed = frame.apply_result.changed,
            copied = frame.apply_result.copied,
            cut = frame.apply_result.cut,
            pasted = frame.apply_result.pasted,
            "virtual input frame"
        );
    }

    pub(super) fn trace_virtual_input_perf(
        &self,
        immediate_focus_commands: &[VirtualInputCommand],
        deferred_focus_commands: &[VirtualInputCommand],
        deferred_copy_commands: &[VirtualInputCommand],
        stats: VirtualInputPerfStats,
    ) {
        if !self.perf_log_enabled || self.editor_mode != EditorMode::VirtualEditor {
            return;
        }
        info!(
            target: "localpaste_gui::perf",
            event = "virtual_input_frame",
            immediate_focus_commands = immediate_focus_commands.len(),
            deferred_focus_commands = deferred_focus_commands.len(),
            deferred_copy_commands = deferred_copy_commands.len(),
            input_route_ms = stats.input_route_ms,
            immediate_apply_ms = stats.immediate_apply_ms,
            deferred_focus_apply_ms = stats.deferred_focus_apply_ms,
            deferred_copy_apply_ms = stats.deferred_copy_apply_ms,
            changed = stats.apply_result.changed,
            copied = stats.apply_result.copied,
            cut = stats.apply_result.cut,
            pasted = stats.apply_result.pasted,
            "virtual editor input routing + apply timings"
        );
    }
}
