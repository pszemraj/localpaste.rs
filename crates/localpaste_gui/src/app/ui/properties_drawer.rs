//! Right-side metadata drawer for infrequent property edits.

use super::super::*;
use eframe::egui;
use localpaste_core::semantic::DerivedMeta;

const AUTO_LANGUAGE: &str = "__auto__";

/// Applies a language choice change to the metadata draft fields.
///
/// `AUTO_LANGUAGE` disables manual mode and clears any resolved language so the
/// next content edit can trigger a fresh auto-detect pass.
///
/// # Arguments
/// - `edit_language_is_manual`: Editable manual-language flag.
/// - `edit_language`: Editable language label field.
/// - `metadata_dirty`: Dirty marker toggled when the choice changes metadata.
/// - `language_choice`: Newly selected combo-box value.
/// - `current_manual_value`: Canonicalized current manual-language option.
fn apply_language_choice(
    edit_language_is_manual: &mut bool,
    edit_language: &mut Option<String>,
    metadata_dirty: &mut bool,
    language_choice: &str,
    current_manual_value: &str,
) {
    if language_choice == AUTO_LANGUAGE {
        if *edit_language_is_manual || edit_language.is_some() {
            *edit_language_is_manual = false;
            *edit_language = None;
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
fn selected_language_choice_text(language_choice: &str, auto_label: &str) -> String {
    if language_choice == AUTO_LANGUAGE {
        return auto_label.to_string();
    }
    localpaste_core::detection::canonical::manual_option_label(language_choice)
        .unwrap_or(language_choice)
        .to_string()
}

/// Returns the compact label used for the auto language selector state.
///
/// # Returns
/// Static `Auto` label so combo-box selected text always matches the selected
/// option value when auto mode is active.
fn auto_language_status_label() -> String {
    "Auto".to_string()
}

fn format_derived_terms(terms: &[String]) -> String {
    if terms.is_empty() {
        "None".to_string()
    } else {
        terms.join(", ")
    }
}

fn render_derived_meta_section(ui: &mut egui::Ui, derived: &DerivedMeta) {
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);
    ui.label(
        RichText::new("Derived retrieval")
            .small()
            .color(COLOR_TEXT_MUTED),
    );
    ui.label(format!("Kind: {}", derived.kind.label()));
    ui.label(format!(
        "Handle: {}",
        derived.handle.as_deref().unwrap_or("None")
    ));
    ui.label(format!("Terms: {}", format_derived_terms(&derived.terms)));
}

/// Seconds without typing after which a language popup's type-ahead starts over.
const LANGUAGE_TYPEAHEAD_RESET_SECS: f64 = 1.0;

/// Language selector options in display order as `(value, label)`: `Auto`, then the manual languages.
fn language_options() -> impl Iterator<Item = (&'static str, &'static str)> {
    std::iter::once((AUTO_LANGUAGE, "Auto")).chain(
        localpaste_core::detection::canonical::MANUAL_LANGUAGE_OPTIONS
            .iter()
            .map(|option| (option.value, option.label)),
    )
}

/// Type-ahead state of one open language popup, kept in egui temp memory.
///
/// Typed characters accumulate until [`LANGUAGE_TYPEAHEAD_RESET_SECS`] pass
/// without typing. The buffer selects the first option whose label starts with
/// it, ignoring case. A buffer of one character typed repeatedly instead cycles
/// through the options starting with that character, wrapping at the end. A
/// buffer that matches no label keeps the current choice.
#[derive(Clone, Default)]
struct LanguageTypeahead {
    /// Lowercased characters typed since the buffer last started over.
    buffer: String,
    /// egui input time of the latest keystroke, in seconds.
    last_typed: f64,
}

impl LanguageTypeahead {
    /// Feeds one typed character and moves `language_choice` to the option it selects.
    ///
    /// # Arguments
    /// - `ch`: Typed character.
    /// - `now`: egui input time of the keystroke, in seconds.
    /// - `language_choice`: Selected option value, updated in place.
    ///
    /// # Returns
    /// `true` when `language_choice` changed.
    fn type_char(&mut self, ch: char, now: f64, language_choice: &mut String) -> bool {
        if now - self.last_typed >= LANGUAGE_TYPEAHEAD_RESET_SECS {
            self.buffer.clear();
        }
        self.last_typed = now;
        self.buffer.extend(ch.to_lowercase());
        match self.target(language_choice) {
            Some(target) if target != language_choice.as_str() => {
                *language_choice = target.to_string();
                true
            }
            _ => false,
        }
    }

    /// The option value the buffer selects from `current`, or `None` when nothing matches.
    fn target(&self, current: &str) -> Option<&'static str> {
        let first = self.buffer.chars().next()?;
        let label_starts_with =
            |label: &str, prefix: &str| label.to_lowercase().starts_with(prefix);
        let repeated =
            self.buffer.len() > first.len_utf8() && self.buffer.chars().all(|ch| ch == first);
        if !repeated {
            return language_options()
                .find(|(_, label)| label_starts_with(label, &self.buffer))
                .map(|(value, _)| value);
        }
        let prefix = first.to_string();
        let cycle = language_options()
            .filter(|(_, label)| label_starts_with(label, &prefix))
            .map(|(value, _)| value)
            .collect::<Vec<_>>();
        let next = cycle
            .iter()
            .position(|value| *value == current)
            .map_or(0, |index| (index + 1) % cycle.len());
        cycle.get(next).copied()
    }
}

/// Characters typed this frame that extend a type-ahead buffer.
///
/// Whitespace and control characters are dropped. Everything else is kept,
/// including `#` and `+` so `c#` and `c+` reach C# and C++.
fn typeahead_chars(events: &[egui::Event]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            egui::Event::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .flat_map(str::chars)
        .filter(|ch| !ch.is_whitespace() && !ch.is_control())
        .collect()
}

/// Applies this frame's typing to a language popup's type-ahead.
///
/// A closed popup ignores typing and drops its buffer, so reopening starts a
/// new one. Time comes from egui's input clock, which tests control through
/// `RawInput::time`.
///
/// # Arguments
/// - `ui`: UI that renders the combo box.
/// - `combo_id`: Widget id of the combo box.
/// - `popup_open`: Whether the combo's popup is open this frame.
/// - `language_choice`: Selected option value, updated in place.
///
/// # Returns
/// `true` when the typed characters changed `language_choice`.
fn apply_language_typeahead(
    ui: &egui::Ui,
    combo_id: egui::Id,
    popup_open: bool,
    language_choice: &mut String,
) -> bool {
    let state_id = combo_id.with("language_typeahead");
    if !popup_open {
        ui.data_mut(|data| data.remove::<LanguageTypeahead>(state_id));
        return false;
    }
    let (typed, now) = ui.input(|input| (typeahead_chars(&input.events), input.time));
    if typed.is_empty() {
        return false;
    }
    ui.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<LanguageTypeahead>(state_id);
        let mut changed = false;
        for ch in typed.chars() {
            changed |= state.type_char(ch, now, language_choice);
        }
        changed
    })
}

