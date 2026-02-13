//! Right-side metadata drawer for infrequent property edits.

use super::super::*;
use eframe::egui;

impl LocalPasteApp {
    pub(crate) fn render_properties_drawer(&mut self, ctx: &egui::Context) {
        if !self.properties_drawer_open || self.selected_id.is_none() {
            return;
        }

        let mut keep_open = true;
        egui::SidePanel::right("properties_drawer")
            .default_width(320.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Properties");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Close").clicked() {
                            keep_open = false;
                        }
                    });
                });

                if let Some(id) = &self.selected_id {
                    ui.label(
                        RichText::new(id.as_str())
                            .small()
                            .monospace()
                            .color(COLOR_TEXT_MUTED),
                    );
                }
                ui.separator();

                ui.label(RichText::new("Name").small().color(COLOR_TEXT_MUTED));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.edit_name)
                            .desired_width(f32::INFINITY),
                    )
                    .changed()
                {
                    self.metadata_dirty = true;
                }

                ui.add_space(6.0);
                ui.label(RichText::new("Language").small().color(COLOR_TEXT_MUTED));
                let mut language_choice = if self.edit_language_is_manual {
                    self.edit_language
                        .clone()
                        .unwrap_or_else(|| "text".to_string())
                } else {
                    "Auto".to_string()
                };
                egui::ComboBox::from_id_salt("drawer_language_select")
                    .selected_text(language_choice.clone())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut language_choice, "Auto".to_string(), "Auto");
                        for value in [
                            "rust",
                            "python",
                            "javascript",
                            "typescript",
                            "json",
                            "yaml",
                            "toml",
                            "markdown",
                            "html",
                            "css",
                            "sql",
                            "shell",
                            "text",
                        ] {
                            ui.selectable_value(
                                &mut language_choice,
                                value.to_string(),
                                value.to_string(),
                            );
                        }
                    });
                if language_choice == "Auto" {
                    if self.edit_language_is_manual || self.edit_language.is_some() {
                        self.edit_language_is_manual = false;
                        self.edit_language = None;
                        self.metadata_dirty = true;
                    }
                } else if !self.edit_language_is_manual
                    || self.edit_language.as_deref() != Some(language_choice.as_str())
                {
                    self.edit_language_is_manual = true;
                    self.edit_language = Some(language_choice);
                    self.metadata_dirty = true;
                }

                ui.add_space(6.0);
                ui.label(RichText::new("Tags").small().color(COLOR_TEXT_MUTED));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut self.edit_tags)
                            .desired_width(f32::INFINITY)
                            .hint_text("comma,separated,tags"),
                    )
                    .changed()
                {
                    self.metadata_dirty = true;
                }

                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            self.metadata_dirty && !self.metadata_save_in_flight,
                            egui::Button::new("Save Metadata"),
                        )
                        .clicked()
                    {
                        self.save_metadata_now();
                    }
                    if ui.button("Save").clicked() {
                        self.save_now();
                    }
                    if ui.button("Export").clicked() {
                        self.export_selected_paste();
                    }
                });
            });

        if !keep_open {
            self.properties_drawer_open = false;
        }
    }
}
