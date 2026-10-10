//! Editor toolbar: title, language, tags, and paste actions on one row.
//!
//! The metadata group (title, language, Apply, tag chips) is left-aligned and
//! the frequent actions are right-aligned. Less frequent actions live in the
//! `More` menu. When the editor is too narrow for both groups, the actions
//! move to a second row instead of overlapping the metadata.

use super::super::*;
use super::properties_drawer::MetadataTextField;
use crate::app::util::parse_tags_csv;
use eframe::egui;

/// Label of the overflow menu holding the less frequent paste actions.
pub(in crate::app) const MORE_MENU_LABEL: &str = "More ⏷";
const TITLE_MIN_WIDTH: f32 = 140.0;
const TITLE_MAX_WIDTH: f32 = 360.0;
/// Title field padding; its width is added to the text width when fitting the row.
const TITLE_MARGIN: egui::Margin = egui::Margin::symmetric(4, 2);
const LANGUAGE_COMBO_WIDTH: f32 = 140.0;
/// Tag chips shown before the rest collapse into a `+N` chip.
const MAX_VISIBLE_TAGS: usize = 3;
/// Space kept between the metadata group and the right-aligned actions.
const GROUP_GAP: f32 = 16.0;

/// Width egui gives a vertical `ui.separator()` in a row, before item spacing.
const SEPARATOR_WIDTH: f32 = 6.0;

/// Toolbar actions in visual order; a separator sets off the destructive one.
const TOOLBAR_ACTIONS: [ToolbarAction; 7] = [
    ToolbarAction::Copy,
    ToolbarAction::Find,
    ToolbarAction::Diff,
    ToolbarAction::History,
    ToolbarAction::More,
    ToolbarAction::Separator,
    ToolbarAction::Delete,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolbarAction {
    Copy,
    Find,
    Diff,
    History,
    More,
    Separator,
    Delete,
}

impl ToolbarAction {
    fn label(self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Find => "Find",
            Self::Diff => "Diff",
            Self::History => "History",
            Self::More => MORE_MENU_LABEL,
            Self::Separator => "",
            Self::Delete => "Delete",
        }
    }
}

/// Requests raised by the editor toolbar during one frame.
#[derive(Default)]
pub(super) struct ToolbarRequests {
    pub(super) apply_metadata: bool,
    pub(super) copy: bool,
    pub(super) copy_link: bool,
    pub(super) duplicate: bool,
    pub(super) export: bool,
    pub(super) find: bool,
    pub(super) properties: bool,
    pub(super) delete: bool,
    pub(super) diff: bool,
    pub(super) history: bool,
    pub(super) tag_search: Option<String>,
    /// A mouse-first action fired while the virtual editor owned focus, so the
    /// editor keeps it rather than treating the click as an outside click.
    pub(super) preserve_editor_focus: bool,
}

