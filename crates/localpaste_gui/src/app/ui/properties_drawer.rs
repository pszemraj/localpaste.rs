//! Right-side metadata drawer for infrequent property edits.

use super::super::*;
use eframe::egui;

const AUTO_LANGUAGE: &str = "__auto__";

/// Applies a language choice change to the metadata draft fields.
///
/// `AUTO_LANGUAGE` disables manual mode without erasing the detected language
/// value currently shown in the editor.
///
/// # Arguments
/// - `edit_language_is_manual`: Editable manual-language flag.
/// - `edit_language`: Editable language label field.
/// - `metadata_dirty`: Dirty marker toggled when the choice changes metadata.
/// - `language_choice`: Newly selected combo-box value.
/// - `current_manual_value`: Canonicalized current manual-language option.
pub(super) fn apply_language_choice(
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

/// Resolves the combo-box label for the selected language choice.
///
/// # Arguments
/// - `language_choice`: Selected combo-box option value.
/// - `auto_label`: Label text used for auto mode.
///
/// # Returns
/// User-facing text for the current language choice.
pub(super) fn selected_language_choice_text(language_choice: &str, auto_label: &str) -> String {
    if language_choice == AUTO_LANGUAGE {
        return auto_label.to_string();
    }
    localpaste_core::detection::canonical::manual_option_label(language_choice)
        .unwrap_or(language_choice)
        .to_string()
}

/// Returns the sentinel key used for auto language mode options.
///
/// # Returns
/// Stable option-key string representing auto mode in language selectors.
pub(super) fn auto_language_choice_key() -> &'static str {
    AUTO_LANGUAGE
}

impl LocalPasteApp {
    /// Renders the side drawer used for less-frequent metadata edits.
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
                let selected_language_text =
                    selected_language_choice_text(language_choice.as_str(), "Auto");
                egui::ComboBox::from_id_salt("drawer_language_select")
                    .selected_text(selected_language_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut language_choice,
                            auto_language_choice_key().to_string(),
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
    fn apply_language_choice_transition_matrix() {
        struct Case {
            is_manual: bool,
            language: Option<&'static str>,
            metadata_dirty: bool,
            language_choice: &'static str,
            current_manual_value: &'static str,
            expected_manual: bool,
            expected_language: Option<&'static str>,
            expected_dirty: bool,
        }

        let cases = [
            Case {
                is_manual: false,
                language: Some("rust"),
                metadata_dirty: false,
                language_choice: AUTO_LANGUAGE,
                current_manual_value: "rust",
                expected_manual: false,
                expected_language: Some("rust"),
                expected_dirty: false,
            },
            Case {
                is_manual: true,
                language: Some("python"),
                metadata_dirty: false,
                language_choice: AUTO_LANGUAGE,
                current_manual_value: "python",
                expected_manual: false,
                expected_language: Some("python"),
                expected_dirty: true,
            },
            Case {
                is_manual: false,
                language: Some("rust"),
                metadata_dirty: false,
                language_choice: "typescript",
                current_manual_value: "rust",
                expected_manual: true,
                expected_language: Some("typescript"),
                expected_dirty: true,
            },
        ];

        for case in cases {
            let mut is_manual = case.is_manual;
            let mut language = case.language.map(str::to_string);
            let mut metadata_dirty = case.metadata_dirty;

            apply_language_choice(
                &mut is_manual,
                &mut language,
                &mut metadata_dirty,
                case.language_choice,
                case.current_manual_value,
            );

            assert_eq!(is_manual, case.expected_manual);
            assert_eq!(language.as_deref(), case.expected_language);
            assert_eq!(metadata_dirty, case.expected_dirty);
        }
    }
}
