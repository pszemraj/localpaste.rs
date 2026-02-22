//! Bottom status bar rendering for save state and server metadata.

use super::super::*;
use eframe::egui;

impl LocalPasteApp {
    /// Renders the bottom status bar with save state and API metadata.
    pub(crate) fn render_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let mut has_primary_item = false;
                    if self.selected_id.is_some() {
                        let (label, color) = match self.save_status {
                            SaveStatus::Saved => ("Saved", COLOR_TEXT_SECONDARY),
                            SaveStatus::Dirty => ("Unsaved", egui::Color32::YELLOW),
                            SaveStatus::Saving => ("Saving...", COLOR_TEXT_MUTED),
                        };
                        ui.label(egui::RichText::new(label).color(color));
                        has_primary_item = true;
                    }
                    if let Some(status) = &self.status {
                        if has_primary_item {
                            ui.separator();
                        }
                        ui.label(egui::RichText::new(&status.text).color(egui::Color32::YELLOW));
                        has_primary_item = true;
                    }
                    if has_primary_item {
                        ui.separator();
                    }
                    ui.label(egui::RichText::new("DB:").small().color(COLOR_TEXT_MUTED));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.db_path)
                                .small()
                                .monospace()
                                .color(COLOR_TEXT_SECONDARY),
                        )
                        .truncate(),
                    );
                });
                // Keep API metadata on its own row so long DB paths and status text
                // cannot starve/right-clip the endpoint label at narrow widths.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let api_label = if self.server_used_fallback {
                        format!("API: http://{} (auto)", self.server_addr)
                    } else {
                        format!("API: http://{}", self.server_addr)
                    };
                    ui.label(
                        egui::RichText::new(api_label)
                            .small()
                            .color(COLOR_TEXT_SECONDARY),
                    );
                    if self.selected_id.is_some() {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!("{} chars", self.active_text_chars()))
                                .small()
                                .color(COLOR_TEXT_MUTED),
                        );
                    }
                });
            });
    }
}
