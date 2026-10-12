//! UI panel modules extracted from the main app update loop.

use super::style::{modal_chrome_style, COLOR_BG_TERTIARY, COLOR_BORDER, COLOR_TEXT_PRIMARY};
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

/// Gap kept between a viewport-fitted floating window and the viewport edges.
pub(in crate::app) const WINDOW_EDGE_MARGIN: f32 = 12.0;
/// Smallest content area a viewport-fitted window shrinks to.
const WINDOW_MIN_CONTENT: egui::Vec2 = egui::vec2(320.0, 200.0);

/// Sizes a large floating window to its preferred size or the viewport, whichever is smaller.
pub(in crate::app) trait ViewportFitExt: Sized {
    /// Sizes the window to `preferred` when the viewport has room and to the viewport otherwise.
    ///
    /// egui caps a window at the full viewport but ignores its own frame and title
    /// bar, and it remembers the size across frames. A preferred size taller or
    /// wider than the app window, or one remembered from before the app window
    /// shrank, therefore pushes the title bar's close button past the edge. A
    /// `max_size` recomputed from the live viewport on every call shrinks the
    /// remembered size too; scroll areas and truncating labels absorb the
    /// difference. The window is also kept [`WINDOW_EDGE_MARGIN`] clear of the
    /// viewport edges, including when dragged.
    ///
    /// egui shrinks a window to its content, so this size holds only while the
    /// content fills the window, for example with a non-shrinking scroll area
    /// in every column. Content that shrinks makes the window resize, and move
    /// when the edge constraint applies, whenever what it shows changes.
    ///
    /// # Arguments
    /// - `ctx`: Context providing the live viewport and window chrome.
    /// - `preferred`: Content-area size to use when the viewport is large enough.
    ///
    /// # Returns
    /// The window with its default and maximum content size set.
    fn fit_to_viewport(self, ctx: &egui::Context, preferred: egui::Vec2) -> Self;
}

impl ViewportFitExt for egui::Window<'_> {
    fn fit_to_viewport(self, ctx: &egui::Context, preferred: egui::Vec2) -> Self {
        let style = ctx.style();
        let frame = egui::Frame::window(&style);
        // The title bar is at least `interact_size.y` tall and sits above the
        // content area, separated from it by a rule one stroke wide.
        let title_bar =
            style.spacing.interact_size.y + frame.inner_margin.sum().y + frame.stroke.width;
        let chrome = frame.total_margin().sum() + egui::vec2(0.0, title_bar);
        let bounds = ctx.content_rect().shrink(WINDOW_EDGE_MARGIN);
        let max_content = (bounds.size() - chrome).max(WINDOW_MIN_CONTENT);
        self.constrain_to(bounds)
            .default_size(preferred.min(max_content))
            .max_size(max_content)
    }
}

/// Shows a floating window with the title bar every LocalPaste window shares.
pub(in crate::app) trait ModalWindowExt {
    /// Shows the window with a [`COLOR_MODAL_CHROME`](super::style::COLOR_MODAL_CHROME)
    /// title bar and no collapse toggle.
    ///
    /// egui fills the topmost window's title bar from the `open` widget visuals
    /// in effect when `show` starts, so the chrome style is installed only until
    /// the content begins; the content and its popups keep the app's visuals.
    /// A window given an `open` flag keeps its close button.
    ///
    /// # Arguments
    /// - `ctx`: Context the window is shown in.
    /// - `add_contents`: Renders the window content.
    ///
    /// # Returns
    /// egui's window response; `None` while the window is closed.
    fn show_modal<R>(
        self,
        ctx: &egui::Context,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>>;
}

impl ModalWindowExt for egui::Window<'_> {
    fn show_modal<R>(
        self,
        ctx: &egui::Context,
        add_contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
        let app_style = ctx.style();
        ctx.set_style(modal_chrome_style(&app_style));
        let response = self.collapsible(false).show(ctx, |ui| {
            ctx.set_style(app_style.clone());
            ui.set_style(app_style.clone());
            add_contents(ui)
        });
        ctx.set_style(app_style);
        response
    }
}

/// Softens pointer feedback for list rows so hover never reads as selection.
///
/// The theme's orange hover fill suits toolbar buttons, but on a list row it
/// outshines the translucent blue selection fill. Rows hover and press with a
/// neutral fill one surface step above their resting fill and a plain border
/// instead; selected rows keep the selection fill because egui applies it over
/// these visuals.
///
/// # Arguments
/// - `visuals`: Visuals of the scope that renders list rows.
/// - `hover_fill`: Neutral fill one step above the rows' resting surface.
fn soften_list_row_visuals(visuals: &mut egui::Visuals, hover_fill: egui::Color32) {
    for state in [&mut visuals.widgets.hovered, &mut visuals.widgets.active] {
        state.bg_fill = hover_fill;
        state.weak_bg_fill = hover_fill;
        state.bg_stroke = egui::Stroke::new(1.0, COLOR_BORDER);
        state.fg_stroke = egui::Stroke::new(1.0, COLOR_TEXT_PRIMARY);
        state.expansion = 0.0;
    }
}

/// Renders a full-width selectable list row.
///
/// The selection and hover highlight span the row. Selection is the strongest
/// highlight; hover is a neutral fill (see [`soften_list_row_visuals`]) and
/// leaves other buttons' hover untouched. The label is left-aligned and
/// truncates with an ellipsis; a non-empty `hint` (a shortcut or short
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
    ui.scope(|ui| {
        // Unfilled rows rest on the panel, so hover lifts them to the tertiary surface.
        soften_list_row_visuals(ui.visuals_mut(), COLOR_BG_TERTIARY);
        ui.add(row)
    })
    .inner
}

/// Command palette modal and quick-action behavior.
pub(super) mod command_palette;
/// Detached diff modal for side-by-side compare operations.
pub(super) mod diff_modal;
/// Standard text editor panel and header controls.
pub(super) mod editor_panel;
/// Virtual preview/editor panel rendering.
pub(super) mod editor_panel_virtual;
/// Editor toolbar: metadata fields, tag chips, and paste actions.
pub(super) mod editor_toolbar;
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