impl LocalPasteApp {
    /// Renders the editor toolbar and returns the actions it requested.
    ///
    /// # Arguments
    /// - `ui`: Editor panel UI receiving the toolbar rows.
    /// - `editor_had_virtual_focus`: Whether the virtual editor owned keyboard
    ///   focus before this frame's toolbar interaction.
    ///
    /// # Returns
    /// Actions for the caller to apply after rendering.
    pub(super) fn render_editor_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        editor_had_virtual_focus: bool,
    ) -> ToolbarRequests {
        let mut requests = ToolbarRequests::default();
        ui.scope(|ui| {
            apply_compact_meta_row_style(ui);
            let actions_width = toolbar_actions_width(ui);
            let metadata_min_width = TITLE_MIN_WIDTH
                + TITLE_MARGIN.sum().x
                + LANGUAGE_COMBO_WIDTH
                + button_width(ui, "Apply")
                + 2.0 * ui.spacing().item_spacing.x
                + overflow_chip_reserve(ui, parse_tags_csv(self.edit_tags.as_str()).len());
            if ui.available_width() >= metadata_min_width + GROUP_GAP + actions_width {
                let (row_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                    egui::Sense::hover(),
                );
                // Actions first, from the right edge; metadata takes what remains.
                let mut actions_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(row_rect)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                self.render_toolbar_actions(
                    &mut actions_ui,
                    TOOLBAR_ACTIONS.iter().rev(),
                    editor_had_virtual_focus,
                    &mut requests,
                );
                let metadata_right = actions_ui.min_rect().left() - GROUP_GAP;
                let mut metadata_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(row_rect.with_max_x(metadata_right.max(row_rect.left())))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                self.render_toolbar_metadata(&mut metadata_ui, &mut requests);
            } else {
                ui.horizontal(|ui| self.render_toolbar_metadata(ui, &mut requests));
                ui.horizontal(|ui| {
                    self.render_toolbar_actions(
                        ui,
                        TOOLBAR_ACTIONS.iter(),
                        editor_had_virtual_focus,
                        &mut requests,
                    );
                });
            }
        });
        requests
    }

    /// Title, language, Apply, and tag chips, laid out left to right.
    fn render_toolbar_metadata(&mut self, ui: &mut egui::Ui, requests: &mut ToolbarRequests) {
        let background_mutation_blocked = self.mutation_shortcut_block_reason().is_some();
        let save_blocked = self.save_block_reason().is_some();
        let tags = parse_tags_csv(self.edit_tags.as_str());
        let apply_width = button_width(ui, "Apply");
        let spacing = ui.spacing().item_spacing.x;
        let tags_width = visible_tag_chips(ui, &tags, f32::INFINITY)
            .iter()
            .map(|chip| chip.width + spacing)
            .sum::<f32>();
        let title_width = (ui.available_width()
            - TITLE_MARGIN.sum().x
            - LANGUAGE_COMBO_WIDTH
            - apply_width
            - 3.0 * spacing
            - tags_width)
            .clamp(TITLE_MIN_WIDTH, TITLE_MAX_WIDTH);

        ui.add_enabled_ui(!background_mutation_blocked, |ui| {
            self.render_title_field(ui, title_width, requests);
            self.render_language_field(ui, "header_language_select", Some(LANGUAGE_COMBO_WIDTH));
        });
        if ui
            .add_enabled(
                !save_blocked && self.metadata_dirty && !self.metadata_save_in_flight,
                toolbar_button("Apply"),
            )
            .clicked()
        {
            requests.apply_metadata = true;
        }
        for chip in visible_tag_chips(ui, &tags, ui.available_width()) {
            match chip.tag {
                Some(tag) => {
                    if small_toolbar_button(ui, chip.label).clicked() {
                        requests.tag_search = Some(tag);
                    }
                }
                None => {
                    if small_toolbar_button(ui, chip.label)
                        .on_hover_text(tags.join(", "))
                        .clicked()
                    {
                        requests.properties = true;
                    }
                }
            }
        }
    }

    fn render_title_field(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        requests: &mut ToolbarRequests,
    ) {
        let (name_response, cancelled) = self.render_metadata_text_field(
            ui,
            egui::Id::new(TITLE_INPUT_ID),
            MetadataTextField::Name,
            |edit| {
                edit.font(egui::TextStyle::Button)
                    .desired_width(width)
                    .margin(TITLE_MARGIN)
                    .muted_hint("Untitled paste")
            },
        );
        // Escape also takes focus away, but it cancels the rename instead of applying it.
        if name_response.lost_focus() && self.metadata_dirty && !cancelled {
            requests.apply_metadata = true;
        }
        if name_response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            requests.apply_metadata = true;
        }
    }

    /// Renders `actions` in iteration order; a right-to-left UI passes them reversed.
    fn render_toolbar_actions<'a>(
        &mut self,
        ui: &mut egui::Ui,
        actions: impl Iterator<Item = &'a ToolbarAction>,
        editor_had_virtual_focus: bool,
        requests: &mut ToolbarRequests,
    ) {
        let background_mutation_blocked = self.mutation_shortcut_block_reason().is_some();
        for action in actions {
            let clicked = match action {
                ToolbarAction::Separator => {
                    ui.separator();
                    false
                }
                ToolbarAction::Delete => ui
                    .add_enabled(
                        !background_mutation_blocked,
                        toolbar_button("Delete").small(),
                    )
                    .clicked(),
                ToolbarAction::More => {
                    self.render_more_menu(ui, background_mutation_blocked, requests)
                }
                _ => small_toolbar_button(ui, action.label()).clicked(),
            };
            if !clicked {
                continue;
            }
            match action {
                ToolbarAction::Copy => requests.copy = true,
                ToolbarAction::Find => requests.find = true,
                ToolbarAction::Diff => requests.diff = true,
                ToolbarAction::History => requests.history = true,
                ToolbarAction::Delete => requests.delete = true,
                ToolbarAction::More | ToolbarAction::Separator => {}
            }
            // Find moves focus to its query and Delete replaces the editor.
            if !matches!(action, ToolbarAction::Find | ToolbarAction::Delete) {
                requests.preserve_editor_focus |= editor_had_virtual_focus;
            }
        }
        if requests.copy_link || requests.duplicate || requests.export {
            requests.preserve_editor_focus |= editor_had_virtual_focus;
        }
    }

    /// Renders the overflow menu button and, while open, its entries.
    ///
    /// # Returns
    /// Whether the menu button itself was clicked this frame.
    fn render_more_menu(
        &mut self,
        ui: &mut egui::Ui,
        background_mutation_blocked: bool,
        requests: &mut ToolbarRequests,
    ) -> bool {
        let button = toolbar_button(MORE_MENU_LABEL).small();
        let (response, _) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            if ui.add(toolbar_button("Copy Link")).clicked() {
                requests.copy_link = true;
            }
            if ui
                .add_enabled(!background_mutation_blocked, toolbar_button("Duplicate"))
                .clicked()
            {
                requests.duplicate = true;
            }
            if ui.add(toolbar_button("Export")).clicked() {
                requests.export = true;
            }
            if ui.add(toolbar_button("Properties")).clicked() {
                requests.properties = true;
            }
        });
        response.clicked()
    }
}

