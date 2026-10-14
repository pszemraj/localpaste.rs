//! Compact scope selector shared by sidebar and paste picker.

use super::super::*;

const SCOPES: [SearchScope; 4] = [
    SearchScope::All,
    SearchScope::Title,
    SearchScope::Metadata,
    SearchScope::Body,
];

/// Render search scope choices, returning the newly selected scope.
///
/// While the menu is open it owns the navigation keys: Up/Down select the
/// neighbouring scope, and Enter or Escape closes the menu keeping the choice.
/// They are consumed, so the surface around the menu, such as the picker's
/// result list, does not also act on them.
///
/// # Arguments
/// - `ui`: Parent surface.
/// - `id`: Stable identifier distinguishing sidebar from picker.
/// - `scope`: Current field scope.
/// - `width`: Outer combo width.
///
/// # Returns
/// Selected field scope for this surface.
pub(super) fn scope_selector(
    ui: &mut egui::Ui,
    id: &str,
    scope: SearchScope,
    width: f32,
) -> SearchScope {
    // `ComboBox::from_id_salt` derives its widget id this way.
    let combo_id = ui.make_persistent_id(egui::IdSalt::new(id));
    let (step, close) = if egui::ComboBox::is_open(ui.ctx(), combo_id) {
        ui.input_mut(|input| {
            let down = input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
            let up = input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
            let enter = input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
            let escape = input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            (isize::from(down) - isize::from(up), enter || escape)
        })
    } else {
        (0, false)
    };
    // A step past either end keeps the current scope.
    let mut selected = SCOPES
        .iter()
        .position(|choice| *choice == scope)
        .and_then(|index| index.checked_add_signed(step))
        .and_then(|index| SCOPES.get(index))
        .copied()
        .unwrap_or(scope);
    let label = |scope| match scope {
        SearchScope::All => "All fields",
        SearchScope::Title => "Title",
        SearchScope::Metadata => "Metadata",
        SearchScope::Body => "Body",
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(selected))
        .width(width)
        .truncate()
        .show_ui(ui, |ui| {
            if close {
                ui.close();
            }
            for scope in SCOPES {
                ui.selectable_value(&mut selected, scope, label(scope));
            }
        })
        .response
        .on_hover_text(
            "Fields to search. Metadata includes title, tags, language, and derived search terms.",
        );
    selected
}