/// Returns the widget id egui assigns to a language combo box.
///
/// `ComboBox::from_id_salt` wraps its argument in `Id::new` and derives the
/// widget id with `ui.make_persistent_id`, and `ComboBox::is_open` expects that
/// widget id. Looking up a bare `Id::new(salt)` yields a different id, so the
/// popup state is never found and the combo always reads as closed.
///
/// # Arguments
/// - `ui`: UI the combo box is added to.
/// - `combo_id_salt`: Id salt passed to `ComboBox::from_id_salt`.
fn language_combo_id(ui: &egui::Ui, combo_id_salt: &str) -> egui::Id {
    ui.make_persistent_id(egui::Id::new(combo_id_salt))
}

/// A language popup opening or being cancelled during one frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LanguagePopupEvent {
    /// The popup opened; the current choice is the one a cancel returns to.
    Opened,
    /// Escape closed the popup to cancel the choices made while it was open.
    Cancelled,
}

/// Renders a language combo box with open-only type-ahead selection.
///
/// While the popup is open, typing selects an option as described on
/// [`LanguageTypeahead`] and scrolls it to the center of the popup so the jump
/// is visible. Enter closes the popup and keeps the choice. Escape closes it
/// and reports [`LanguagePopupEvent::Cancelled`] so the caller can restore the
/// choice from when it opened. Both keys are consumed, so no later handler in
/// the frame also acts on them.
///
/// # Arguments
/// - `ui`: Parent UI receiving the combo box.
/// - `combo_id_salt`: Stable egui id salt for the combo instance.
/// - `width`: Optional explicit combo width override.
/// - `language_choice`: Mutable selected option value.
///
/// # Returns
/// The popup event of this frame, if any.
fn render_language_choice_combo(
    ui: &mut egui::Ui,
    combo_id_salt: &str,
    width: Option<f32>,
    language_choice: &mut String,
) -> Option<LanguagePopupEvent> {
    let combo_id = language_combo_id(ui, combo_id_salt);
    let popup_open = egui::ComboBox::is_open(ui.ctx(), combo_id);
    let (cancel, accept) = if popup_open {
        ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            )
        })
    } else {
        (false, false)
    };
    let jumped = apply_language_typeahead(ui, combo_id, popup_open, language_choice);

    let auto_label = auto_language_status_label();
    let selected_language_text =
        selected_language_choice_text(language_choice.as_str(), auto_label.as_str());
    let mut combo =
        egui::ComboBox::from_id_salt(combo_id_salt).selected_text(selected_language_text);
    if let Some(width) = width {
        combo = combo.width(width);
    }
    combo.show_ui(ui, |ui| {
        if cancel || accept {
            ui.close();
        }
        for (value, label) in language_options() {
            let response = ui.selectable_value(language_choice, value.to_string(), label);
            if jumped && language_choice.as_str() == value {
                response.scroll_to_me(Some(egui::Align::Center));
            }
        }
    });
    if cancel {
        Some(LanguagePopupEvent::Cancelled)
    } else if !popup_open && egui::ComboBox::is_open(ui.ctx(), combo_id) {
        Some(LanguagePopupEvent::Opened)
    } else {
        None
    }
}

