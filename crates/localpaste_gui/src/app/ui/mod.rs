//! UI panel modules extracted from the main app update loop.

use eframe::egui;

/// Requests query focus only when its widget can join the accessibility tree.
///
/// Floating windows first render an invisible sizing pass. Focusing an input in
/// that pass publishes a focus id without a node and crashes native AccessKit.
/// Call before adding the input so a visible pass can accept its current events.
///
/// # Arguments
/// - `ui`: UI pass that will create the query widget.
/// - `id`: Stable id of that query widget.
///
/// # Returns
/// `true` when this visible pass accepted the request.
pub(in crate::app) fn focus_visible_query(ui: &egui::Ui, id: egui::Id) -> bool {
    if !ui.is_visible() || ui.is_sizing_pass() {
        return false;
    }
    ui.memory_mut(|memory| memory.request_focus(id));
    true
}

/// Renders a full-width selectable list row.
///
/// The selection and hover highlight span the row. The label is left-aligned
/// and truncates with an ellipsis; a non-empty `hint` (a shortcut or short
/// description) is right-aligned in the muted text color.
///
/// # Arguments
/// - `ui`: Vertical list UI receiving the row.
/// - `selected`: Whether the row carries the selection highlight.
/// - `label`: Primary row text.
/// - `hint`: Right-aligned secondary text; empty for none.
///
/// # Returns
/// The row's click response.
pub(in crate::app) fn list_row(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    hint: &str,
) -> egui::Response {
    let mut row = egui::Button::selectable(selected, label)
        .min_size(egui::vec2(ui.available_width(), 0.0))
        .truncate();
    if !hint.is_empty() {
        row = row.shortcut_text(hint);
    }
    ui.add(row)
}

/// Command palette modal and quick-action behavior.
pub(super) mod command_palette;
/// Detached diff modal for side-by-side compare operations.
pub(super) mod diff_modal;
/// Standard text editor panel and header controls.
pub(super) mod editor_panel;
/// Virtual preview/editor panel rendering.
pub(super) mod editor_panel_virtual;
/// Detached version-history modal for historical snapshots/reset.
pub(super) mod history_modal;
/// Separate paste discovery and result actions.
pub(super) mod paste_picker;
/// Right-side properties drawer.
pub(super) mod properties_drawer;
/// Shared search-scope controls.
pub(super) mod search_scope;
/// Keyboard shortcut help window.
pub(super) mod shortcut_help;
/// Top bar and left sidebar surfaces.
pub(super) mod sidebar;
/// Bottom status bar content.
pub(super) mod status_bar;
/// Transient toast notifications.
pub(super) mod toasts;
