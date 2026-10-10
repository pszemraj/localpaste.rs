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
    label_rects(output, label, |pos| pos.x > 300.0)
        .into_iter()
        .next()
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

/// Rendered text rects right of the sidebar whose text satisfies `pick`.
fn editor_texts_where(output: &egui::FullOutput, pick: impl Fn(&str) -> bool) -> Vec<egui::Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.pos.x > 300.0 && pick(&text.galley.job.text) => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn tags_always_show_a_chip_or_overflow_count_clear_of_the_actions() {
    let is_chip = |text: &str| {
        text.starts_with('#')
            || text.strip_prefix('+').is_some_and(|count| {
                !count.is_empty() && count.chars().all(|ch| ch.is_ascii_digit())
            })
    };
    // Toolbar buttons pad their text by 8 points on each side.
    let button_padding = 8.0;
    for tags in ["infra", "infra, deploy, backend, staging, urgent"] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        harness.app.edit_tags = tags.to_string();
        for width in (MIN_WINDOW_SIZE[0] as u32..=1400).step_by(10) {
            let width = width as f32;
            let output = render_at(&mut harness.app, &ctx, width);
            let chips = editor_texts_where(&output, is_chip);
            assert!(!chips.is_empty(), "tags {tags:?} vanished at width {width}");

            let actions = editor_texts_where(&output, |text| ROW_ACTIONS.contains(&text));
            let actions_left = actions
                .iter()
                .map(|rect| rect.left())
                .fold(f32::INFINITY, f32::min);
            let action_row = actions[0].center().y;
            for label in chips.iter().chain(
                ["Alpha", "Apply"]
                    .iter()
                    .filter_map(|label| editor_text_rect(&output, label))
                    .collect::<Vec<_>>()
                    .iter(),
            ) {
                if (label.center().y - action_row).abs() < 3.0 {
                    assert!(
                        label.right() + 2.0 * button_padding <= actions_left,
                        "{label:?} overlaps the actions at width {width} with tags {tags:?}"
                    );
                }
            }
        }
    }
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

/// Rects of every rendered `label` text right of the sidebar, left to right.
fn editor_text_rects(output: &egui::FullOutput, label: &str) -> Vec<egui::Rect> {
    let mut rects = label_rects(output, label, |pos| pos.x > 300.0);
    rects.sort_by(|a, b| a.left().total_cmp(&b.left()));
    rects
}

/// Frame width shared by the language combo tests, wide enough to show the Properties drawer.
const LANGUAGE_COMBO_FRAME_WIDTH: f32 = 1372.0;
/// Seconds between keystrokes of one type-ahead burst.
const KEYSTROKE_GAP: f64 = 0.2;
/// A pause longer than the one-second type-ahead reset, so the next key starts a new buffer.
const TYPEAHEAD_PAUSE: f64 = 1.5;

/// Types each character of `keys` in its own frame, then lets the frame settle.
///
/// The first key arrives `first_gap` seconds after the latest frame and the rest
/// follow [`KEYSTROKE_GAP`] apart, all on egui's input clock rather than the wall clock.
fn type_keys(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    keys: &str,
    first_gap: f64,
) -> egui::FullOutput {
    let mut gap = first_gap;
    let mut output = None;
    for key in keys.chars() {
        let time = ctx.input(|input| input.time) + gap;
        let _ = run_full_update_with_input(
            app,
            ctx,
            egui::RawInput {
                events: vec![egui::Event::Text(key.to_string())],
                time: Some(time),
                ..input_at(LANGUAGE_COMBO_FRAME_WIDTH)
            },
        );
        output = Some(render_at(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH));
        gap = KEYSTROKE_GAP;
    }
    output.expect("at least one key")
}

/// Where each language combo renders its selected text while auto-detect is active.
#[derive(Clone, Copy)]
enum LanguageCombo {
    Toolbar,
    Drawer,
}

