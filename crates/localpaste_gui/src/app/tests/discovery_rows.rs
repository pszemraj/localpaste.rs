//! Row layout of the command palette and paste picker lists.

use super::*;

fn discovery_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 800.0),
        )),
        ..Default::default()
    }
}

/// Renders twice so floating windows finish their sizing pass.
fn render_settled(app: &mut LocalPasteApp, ctx: &egui::Context) -> egui::FullOutput {
    let _ = run_full_update_with_input(app, ctx, discovery_input());
    run_full_update_with_input(app, ctx, discovery_input())
}

fn text_shapes<'a>(
    output: &'a egui::FullOutput,
    text: &'a str,
) -> impl Iterator<Item = &'a egui::epaint::TextShape> + 'a {
    output
        .shapes
        .iter()
        .filter_map(move |clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.job.text == text => Some(shape),
            _ => None,
        })
}

fn text_rect(shape: &egui::epaint::TextShape) -> egui::Rect {
    egui::Rect::from_min_size(shape.pos, shape.galley.size())
}

fn first_text_color(shape: &egui::epaint::TextShape) -> egui::Color32 {
    shape
        .galley
        .job
        .sections
        .first()
        .map_or(shape.fallback_color, |section| section.format.color)
}

#[test]
fn palette_rows_span_the_list_with_muted_right_aligned_hints() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.command_palette_open = true;
    let output = render_settled(&mut harness.app, &ctx);

    let shortcut = shortcuts::runtime_shortcut_label(RuntimeShortcutAction::NewPaste);
    let label = text_rect(text_shapes(&output, "New paste").next().expect("label"));
    let hint_shape = text_shapes(&output, &shortcut)
        .next()
        .expect("shortcut hint");
    let hint = text_rect(hint_shape);
    assert_eq!(first_text_color(hint_shape), COLOR_TEXT_MUTED);
    assert!(
        hint.left() - label.right() > 200.0,
        "hint {hint:?} should sit at the far end of the row from {label:?}"
    );
    assert!(
        text_shapes(&output, &format!("New paste  ({shortcut})"))
            .next()
            .is_none(),
        "shortcuts are no longer inlined into the label"
    );

    // The selected row's highlight is one filled rect behind both label and hint.
    assert!(
        output.shapes.iter().any(|clipped| matches!(
            &clipped.shape,
            egui::Shape::Rect(rect) if rect.fill.a() > 0
                && rect.rect.contains(label.center())
                && rect.rect.contains(hint.center())
        )),
        "selected row highlight should span from {label:?} to {hint:?}"
    );
}

#[test]
fn picker_actions_share_a_right_aligned_column_and_long_names_truncate() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    let long_name = "an extremely long paste name ".repeat(8);
    harness.app.all_pastes = vec![
        test_summary("short", "Short", Some("rust"), 10),
        test_summary("long", long_name.trim(), None, 10),
    ];
    // Without an open paste the editor toolbar's own Copy button is absent.
    harness.app.selected_id = None;
    harness.app.selected_paste = None;
    harness.app.open_paste_picker();
    let output = render_settled(&mut harness.app, &ctx);

    // The sidebar (300 points wide) has its own Delete button.
    let in_picker = |shape: &&egui::epaint::TextShape| shape.pos.x > 300.0;
    for action in ["Copy", "Copy Fenced", "Delete"] {
        let lefts = text_shapes(&output, action)
            .filter(in_picker)
            .map(|shape| shape.pos.x)
            .collect::<Vec<_>>();
        assert_eq!(lefts.len(), 2, "{action} rendered {lefts:?}");
        assert!(
            (lefts[0] - lefts[1]).abs() < 0.5,
            "{action} buttons should align: {lefts:?}"
        );
    }
    let long = text_shapes(&output, long_name.trim())
        .next()
        .expect("long name");
    assert!(long.galley.elided, "long names truncate with an ellipsis");
    let window = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("paste_picker_modal")))
        .expect("picker window");
    assert!(
        discovery_input().screen_rect.unwrap().contains_rect(window),
        "full-width rows must not inflate the picker past the screen: {window:?}"
    );
    let copy_left = text_shapes(&output, "Copy")
        .map(|shape| shape.pos.x)
        .fold(f32::INFINITY, f32::min);
    assert!(text_rect(long).right() < copy_left);
    for language in ["rust", "auto"] {
        let shape = text_shapes(&output, language).next().expect(language);
        assert_eq!(first_text_color(shape), COLOR_TEXT_MUTED);
    }
}

/// Fills of every rectangle drawn behind `point`.
fn fills_behind(output: &egui::FullOutput, point: egui::Pos2) -> Vec<egui::Color32> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) if rect.rect.contains(point) => Some(rect.fill),
            _ => None,
        })
        .collect()
}

/// Settles the palette, then rests the pointer on the center of `label`.
fn hover_palette_label(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    label: &str,
) -> (egui::FullOutput, egui::Pos2) {
    let settled = render_settled(app, ctx);
    let center = text_rect(text_shapes(&settled, label).next().expect("row label")).center();
    let hover = || egui::RawInput {
        events: vec![egui::Event::PointerMoved(center)],
        ..discovery_input()
    };
    // Hover resolves against the previous frame's widget rects, and the
    // window's fade-in must finish so fills are fully opaque.
    let mut output = settled;
    for _ in 0..12 {
        output = run_full_update_with_input(app, ctx, hover());
    }
    (output, center)
}

#[test]
fn palette_hover_is_neutral_and_weaker_than_the_selected_row() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.command_palette_open = true;
    // The first row is selected, so the second is free to carry only hover.
    let (output, center) = hover_palette_label(&mut harness.app, &ctx, "Paste as new paste");
    let fills = fills_behind(&output, center);
    assert!(
        fills.contains(&COLOR_BG_TERTIARY),
        "hovered row should use the neutral fill: {fills:?}"
    );
    assert!(
        !fills.contains(&COLOR_ACCENT_SURFACE_HOVER) && !fills.contains(&COLOR_ACCENT_SURFACE),
        "list hover must not use the toolbar accent: {fills:?}"
    );
    assert_eq!(
        harness.app.command_palette_selected, 0,
        "hover keeps selection"
    );

    // Hovering the selected row leaves it at the selection fill.
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.command_palette_open = true;
    let (output, center) = hover_palette_label(&mut harness.app, &ctx, "New paste");
    let fills = fills_behind(&output, center);
    let selection = ctx.style().visuals.selection.bg_fill;
    assert!(
        fills.contains(&selection),
        "hovered selected row keeps the selection fill: {fills:?}"
    );
    assert!(
        !fills.contains(&COLOR_BG_TERTIARY) && !fills.contains(&COLOR_ACCENT_SURFACE_HOVER),
        "hover must not replace the selection fill: {fills:?}"
    );
}

#[test]
fn list_hover_scope_leaves_toolbar_button_hover_accented() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.command_palette_open = true;
    let _ = render_settled(&mut harness.app, &ctx);
    let hovered = ctx.style().visuals.widgets.hovered;
    assert_eq!(hovered.weak_bg_fill, COLOR_ACCENT_SURFACE_HOVER);
    assert_eq!(hovered.bg_fill, COLOR_ACCENT_SURFACE_HOVER);
}
