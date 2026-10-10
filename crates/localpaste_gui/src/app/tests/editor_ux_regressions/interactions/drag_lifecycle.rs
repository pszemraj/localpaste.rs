//! Rendered drag start, autoscroll, and teardown regressions for the virtual editor.

use super::*;

fn requests_immediate_repaint(output: &egui::FullOutput) -> bool {
    output
        .viewport_output
        .values()
        .any(|viewport| viewport.repaint_delay == Duration::ZERO)
}

#[test]
fn held_drag_past_a_reached_document_edge_stops_repainting() {
    for edge in ["start", "end"] {
        let (mut harness, _event_tx) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        let text = "alpha beta gamma delta omega\n".repeat(160);
        harness.app.reset_virtual_editor(&text);
        for _ in 0..3 {
            review_frame(&mut harness.app, &ctx, vec![]);
        }
        let line_height = harness.app.virtual_line_height;
        if edge == "end" {
            harness.app.virtual_pending_scroll_offset_y = Some(line_height * 10_000.0);
            for _ in 0..3 {
                review_frame(&mut harness.app, &ctx, vec![]);
            }
        }
        let viewport = harness.app.virtual_viewport.rect.unwrap();
        let (start, outside) = if edge == "start" {
            (
                egui::pos2(viewport.left() + 40.0, viewport.top() + line_height * 3.5),
                egui::pos2(viewport.left() + 2.0, -40.0),
            )
        } else {
            (
                egui::pos2(viewport.left() + 40.0, viewport.top() + line_height * 3.5),
                egui::pos2(viewport.left() + 2.0, 940.0),
            )
        };
        review_frame(&mut harness.app, &ctx, primary_pointer_events(start, true));
        review_frame(
            &mut harness.app,
            &ctx,
            vec![egui::Event::PointerMoved(start + egui::vec2(60.0, 0.0))],
        );
        assert!(harness.app.virtual_drag_active, "{edge}");
        review_frame(
            &mut harness.app,
            &ctx,
            vec![egui::Event::PointerMoved(outside)],
        );
        let mut output = review_frame(&mut harness.app, &ctx, vec![]);
        for _ in 0..40 {
            let offset = harness.app.virtual_viewport.offset_y;
            output = review_frame(&mut harness.app, &ctx, vec![]);
            if harness.app.virtual_viewport.offset_y == offset
                && !requests_immediate_repaint(&output)
            {
                break;
            }
        }

        assert!(harness.app.virtual_drag_active, "{edge}");
        assert!(
            !requests_immediate_repaint(&output),
            "a drag held past the {edge} edge must stop repainting once the viewport is there"
        );
        let expected_cursor = if edge == "start" { 0 } else { text.len() };
        assert_eq!(
            harness.app.virtual_editor_state.cursor(),
            expected_cursor,
            "{edge}"
        );
    }
}

#[test]
fn floating_scrollbar_outer_margin_keeps_scrollbar_presses_off_the_rows() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    let text = "alpha beta gamma delta omega\n".repeat(240);
    harness.app.reset_virtual_editor(&text);
    review_frame(&mut harness.app, &ctx, vec![]);
    ctx.all_styles_mut(|style| style.spacing.scroll.bar_outer_margin = 8.0);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness
        .app
        .virtual_editor_state
        .restore_selection(5, Some(24), len);
    let viewport = harness.app.virtual_viewport.rect.unwrap();
    // The hover-expanded bar spans [right - margin - bar_width, right - margin].
    let thumb = egui::pos2(viewport.right() - 14.0, viewport.top() + 5.0);
    for _ in 0..20 {
        review_frame(
            &mut harness.app,
            &ctx,
            vec![egui::Event::PointerMoved(thumb)],
        );
    }
    let offset_before = harness.app.virtual_viewport.offset_y;

    review_frame(&mut harness.app, &ctx, primary_pointer_events(thumb, true));
    let dragged = thumb + egui::vec2(0.0, 140.0);
    review_frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::PointerMoved(dragged)],
    );
    review_frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: dragged,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );

    assert!(
        harness.app.virtual_viewport.offset_y > offset_before,
        "dragging the inset scrollbar thumb should move the viewport"
    );
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(5..24),
        "a press on the inset scrollbar must not become an editor row click"
    );
    assert!(!harness.app.virtual_drag_active);
}