/// One tag chip: a tag that searches on click, or the `+N` overflow chip.
struct TagChip {
    label: String,
    tag: Option<String>,
    width: f32,
}

/// Chooses the tag chips that fit in `available_width`.
///
/// At most [`MAX_VISIBLE_TAGS`] tags show; any tag that does not fit or is
/// beyond the cap is counted by a trailing `+N` chip.
fn visible_tag_chips(ui: &egui::Ui, tags: &[String], available_width: f32) -> Vec<TagChip> {
    let spacing = ui.spacing().item_spacing.x;
    let overflow_label = |hidden: usize| format!("+{hidden}");
    let mut chips = Vec::new();
    let mut used = 0.0;
    for (index, tag) in tags.iter().enumerate().take(MAX_VISIBLE_TAGS) {
        let label = format!("#{tag}");
        let width = button_width(ui, &label);
        let hidden_after = tags.len() - index - 1;
        let reserve = if hidden_after > 0 {
            button_width(ui, &overflow_label(hidden_after)) + spacing
        } else {
            0.0
        };
        if used + width + spacing + reserve > available_width {
            break;
        }
        used += width + spacing;
        chips.push(TagChip {
            label,
            tag: Some(tag.clone()),
            width,
        });
    }
    let hidden = tags.len() - chips.len();
    if hidden > 0 {
        let label = overflow_label(hidden);
        let width = button_width(ui, &label);
        if used + width <= available_width {
            chips.push(TagChip {
                label,
                tag: None,
                width,
            });
        }
    }
    chips
}

/// Room the one-row layout keeps for tags: the widest `+N` chip and its spacing.
///
/// Reserving it means a paste with tags always shows tag chips or the `+N`
/// chip, rather than losing both just above the two-row threshold.
///
/// # Arguments
/// - `ui`: UI in the toolbar's style.
/// - `tag_count`: Number of tags on the paste.
///
/// # Returns
/// The reserved width, or zero for a paste without tags.
fn overflow_chip_reserve(ui: &egui::Ui, tag_count: usize) -> f32 {
    if tag_count == 0 {
        return 0.0;
    }
    button_width(ui, &format!("+{tag_count}")) + ui.spacing().item_spacing.x
}

/// Width of a toolbar button showing `label` in the current style.
fn button_width(ui: &egui::Ui, label: &str) -> f32 {
    let text = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    text.size().x + 2.0 * ui.spacing().button_padding.x
}

/// Width of the right-aligned action group, including its separator.
fn toolbar_actions_width(ui: &egui::Ui) -> f32 {
    let spacing = ui.spacing().item_spacing.x;
    TOOLBAR_ACTIONS
        .iter()
        .map(|action| match action {
            ToolbarAction::Separator => SEPARATOR_WIDTH + spacing,
            _ => button_width(ui, action.label()) + spacing,
        })
        .sum::<f32>()
}

/// Toolbar buttons stay out of egui's focus ring so clicking one does not take
/// keyboard focus from the editor.
fn toolbar_button(label: impl Into<egui::WidgetText>) -> egui::Button<'static> {
    egui::Button::new(label).sense(non_focusable_click_sense())
}

fn small_toolbar_button(ui: &mut egui::Ui, label: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(toolbar_button(label).small())
}

fn apply_compact_meta_row_style(ui: &mut egui::Ui) {
    let mut compact_style = (**ui.style()).clone();
    for text_style in [egui::TextStyle::Body, egui::TextStyle::Button] {
        if let Some(font) = compact_style.text_styles.get(&text_style).cloned() {
            compact_style.text_styles.insert(
                text_style,
                egui::FontId::new((font.size - 2.0).max(10.0), font.family),
            );
        }
    }
    compact_style.spacing.button_padding = egui::vec2(8.0, 4.0);
    compact_style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    compact_style.spacing.interact_size.y = 28.0;
    ui.set_style(compact_style);
}