/// Renders the app and returns the center of `combo`'s `Auto` selected text.
fn auto_combo_center(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    combo: LanguageCombo,
) -> egui::Pos2 {
    let output = render_at(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH);
    let rects = editor_text_rects(&output, "Auto");
    let rect = match combo {
        LanguageCombo::Toolbar => rects.first(),
        LanguageCombo::Drawer => rects.last(),
    };
    rect.expect("combo selected text").center()
}

/// Rendered rects of the popup option `label`, excluding combo buttons above `combo_center`.
fn popup_option_rects(
    output: &egui::FullOutput,
    label: &str,
    combo_center: egui::Pos2,
) -> Vec<egui::Rect> {
    editor_text_rects(output, label)
        .into_iter()
        .filter(|rect| rect.top() > combo_center.y + 10.0)
        .collect()
}

/// Renders enough frames for egui's animated popup scroll to finish.
fn settle_popup_scroll(app: &mut LocalPasteApp, ctx: &egui::Context) -> egui::FullOutput {
    let mut output = render_at(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH);
    for _ in 0..10 {
        output = render_at(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH);
    }
    output
}

/// Shows `combo` with auto-detect active, opens its popup, and returns the combo center.
fn open_language_combo(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    combo: LanguageCombo,
) -> egui::Pos2 {
    app.properties_drawer_open = matches!(combo, LanguageCombo::Drawer);
    let combo_center = auto_combo_center(app, ctx, combo);
    assert!(!app.edit_language_is_manual && !app.metadata_dirty);
    let opened = click(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH, combo_center);
    assert!(
        !popup_option_rects(&opened, "Auto", combo_center).is_empty(),
        "the language popup is open"
    );
    combo_center
}

/// Types prefixes into `combo`'s open popup and asserts each lands on its language.
fn assert_prefix_typing_selects_languages(combo: LanguageCombo) {
    let mut harness = make_app();
    let app = &mut harness.app;
    let ctx = egui::Context::default();
    let combo_center = open_language_combo(app, &ctx, combo);
    assert!(
        popup_option_rects(
            &render_at(app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH),
            "Python",
            combo_center
        )
        .is_empty(),
        "Python starts below the popup's visible rows"
    );

    let _ = type_keys(app, &ctx, "py", TYPEAHEAD_PAUSE);
    assert!(app.edit_language_is_manual);
    assert_eq!(app.edit_language.as_deref(), Some("python"));
    assert!(
        app.metadata_dirty,
        "a type-ahead jump dirties metadata like a click"
    );
    let settled = settle_popup_scroll(app, &ctx);
    assert!(
        !popup_option_rects(&settled, "Python", combo_center).is_empty(),
        "the popup scrolled the Python option into view"
    );

    // Spelling a name out stays on it instead of walking through other first letters.
    for (keys, expected) in [
        ("python", Some("python")),
        ("ja", Some("java")),
        ("jav", Some("java")),
        ("javas", Some("javascript")),
        ("sh", Some("shell")),
        ("sq", Some("sql")),
        ("c#", Some("cs")),
        ("c+", Some("cpp")),
        ("pl", Some("text")),
        // Nothing matches, so the choice stays where it was.
        ("q", Some("text")),
        ("plx", Some("text")),
        ("a", None),
    ] {
        let _ = type_keys(app, &ctx, keys, TYPEAHEAD_PAUSE);
        assert_eq!(app.edit_language.as_deref(), expected, "typed {keys:?}");
        assert_eq!(
            app.edit_language_is_manual,
            expected.is_some(),
            "typed {keys:?}"
        );
    }
}

#[test]
fn toolbar_language_popup_type_ahead_matches_typed_prefixes() {
    assert_prefix_typing_selects_languages(LanguageCombo::Toolbar);
}

#[test]
fn drawer_language_popup_type_ahead_matches_typed_prefixes() {
    assert_prefix_typing_selects_languages(LanguageCombo::Drawer);
}

