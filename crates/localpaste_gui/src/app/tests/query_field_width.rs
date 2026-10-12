//! Query fields of the discovery and diff windows reach their window's content edge.

use super::*;

/// Renders enough frames for floating windows to finish sizing and fading in.
fn settle(app: &mut LocalPasteApp, ctx: &egui::Context) {
    for _ in 0..4 {
        run_full_update(app, ctx, vec![]);
    }
}

/// Asserts the query field is wider than the default text-edit width and ends at the
/// window's content edge (window margin plus stroke).
fn assert_field_reaches_window_edge(ctx: &egui::Context, field_id: &str, window_id: egui::Id) {
    let field = ctx
        .read_response(egui::Id::new(field_id))
        .expect("query field")
        .rect;
    let window = ctx
        .memory(|memory| memory.area_rect(window_id))
        .expect("window");
    assert!(
        field.width() > 400.0,
        "{field_id} should fill its row, not the default 280 points: {field:?}"
    );
    assert!(
        window.right() - field.right() < 20.0,
        "{field_id} should end at the window content edge: field {field:?}, window {window:?}"
    );
}

#[test]
fn command_palette_query_fills_the_dialog() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.command_palette_open = true;
    settle(&mut harness.app, &ctx);
    assert_field_reaches_window_edge(
        &ctx,
        COMMAND_PALETTE_INPUT_ID,
        egui::Id::new("command_palette_modal"),
    );
}

#[test]
fn paste_picker_query_fills_the_dialog() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.open_paste_picker();
    settle(&mut harness.app, &ctx);
    assert_field_reaches_window_edge(
        &ctx,
        PASTE_PICKER_INPUT_ID,
        egui::Id::new("paste_picker_modal"),
    );
}

#[test]
fn diff_query_fills_the_rest_of_its_row() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.open_diff_modal();
    assert!(harness.app.version_ui.diff_modal_open);
    settle(&mut harness.app, &ctx);
    assert_field_reaches_window_edge(&ctx, DIFF_QUERY_INPUT_ID, egui::Id::new("Diff"));
}
