//! Compact scope selector shared by sidebar and paste picker.

use super::super::*;

/// Render search scope choices, returning the newly selected scope.
///
/// # Arguments
/// - `ui`: Parent surface.
/// - `id`: Stable identifier distinguishing sidebar from picker.
/// - `scope`: Current field scope.
///
/// # Returns
/// Selected field scope for this surface.
pub(super) fn scope_selector(ui: &mut egui::Ui, id: &str, scope: SearchScope) -> SearchScope {
    let mut selected = scope;
    let label = |scope| match scope {
        SearchScope::All => "All fields",
        SearchScope::Title => "Title",
        SearchScope::Metadata => "Metadata",
        SearchScope::Body => "Body",
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(scope))
        .show_ui(ui, |ui| {
            for scope in [
                SearchScope::All,
                SearchScope::Title,
                SearchScope::Metadata,
                SearchScope::Body,
            ] {
                ui.selectable_value(&mut selected, scope, label(scope));
            }
        })
        .response
        .on_hover_text("Metadata includes title, tags, language, and derived search terms.");
    selected
}
