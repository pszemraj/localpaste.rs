//! Ephemeral toast notifications for failures, warnings, and undoable actions.

use super::super::*;
use eframe::egui;

/// Gap between the toast stack and the window edge or status bar.
const TOAST_MARGIN: f32 = 12.0;

impl LocalPasteApp {
    /// Renders transient toasts stacked bottom-right, just above the status bar.
    ///
    /// Call after the status bar panel so the stack anchors above it; the
    /// newest toast sits closest to the status bar. Keeping toasts out of the
    /// top bar and editor toolbar means they never cover those controls. The
    /// stack paints above floating windows and below popups and tooltips.
    pub(crate) fn render_toasts(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }

        // After the bottom panel, the remaining area ends at the status bar's top edge.
        let above_status_bar =
            ctx.available_rect().right_bottom() - egui::vec2(TOAST_MARGIN, TOAST_MARGIN);
        let mut restore_undo_token: Option<String> = None;
        let stack = egui::Area::new(egui::Id::new("toast_area"))
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .fixed_pos(above_status_bar)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.vertical(|ui| {
                    for toast in &self.toasts {
                        egui::Frame::popup(ui.style())
                            .fill(COLOR_BG_SECONDARY)
                            .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        egui::RichText::new(&toast.text)
                                            .small()
                                            .color(COLOR_TEXT_PRIMARY),
                                    );
                                    if let Some(ToastAction::UndoDelete { undo_token }) =
                                        &toast.action
                                    {
                                        if ui.small_button("Undo").clicked() {
                                            restore_undo_token = Some(undo_token.clone());
                                        }
                                    }
                                });
                            });
                    }
                });
            });
        // Toasts share the floating-window order, so popups, menus, and tooltips
        // stay above them. Raising the stack every frame keeps it above Diff and
        // History, which a click inside them would otherwise bring forward.
        ctx.move_to_top(stack.response.layer_id);
        if let Some(undo_token) = restore_undo_token {
            self.restore_deleted_paste(undo_token);
        }
    }
}