/// Language draft of one paste, captured when a language popup opens so Escape can restore it.
#[derive(Clone, Default)]
struct LanguageDraft {
    /// Paste the draft belongs to; a cancel after another paste loads restores nothing.
    paste_id: Option<String>,
    /// `edit_language` when the popup opened.
    language: Option<String>,
    /// `edit_language_is_manual` when the popup opened.
    is_manual: bool,
}

/// A metadata draft string edited in a single-line text field.
#[derive(Clone, Copy)]
pub(super) enum MetadataTextField {
    /// `edit_name`, edited by the toolbar title and the drawer's Name field.
    Name,
    /// `edit_tags` as comma-separated text, edited by the drawer's Tags field.
    Tags,
}

/// A metadata text field's draft when the field gained focus, kept so Escape can restore it.
#[derive(Clone, Default)]
struct FieldDraft {
    /// Paste the draft belongs to; a draft captured before another paste loaded is not restored.
    paste_id: Option<String>,
    /// The field's text when it gained focus.
    value: String,
}

impl LocalPasteApp {
    /// Recomputes `metadata_dirty` from whether the metadata draft differs from the loaded paste.
    pub(super) fn refresh_metadata_dirty(&mut self) {
        if let Some(paste) = self.selected_paste.as_ref() {
            self.metadata_dirty = self.edit_name != paste.name
                || self.edit_language != paste.language
                || self.edit_language_is_manual != paste.language_is_manual
                || self.edit_tags != paste.tags.join(", ");
        }
    }

    /// The draft string `field` edits.
    fn metadata_text_mut(&mut self, field: MetadataTextField) -> &mut String {
        match field {
            MetadataTextField::Name => &mut self.edit_name,
            MetadataTextField::Tags => &mut self.edit_tags,
        }
    }

    /// The loaded paste's value of `field`, in the form the draft holds it.
    fn saved_metadata_text(&self, field: MetadataTextField) -> Option<String> {
        let paste = self.selected_paste.as_ref()?;
        Some(match field {
            MetadataTextField::Name => paste.name.clone(),
            MetadataTextField::Tags => paste.tags.join(", "),
        })
    }

