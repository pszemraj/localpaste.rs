//! Right-side metadata drawer for infrequent property edits.

use super::super::*;
use eframe::egui;

const AUTO_LANGUAGE: &str = "__auto__";

fn apply_language_choice(
    edit_language_is_manual: &mut bool,
    edit_language: &mut Option<String>,
    metadata_dirty: &mut bool,
    language_choice: &str,
    current_manual_value: &str,
) {
    if language_choice == AUTO_LANGUAGE {
        // Auto mode still carries a detected language value for highlighting/filter labels.
        // Only mark dirty when transitioning from manual -> auto.
        if *edit_language_is_manual {
            *edit_language_is_manual = false;
            *metadata_dirty = true;
        }
        return;
    }

    if !*edit_language_is_manual || current_manual_value != language_choice {
        *edit_language_is_manual = true;
        *edit_language = Some(language_choice.to_string());
        *metadata_dirty = true;
    }
}

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
                let current_manual_value = self
                    .edit_language
                    .as_deref()
                    .map(localpaste_core::detection::canonical::canonicalize)
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "text".to_string());
                let mut language_choice = if self.edit_language_is_manual {
                    current_manual_value.clone()
                } else {
                    AUTO_LANGUAGE.to_string()
                };
                let selected_language_text = if language_choice == AUTO_LANGUAGE {
                    "Auto".to_string()
                } else {
                    localpaste_core::detection::canonical::manual_option_label(
                        language_choice.as_str(),
                    )
                    .unwrap_or(language_choice.as_str())
                    .to_string()
                };
                egui::ComboBox::from_id_salt("drawer_language_select")
                    .selected_text(selected_language_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut language_choice,
                            AUTO_LANGUAGE.to_string(),
                            "Auto",
                        );
                        for option in localpaste_core::detection::canonical::MANUAL_LANGUAGE_OPTIONS
                        {
                            ui.selectable_value(
                                &mut language_choice,
                                option.value.to_string(),
                                option.label,
                            );
                        }
                    });
                apply_language_choice(
                    &mut self.edit_language_is_manual,
                    &mut self.edit_language,
                    &mut self.metadata_dirty,
                    language_choice.as_str(),
                    current_manual_value.as_str(),
                );

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

#[cfg(test)]
mod tests {
    use super::{apply_language_choice, AUTO_LANGUAGE};

    #[test]
    fn auto_mode_with_detected_language_does_not_dirty_or_clear_language() {
        let mut is_manual = false;
        let mut language = Some("rust".to_string());
        let mut metadata_dirty = false;

        apply_language_choice(
            &mut is_manual,
            &mut language,
            &mut metadata_dirty,
            AUTO_LANGUAGE,
            "rust",
        );

        assert!(!is_manual);
        assert_eq!(language.as_deref(), Some("rust"));
        assert!(!metadata_dirty);
    }

    #[test]
    fn manual_to_auto_marks_dirty_but_preserves_language_value() {
        let mut is_manual = true;
        let mut language = Some("python".to_string());
        let mut metadata_dirty = false;

        apply_language_choice(
            &mut is_manual,
            &mut language,
            &mut metadata_dirty,
            AUTO_LANGUAGE,
            "python",
        );

        assert!(!is_manual);
        assert_eq!(language.as_deref(), Some("python"));
        assert!(metadata_dirty);
    }

    #[test]
    fn auto_to_manual_sets_manual_language_and_marks_dirty() {
        let mut is_manual = false;
        let mut language = Some("rust".to_string());
        let mut metadata_dirty = false;

        apply_language_choice(
            &mut is_manual,
            &mut language,
            &mut metadata_dirty,
            "typescript",
            "rust",
        );

        assert!(is_manual);
        assert_eq!(language.as_deref(), Some("typescript"));
        assert!(metadata_dirty);
    }
}
