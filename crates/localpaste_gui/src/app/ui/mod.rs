//! UI panel modules extracted from the main app update loop.

/// Command palette modal and quick-action behavior.
pub(super) mod command_palette;
/// Standard text editor panel and header controls.
pub(super) mod editor_panel;
/// Virtual preview/editor panel rendering.
pub(super) mod editor_panel_virtual;
/// Right-side properties drawer.
pub(super) mod properties_drawer;
/// Keyboard shortcut help window.
pub(super) mod shortcut_help;
/// Top bar and left sidebar surfaces.
pub(super) mod sidebar;
/// Bottom status bar content.
pub(super) mod status_bar;
/// Transient toast notifications.
pub(super) mod toasts;
