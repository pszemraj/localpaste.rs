//! Rendered editor toolbar layout and its overflow menu.

use super::super::ui::editor_toolbar::MORE_MENU_LABEL;
use super::*;

const ROW_ACTIONS: [&str; 6] = ["Copy", "Find", "Diff", "History", MORE_MENU_LABEL, "Delete"];
const MENU_ACTIONS: [&str; 4] = ["Copy Link", "Duplicate", "Export", "Properties"];

fn input_at(width: f32) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 800.0),
        )),
        ..Default::default()
    }
}

fn render_at(app: &mut LocalPasteApp, ctx: &egui::Context, width: f32) -> egui::FullOutput {
    let _ = run_full_update_with_input(app, ctx, input_at(width));
    run_full_update_with_input(app, ctx, input_at(width))
}

/// Rendered rect of an exact text label right of the 300-point sidebar.
fn editor_text_rect(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text == label && text.pos.x > 300.0 => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
}

fn click(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    width: f32,
    pos: egui::Pos2,
) -> egui::FullOutput {
    for pressed in [true, false] {
        let _ = run_full_update_with_input(
            app,
            ctx,
            egui::RawInput {
                events: primary_pointer_events(pos, pressed),
                ..input_at(width)
            },
        );
    }
    render_at(app, ctx, width)
}

fn assert_no_overlap(rects: &mut [egui::Rect]) {
    rects.sort_by(|a, b| a.left().total_cmp(&b.left()));
    for pair in rects.windows(2) {
        assert!(pair[0].right() <= pair[1].left(), "overlap: {pair:?}");
    }
}

#[test]
fn toolbar_fits_one_row_at_common_widths() {
    // A maximized 13-inch MacBook Air shows the tags; the default window keeps
    // one row by folding them into the `+N` chip.
    for (width, tag_chips) in [
        (1372.0, ["#infra", "#deploy"].as_slice()),
        (1100.0, &["+2"]),
    ] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        harness.app.edit_tags = "infra, deploy".to_string();
        let output = render_at(&mut harness.app, &ctx, width);

        let mut rects = ["Alpha", "Apply"]
            .into_iter()
            .chain(tag_chips.iter().copied())
            .chain(ROW_ACTIONS)
            .map(|label| {
                editor_text_rect(&output, label)
                    .unwrap_or_else(|| panic!("missing {label} at width {width}"))
            })
            .collect::<Vec<_>>();
        let row_center = rects[0].center().y;
        for rect in &rects {
            assert!(
                (rect.center().y - row_center).abs() < 3.0,
                "{rect:?} left the toolbar row at width {width}"
            );
        }
        let delete = editor_text_rect(&output, "Delete").unwrap();
        assert!(delete.right() > width - 40.0, "actions are right-aligned");
        assert_no_overlap(&mut rects);
        for label in MENU_ACTIONS {
            assert!(
                editor_text_rect(&output, label).is_none(),
                "{label} belongs in the More menu"
            );
        }
    }
}

#[test]
fn narrow_toolbar_moves_actions_to_a_second_row_without_overlap() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    let output = render_at(&mut harness.app, &ctx, 900.0);
    let title = editor_text_rect(&output, "Alpha").expect("title");
    let mut actions = ROW_ACTIONS
        .map(|label| editor_text_rect(&output, label).expect(label))
        .to_vec();
    for rect in &actions {
        assert!(
            rect.top() > title.bottom(),
            "{rect:?} should wrap below the title"
        );
    }
    assert_no_overlap(&mut actions);
}

#[test]
fn tag_chips_cap_at_three_with_an_overflow_count() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.edit_tags = "a, b, c, d, e".to_string();
    let output = render_at(&mut harness.app, &ctx, 1372.0);
    for label in ["#a", "#b", "#c", "+2"] {
        assert!(
            editor_text_rect(&output, label).is_some(),
            "missing {label}"
        );
    }
    assert!(editor_text_rect(&output, "#d").is_none());

    // The overflow chip opens Properties, where every tag is editable.
    let more_tags = editor_text_rect(&output, "+2").unwrap().center();
    click(&mut harness.app, &ctx, 1372.0, more_tags);
    assert!(harness.app.properties_drawer_open);
}

#[test]
fn more_menu_holds_the_secondary_actions() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    let font = egui::FontId::proportional(13.0);
    let output = render_at(&mut harness.app, &ctx, 1372.0);
    assert!(
        ctx.fonts_mut(|fonts| fonts.has_glyphs(&font, MORE_MENU_LABEL)),
        "the More label must render with the bundled fonts"
    );

    let more = editor_text_rect(&output, MORE_MENU_LABEL).unwrap().center();
    let output = click(&mut harness.app, &ctx, 1372.0, more);
    for label in MENU_ACTIONS {
        assert!(
            editor_text_rect(&output, label).is_some(),
            "menu lacks {label}"
        );
    }
    let properties = editor_text_rect(&output, "Properties").unwrap().center();
    let output = click(&mut harness.app, &ctx, 1372.0, properties);
    assert!(harness.app.properties_drawer_open);
    assert!(
        editor_text_rect(&output, "Copy Link").is_none(),
        "choosing an entry closes the menu"
    );
}