    /// Renders a single-line text field bound to a metadata draft string.
    ///
    /// Typing dirties metadata. Escape cancels the edit made since the field
    /// gained focus: the draft returns to its text from that moment, or to the
    /// loaded paste's value when another paste has loaded since, and
    /// `metadata_dirty` is recomputed against the loaded paste, so edits made
    /// elsewhere stay staged. egui releases focus on Escape before widgets
    /// render, so the field detects the key by having held focus last frame and
    /// consumes it.
    ///
    /// # Arguments
    /// - `ui`: Parent UI receiving the field.
    /// - `id`: Widget id of the text field.
    /// - `field`: Draft string the field edits.
    /// - `style`: Applies the field's width, font, and placeholder.
    ///
    /// # Returns
    /// The field's response, and whether Escape cancelled its edit this frame.
    pub(super) fn render_metadata_text_field(
        &mut self,
        ui: &mut egui::Ui,
        id: egui::Id,
        field: MetadataTextField,
        style: impl for<'t> FnOnce(egui::TextEdit<'t>) -> egui::TextEdit<'t>,
    ) -> (egui::Response, bool) {
        let draft_id = id.with("focus_draft");
        let cancelled = ui.memory(|memory| memory.had_focus_last_frame(id))
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if cancelled {
            let restored = ui
                .data_mut(|data| data.remove_temp::<FieldDraft>(draft_id))
                .filter(|draft| draft.paste_id == self.selected_id)
                .map(|draft| draft.value)
                .or_else(|| self.saved_metadata_text(field));
            if let Some(value) = restored {
                *self.metadata_text_mut(field) = value;
                self.refresh_metadata_dirty();
            }
        }
        let response = ui.add(style(
            egui::TextEdit::singleline(self.metadata_text_mut(field)).id(id),
        ));
        if response.gained_focus() {
            let draft = FieldDraft {
                paste_id: self.selected_id.clone(),
                value: self.metadata_text_mut(field).clone(),
            };
            ui.data_mut(|data| data.insert_temp(draft_id, draft));
        }
        if response.changed() {
            self.metadata_dirty = true;
        }
        (response, cancelled)
    }

    /// Renders a language combo bound to the metadata draft.
    ///
    /// A click or type-ahead in the popup applies its choice to the draft at
    /// once. Escape restores the draft captured when the popup opened and
    /// recomputes `metadata_dirty` against the loaded paste, so a cancelled
    /// choice leaves nothing to save. The captured draft keeps the language
    /// auto-detection resolved, which re-applying `Auto` would clear.
    ///
    /// # Arguments
    /// - `ui`: Parent UI receiving the combo box.
    /// - `combo_id_salt`: Stable egui id salt for the combo instance.
    /// - `width`: Optional explicit combo width override.
    pub(super) fn render_language_field(
        &mut self,
        ui: &mut egui::Ui,
        combo_id_salt: &str,
        width: Option<f32>,
    ) {
        let draft_id = language_combo_id(ui, combo_id_salt).with("language_draft");
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
        let previous_language_choice = language_choice.clone();
        match render_language_choice_combo(ui, combo_id_salt, width, &mut language_choice) {
            Some(LanguagePopupEvent::Opened) => {
                let draft = LanguageDraft {
                    paste_id: self.selected_id.clone(),
                    language: self.edit_language.clone(),
                    is_manual: self.edit_language_is_manual,
                };
                ui.data_mut(|data| data.insert_temp(draft_id, draft));
            }
            Some(LanguagePopupEvent::Cancelled) => {
                let draft = ui.data_mut(|data| data.remove_temp::<LanguageDraft>(draft_id));
                if let Some(draft) = draft.filter(|draft| draft.paste_id == self.selected_id) {
                    self.edit_language = draft.language;
                    self.edit_language_is_manual = draft.is_manual;
                    self.refresh_metadata_dirty();
                }
                return;
            }
            None => {}
        }
        if language_choice != previous_language_choice {
            apply_language_choice(
                &mut self.edit_language_is_manual,
                &mut self.edit_language,
                &mut self.metadata_dirty,
                language_choice.as_str(),
                current_manual_value.as_str(),
            );
        }
    }

    /// Renders the side drawer used for less-frequent metadata edits.
    pub(crate) fn render_properties_drawer(&mut self, ctx: &egui::Context) {
        if !self.properties_drawer_open || self.selected_id.is_none() {
            return;
        }

        let mutation_block_reason = self.mutation_shortcut_block_reason();
        let save_blocked = self.save_block_reason().is_some();
        let background_mutation_blocked = mutation_block_reason.is_some();
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

                ui.add_enabled_ui(!background_mutation_blocked, |ui| {
                    ui.label(RichText::new("Name").small().color(COLOR_TEXT_MUTED));
                    self.render_metadata_text_field(
                        ui,
                        egui::Id::new(PROPERTIES_NAME_INPUT_ID),
                        MetadataTextField::Name,
                        |edit| edit.desired_width(f32::INFINITY),
                    );

                    ui.add_space(6.0);
                    ui.label(RichText::new("Language").small().color(COLOR_TEXT_MUTED));
                    self.render_language_field(ui, "drawer_language_select", None);

                    ui.add_space(6.0);
                    ui.label(RichText::new("Tags").small().color(COLOR_TEXT_MUTED));
                    self.render_metadata_text_field(
                        ui,
                        egui::Id::new(PROPERTIES_TAGS_INPUT_ID),
                        MetadataTextField::Tags,
                        |edit| {
                            edit.desired_width(f32::INFINITY)
                                .muted_hint("comma,separated,tags")
                        },
                    );
                });
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            !save_blocked && self.metadata_dirty && !self.metadata_save_in_flight,
                            egui::Button::new("Save Metadata"),
                        )
                        .clicked()
                    {
                        self.save_metadata_now();
                    }
                });
                if let Some(reason) = mutation_block_reason {
                    ui.add_space(6.0);
                    ui.label(RichText::new(reason).small().color(COLOR_TEXT_MUTED));
                }
                if let Some(summary) = self.selected_paste_summary() {
                    render_derived_meta_section(ui, &summary.derived);
                }
                ui.add_space(10.0);
                if ui.button("Export").clicked() {
                    self.export_selected_paste();
                }
            });

        if !keep_open {
            self.properties_drawer_open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_language_choice, auto_language_status_label, format_derived_terms, language_options,
        typeahead_chars, LanguageTypeahead, AUTO_LANGUAGE,
    };
    use eframe::egui;

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
                language: None,
                metadata_dirty: false,
                language_choice: AUTO_LANGUAGE,
                current_manual_value: "text",
                expected_manual: false,
                expected_language: None,
                expected_dirty: false,
            },
            Case {
                is_manual: true,
                language: Some("python"),
                metadata_dirty: false,
                language_choice: AUTO_LANGUAGE,
                current_manual_value: "python",
                expected_manual: false,
                expected_language: None,
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

    #[test]
    fn auto_language_status_label_matrix() {
        assert_eq!(auto_language_status_label(), "Auto");
    }

    #[test]
    fn only_auto_starts_with_a_so_a_alone_selects_it() {
        let starting_with_a = language_options()
            .filter(|(_, label)| label.to_lowercase().starts_with('a'))
            .map(|(value, _)| value)
            .collect::<Vec<_>>();
        assert_eq!(starting_with_a, [AUTO_LANGUAGE]);
    }

    #[test]
    fn language_typeahead_script_matrix() {
        // (key, input time, choice afterwards, whether the key changed it)
        let script = [
            ('P', 10.0, "perl", true),
            ('Y', 10.2, "python", true),
            ('t', 10.4, "python", false),
            ('z', 10.6, "python", false),
            ('c', 12.0, "c", true),
            ('#', 12.1, "cs", true),
            ('p', 14.0, "perl", true),
            ('p', 14.1, "text", true),
            ('p', 14.2, "powershell", true),
            ('p', 14.3, "python", true),
            ('p', 14.4, "perl", true),
            ('a', 16.0, AUTO_LANGUAGE, true),
            ('a', 16.1, AUTO_LANGUAGE, false),
            ('1', 18.0, AUTO_LANGUAGE, false),
        ];
        let mut state = LanguageTypeahead::default();
        let mut choice = "lua".to_string();
        for (key, time, expected, changed) in script {
            assert_eq!(
                state.type_char(key, time, &mut choice),
                changed,
                "{key} at {time}"
            );
            assert_eq!(choice, expected, "{key} at {time}");
        }
    }

    #[test]
    fn typeahead_chars_drop_whitespace_and_controls() {
        let events = vec![
            egui::Event::Text(" c".to_string()),
            egui::Event::Key {
                key: egui::Key::P,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Text("+\t+".to_string()),
        ];
        assert_eq!(typeahead_chars(&events), "c++");
    }

    #[test]
    fn format_derived_terms_handles_empty_and_joined_lists() {
        assert_eq!(format_derived_terms(&[]), "None");
        assert_eq!(
            format_derived_terms(&["fsdp2".to_string(), "cublaslt".to_string()]),
            "fsdp2, cublaslt"
        );
    }
}