#[test]
fn repeating_a_letter_cycles_through_the_languages_starting_with_it() {
    for combo in [LanguageCombo::Toolbar, LanguageCombo::Drawer] {
        let mut harness = make_app();
        let app = &mut harness.app;
        let ctx = egui::Context::default();
        let _ = open_language_combo(app, &ctx, combo);

        let _ = type_keys(app, &ctx, "p", TYPEAHEAD_PAUSE);
        assert_eq!(app.edit_language.as_deref(), Some("perl"));
        for expected in ["text", "powershell", "python", "perl", "text"] {
            let _ = type_keys(app, &ctx, "p", KEYSTROKE_GAP);
            assert_eq!(app.edit_language.as_deref(), Some(expected));
        }
    }
}

#[test]
fn language_type_ahead_buffer_resets_after_a_pause() {
    let mut harness = make_app();
    let app = &mut harness.app;
    let ctx = egui::Context::default();
    let _ = open_language_combo(app, &ctx, LanguageCombo::Toolbar);

    let _ = type_keys(app, &ctx, "p", TYPEAHEAD_PAUSE);
    let _ = type_keys(app, &ctx, "y", 0.9);
    assert_eq!(
        app.edit_language.as_deref(),
        Some("python"),
        "0.9 s keeps the buffer"
    );

    let _ = type_keys(app, &ctx, "p", TYPEAHEAD_PAUSE);
    assert_eq!(
        app.edit_language.as_deref(),
        Some("perl"),
        "a fresh `p` starts at Perl"
    );
    let _ = type_keys(app, &ctx, "y", TYPEAHEAD_PAUSE);
    assert_eq!(
        app.edit_language.as_deref(),
        Some("yaml"),
        "a pause starts a new buffer"
    );
}

#[test]
fn typing_with_language_combos_closed_keeps_the_choice() {
    let mut harness = make_app();
    let app = &mut harness.app;
    let ctx = egui::Context::default();
    app.properties_drawer_open = true;
    let _ = render_at(app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH);

    let _ = type_keys(app, &ctx, "m", TYPEAHEAD_PAUSE);
    assert!(!app.edit_language_is_manual);
    assert_eq!(app.edit_language, None);
    assert!(!app.metadata_dirty);

    let toolbar_combo = auto_combo_center(app, &ctx, LanguageCombo::Toolbar);
    let _ = click(app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH, toolbar_combo);
    let _ = type_keys(app, &ctx, "p", TYPEAHEAD_PAUSE);
    assert_eq!(app.edit_language.as_deref(), Some("perl"));
    let escape = |app: &mut LocalPasteApp| {
        let _ = run_full_update_with_input(
            app,
            &ctx,
            egui::RawInput {
                events: vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
                ..input_at(LANGUAGE_COMBO_FRAME_WIDTH)
            },
        );
    };
    escape(app);
    let _ = type_keys(app, &ctx, "m", KEYSTROKE_GAP);
    assert_eq!(
        app.edit_language.as_deref(),
        Some("perl"),
        "a closed popup ignores letters"
    );

    // Reopening within a second still starts a new buffer: `y` alone is YAML, not `py`.
    let perl_combo = editor_text_rects(&render_at(app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH), "Perl")
        .first()
        .expect("toolbar combo shows Perl")
        .center();
    let _ = click(app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH, perl_combo);
    let _ = type_keys(app, &ctx, "y", KEYSTROKE_GAP);
    assert_eq!(app.edit_language.as_deref(), Some("yaml"));
    escape(app);
}

#[test]
fn language_letter_jump_does_not_type_into_previously_focused_fields() {
    for focused in [TITLE_INPUT_ID, VIRTUAL_EDITOR_ID] {
        let mut harness = make_app();
        let app = &mut harness.app;
        let ctx = egui::Context::default();
        let combo_center = auto_combo_center(app, &ctx, LanguageCombo::Toolbar);
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(focused)));
        let _ = render_at(app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH);
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(focused))));

        let _ = click(app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH, combo_center);
        let _ = type_keys(app, &ctx, "m", TYPEAHEAD_PAUSE);
        assert_eq!(app.edit_language.as_deref(), Some("markdown"), "{focused}");
        assert_eq!(
            app.edit_name, "Alpha",
            "{focused} must not receive the letter"
        );
        assert_eq!(
            app.virtual_editor_buffer.to_string(),
            "content",
            "{focused} must not receive the letter"
        );
    }
}
