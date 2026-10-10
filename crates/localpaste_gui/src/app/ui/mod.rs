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
