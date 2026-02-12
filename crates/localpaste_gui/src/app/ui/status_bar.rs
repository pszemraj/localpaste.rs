//! Bottom status bar rendering for save state and server metadata.

use super::super::*;
use eframe::egui;

impl LocalPasteApp {
    pub(crate) fn render_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.selected_id.is_some() {
                        let (label, color) = match self.save_status {
                            SaveStatus::Saved => ("Saved", COLOR_TEXT_SECONDARY),
                            SaveStatus::Dirty => ("Unsaved", egui::Color32::YELLOW),
                            SaveStatus::Saving => ("Saving...", COLOR_TEXT_MUTED),
                        };
                        ui.label(egui::RichText::new(label).color(color));
                        ui.separator();
                    }
                    if let Some(status) = &self.status {
                        ui.label(egui::RichText::new(&status.text).color(egui::Color32::YELLOW));
                    }

                    let language_options = self.language_filter_options();
                    if !language_options.is_empty() {
                        ui.separator();
                        ui.label(
                            egui::RichText::new("Language")
                                .small()
                                .color(COLOR_TEXT_MUTED),
                        );
                        let mut selected_language = self.active_language_filter.clone();
                        egui::ComboBox::from_id_salt("status_language_filter")
                            .selected_text(selected_language.as_deref().unwrap_or("Any"))
                            .width(140.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut selected_language, None, "Any");
                                for lang in &language_options {
                                    ui.selectable_value(
                                        &mut selected_language,
                                        Some(lang.clone()),
                                        lang.as_str(),
                                    );
                                }
                            });
                        if selected_language != self.active_language_filter {
                            self.set_active_language_filter(selected_language);
                        }
                    }
                });
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
