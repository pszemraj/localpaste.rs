//! Ephemeral toast notifications for short action feedback.

use super::super::*;
use eframe::egui;

impl LocalPasteApp {
    /// Renders transient toast notifications in the top-right overlay area.
    pub(crate) fn render_toasts(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }

        egui::Area::new(egui::Id::new("toast_area"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.vertical(|ui| {
                    for toast in self.toasts.iter().rev() {
                        egui::Frame::popup(ui.style())
                            .fill(COLOR_BG_SECONDARY)
                            .stroke(egui::Stroke::new(1.0, COLOR_BORDER))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(&toast.text)
                                        .small()
                                        .color(COLOR_TEXT_PRIMARY),
                                );
                            });
                    }
                });
            });
    }
}
