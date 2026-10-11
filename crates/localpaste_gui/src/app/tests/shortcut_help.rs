//! Shortcut-help focus ownership and dismissal regressions.

use super::*;

const SHORTCUT_HELP_QUERY_ID: &str = "shortcut_help_query";
const SHORTCUT_HELP_WINDOW_ID: &str = "Keyboard Shortcuts";

fn focus_editor(ctx: &egui::Context) {
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(VIRTUAL_EDITOR_ID)));
}

fn shortcut_help_viewport() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0))
}

fn shortcut_help_input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(shortcut_help_viewport()),
        events,
        ..Default::default()
    }
}

fn check_shortcut_help_toolbar_focus(query: &str, editor: bool) {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor("before selection after");
    harness
        .app
        .virtual_editor_state
        .restore_selection(16, Some(7), 22);
    let origin = egui::Id::new(if editor {
        VIRTUAL_EDITOR_ID
    } else {
        SEARCH_INPUT_ID
    });
    ctx.memory_mut(|memory| memory.request_focus(origin));
    for _ in 0..3 {
        run_full_update_with_input(&mut harness.app, &ctx, shortcut_help_input(Vec::new()));
    }

    if !query.is_empty() {
        run_full_update_with_input(
            &mut harness.app,
            &ctx,
            shortcut_help_input(vec![key_event(egui::Key::F1, egui::Modifiers::NONE)]),
        );
        run_full_update_with_input(
            &mut harness.app,
            &ctx,
            shortcut_help_input(vec![egui::Event::Text(query.into())]),
        );
        run_full_update_with_input(
            &mut harness.app,
            &ctx,
            shortcut_help_input(vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)]),
        );
    }
    let output =
        run_full_update_with_input(&mut harness.app, &ctx, shortcut_help_input(Vec::new()));
    let button = rendered_label_center(&output, "Shortcuts (F1)");
    for pressed in [true, false] {
        run_full_update_with_input(
            &mut harness.app,
            &ctx,
            shortcut_help_input(primary_pointer_events(button, pressed)),
        );
    }
    for _ in 0..3 {
        run_full_update_with_input(&mut harness.app, &ctx, shortcut_help_input(Vec::new()));
    }
    assert!(harness.app.shortcut_help_open);
    assert!(
        ctx.memory(|memory| memory.has_focus(egui::Id::new(SHORTCUT_HELP_QUERY_ID))),
        "toolbar-opened help query should focus with retained query {query:?}"
    );
    assert_eq!(harness.app.shortcut_help_query, query);
    run_full_update_with_input(
        &mut harness.app,
        &ctx,
        shortcut_help_input(vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)]),
    );
    assert!(
        ctx.memory(|memory| memory.has_focus(origin)),
        "Escape should restore toolbar opener's focus"
    );
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(7..16)
    );
    run_full_update_with_input(
        &mut harness.app,
        &ctx,
        shortcut_help_input(vec![egui::Event::Text("replacement".into())]),
    );
    if editor {
        assert_eq!(harness.app.active_snapshot(), "before replacement after");
    } else {
        assert_eq!(harness.app.search_query, "replacement");
        assert_eq!(harness.app.active_snapshot(), "before selection after");
    }
}

#[test]
fn shortcut_help_toolbar_restores_editor_selection_on_escape() {
    check_shortcut_help_toolbar_focus("", true);
}

#[test]
fn shortcut_help_toolbar_reopening_focuses_retained_query() {
    check_shortcut_help_toolbar_focus("picker", true);
}

#[test]
fn shortcut_help_toolbar_returns_to_sidebar_search_on_first_and_repeated_open() {
    for query in ["", "picker"] {
        check_shortcut_help_toolbar_focus(query, false);
    }
}

fn settled_shortcut_help_rect(query: &str) -> egui::Rect {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    run_full_update_with_input(
        &mut harness.app,
        &ctx,
        shortcut_help_input(vec![key_event(egui::Key::F1, egui::Modifiers::NONE)]),
    );
    if !query.is_empty() {
        run_full_update_with_input(
            &mut harness.app,
            &ctx,
            shortcut_help_input(vec![egui::Event::Text(query.into())]),
        );
    }
    for _ in 0..3 {
        run_full_update_with_input(&mut harness.app, &ctx, shortcut_help_input(Vec::new()));
    }
    ctx.memory(|memory| {
        memory
            .area_rect(egui::Id::new(SHORTCUT_HELP_WINDOW_ID))
            .expect("shortcut help window should have a settled area")
    })
}

#[test]
fn shortcut_help_escape_restores_editor_focus_and_selection() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("before selection after");
    let text_len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(7, text_len);
    harness
        .app
        .virtual_editor_state
        .move_cursor(16, text_len, true);
    let selection_before = harness.app.virtual_editor_state.selection_range();

    let ctx = egui::Context::default();
    focus_editor(&ctx);
    run_full_update(&mut harness.app, &ctx, Vec::new());
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
    );
    assert!(harness.app.shortcut_help_open);
    assert!(harness.app.shortcut_help_focus_requested);
    assert!(!ctx.memory(|memory| memory.has_focus(egui::Id::new(SHORTCUT_HELP_QUERY_ID))));
    // A cold floating window must finish its invisible sizing pass first.
    run_full_update(&mut harness.app, &ctx, Vec::new());
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(SHORTCUT_HELP_QUERY_ID))));

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("undo".into())],
    );
    assert_eq!(harness.app.shortcut_help_query, "undo");
    assert_eq!(harness.app.active_snapshot(), "before selection after");
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        selection_before
    );

    // Clicking Shortcuts again must not replace the original editor focus owner.
    harness.app.open_shortcut_help(&ctx);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!harness.app.shortcut_help_open);
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        selection_before
    );

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("replacement".into())],
    );
    assert_eq!(harness.app.active_snapshot(), "before replacement after");
}

#[test]
fn shortcut_help_f1_toggle_restores_editor_focus() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("alpha beta");
    let text_len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(6, text_len);
    harness
        .app
        .virtual_editor_state
        .move_cursor(10, text_len, true);

    let ctx = egui::Context::default();
    focus_editor(&ctx);
    run_full_update(&mut harness.app, &ctx, Vec::new());

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
    );
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("paste".into())],
    );
    assert_eq!(harness.app.shortcut_help_query, "paste");

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
    );
    assert!(!harness.app.shortcut_help_open);
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("replacement".into())],
    );
    assert_eq!(harness.app.active_snapshot(), "alpha replacement");
}

#[test]
fn shortcut_help_escape_restores_sidebar_search_focus_without_editor_mutation() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.search_focus_requested = true;
    run_full_update(&mut harness.app, &ctx, Vec::new());
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(SEARCH_INPUT_ID))));

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("tag".into())],
    );
    assert_eq!(harness.app.search_query, "tag");

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
    );
    // A cold floating window must finish its invisible sizing pass first.
    run_full_update(&mut harness.app, &ctx, Vec::new());
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(SHORTCUT_HELP_QUERY_ID))));
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("shortcut".into())],
    );

    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(SEARCH_INPUT_ID))));
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text(" search".into())],
    );
    assert_eq!(harness.app.search_query, "tag search");
    assert_eq!(harness.app.active_snapshot(), "content");
}

#[test]
fn shortcut_help_escape_leaves_detached_version_overlay_open() {
    #[derive(Clone, Copy, Debug)]
    enum VersionOverlay {
        History,
        Diff,
    }

    for overlay in [VersionOverlay::History, VersionOverlay::Diff] {
        let mut harness = make_app();
        match overlay {
            VersionOverlay::History => harness.app.version_ui.history_modal_open = true,
            VersionOverlay::Diff => harness.app.version_ui.diff_modal_open = true,
        }
        let selected_before = harness.app.selected_id.clone();
        let ctx = egui::Context::default();

        run_full_update(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
        );
        assert!(harness.app.shortcut_help_open, "{overlay:?}");
        // A cold floating window must finish its invisible sizing pass first.
        run_full_update(&mut harness.app, &ctx, Vec::new());
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(SHORTCUT_HELP_QUERY_ID))));

        run_full_update(
            &mut harness.app,
            &ctx,
            vec![egui::Event::Text("undo".into())],
        );
        assert_eq!(harness.app.shortcut_help_query, "undo", "{overlay:?}");

        run_full_update(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(!harness.app.shortcut_help_open, "{overlay:?}");
        assert_eq!(harness.app.selected_id, selected_before, "{overlay:?}");
        match overlay {
            VersionOverlay::History => assert!(harness.app.version_ui.history_modal_open),
            VersionOverlay::Diff => assert!(harness.app.version_ui.diff_modal_open),
        }
    }
}

#[test]
fn shortcut_help_window_geometry_is_stable_across_search_results() {
    let empty = settled_shortcut_help_rect("");
    let word = settled_shortcut_help_rect("word");
    let unmatched = settled_shortcut_help_rect("no such shortcut");
    let viewport = shortcut_help_viewport();

    for (label, rect) in [("empty", empty), ("word", word), ("unmatched", unmatched)] {
        assert!(
            viewport.contains_rect(rect),
            "{label} help window should stay within the supported viewport: {rect:?}"
        );
        assert!(
            (rect.width() - empty.width()).abs() <= 0.5,
            "{label} help width changed: empty={empty:?}, current={rect:?}"
        );
        assert!(
            (rect.height() - empty.height()).abs() <= 0.5,
            "{label} help height changed: empty={empty:?}, current={rect:?}"
        );
    }
}

#[test]
fn shortcut_help_lists_the_palette_alias_on_its_primary_row() {
    let macos = cfg!(target_os = "macos");
    let keys = |label| super::super::shortcuts::platform_shortcut_keys(label, macos).unwrap();
    let (primary, alias) = (keys("Ctrl/Cmd+K"), keys("Ctrl/Cmd+Shift+P"));
    for query in ["", "palette", primary.as_str(), alias.as_str()] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        run_full_update_with_input(
            &mut harness.app,
            &ctx,
            shortcut_help_input(vec![key_event(egui::Key::F1, egui::Modifiers::NONE)]),
        );
        run_full_update_with_input(
            &mut harness.app,
            &ctx,
            shortcut_help_input(vec![egui::Event::Text(query.into())]),
        );
        let mut output =
            run_full_update_with_input(&mut harness.app, &ctx, shortcut_help_input(Vec::new()));
        for _ in 0..3 {
            output =
                run_full_update_with_input(&mut harness.app, &ctx, shortcut_help_input(Vec::new()));
        }
        // Key labels are the only monospace text; the query field shows the query too.
        let texts = |keep: &dyn Fn(&str, &egui::FontFamily) -> bool| {
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text)
                        if keep(
                            &text.galley.job.text,
                            &text.galley.job.sections[0].format.font_id.family,
                        ) =>
                    {
                        Some(text.galley.job.text.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            texts(&|text, _| text.starts_with("Toggle command palette")),
            [format!("Toggle command palette  also {alias}")],
            "query {query:?} should show one palette row naming the alias"
        );
        assert_eq!(
            texts(&|text, family| *family == egui::FontFamily::Monospace
                && (text == primary || text == alias)),
            std::slice::from_ref(&primary),
            "query {query:?} should key the row by the primary chord"
        );
    }
}

/// Where help draws its first section title, `App actions`, while it is scrolled into view.
fn first_section_title(output: &egui::FullOutput) -> Option<egui::Rect> {
    match label_rects(output, "App actions", |_| true).as_slice() {
        [] => None,
        [rect] => Some(*rect),
        rects => panic!("expected one App actions title, found {rects:?}"),
    }
}

#[test]
fn shortcut_help_reopens_at_the_top_of_its_list() {
    for (close, restart) in [
        (egui::Key::Escape, false),
        (egui::Key::F1, false),
        (egui::Key::Escape, true),
    ] {
        let what = format!("closed with {close:?}, restarted: {restart}");
        let mut harness = make_app();
        let ctx = egui::Context::default();
        // Half-second steps let egui's smooth wheel scrolling finish between frames.
        let mut time = 0.0;
        let mut frame = |app: &mut LocalPasteApp, events: Vec<egui::Event>| {
            time += 0.5;
            run_full_update_with_input(
                app,
                &ctx,
                egui::RawInput {
                    time: Some(time),
                    ..shortcut_help_input(events)
                },
            )
        };
        let f1 = || vec![key_event(egui::Key::F1, egui::Modifiers::NONE)];

        frame(&mut harness.app, f1());
        let mut output = frame(&mut harness.app, Vec::new());
        for _ in 0..3 {
            output = frame(&mut harness.app, Vec::new());
        }
        let top = first_section_title(&output)
            .unwrap_or_else(|| panic!("{what}: help should open showing App actions"));

        frame(
            &mut harness.app,
            vec![
                egui::Event::PointerMoved(shortcut_help_viewport().center()),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -400.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..3 {
            output = frame(&mut harness.app, Vec::new());
        }
        assert!(
            first_section_title(&output).is_none_or(|title| title.top() < top.top()),
            "{what}: the wheel should scroll the list down"
        );

        frame(
            &mut harness.app,
            vec![key_event(close, egui::Modifiers::NONE)],
        );
        assert!(!harness.app.shortcut_help_open, "{what}");
        frame(&mut harness.app, Vec::new());
        if restart {
            // A restart keeps egui's persisted memory but builds a new app.
            harness = make_app();
        }

        frame(&mut harness.app, f1());
        assert!(harness.app.shortcut_help_open, "{what}");
        for _ in 0..3 {
            output = frame(&mut harness.app, Vec::new());
        }
        assert_eq!(
            first_section_title(&output),
            Some(top),
            "{what}: reopened help should start at the top of its list"
        );
    }
}

/// One painted help row: its key label galley and where its description sits.
struct RenderedHelpRow {
    label: String,
    key: egui::Rect,
    key_lines: usize,
    description: egui::Rect,
    description_lines: usize,
}

/// The help rows painted in `output`, in paint order.
///
/// # Arguments
/// - `output`: A settled frame with help open.
/// - `window`: Help's window rect, so text elsewhere in the app is ignored.
/// - `expected`: Each key-column label, mapped to the description on its row.
fn rendered_help_rows(
    output: &egui::FullOutput,
    window: egui::Rect,
    expected: &std::collections::BTreeMap<String, &str>,
) -> Vec<RenderedHelpRow> {
    let texts = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if window.contains(text.pos) => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();
    texts
        .iter()
        .filter(|text| {
            text.galley.job.sections[0].format.font_id.family == egui::FontFamily::Monospace
        })
        .filter_map(|key| {
            let label = &key.galley.job.text;
            let description = expected.get(label)?;
            let description = texts
                .iter()
                .find(|text| text.galley.job.text.starts_with(description))
                .unwrap_or_else(|| panic!("{label} row should show {description:?}"));
            Some(RenderedHelpRow {
                label: label.clone(),
                key: egui::Rect::from_min_size(key.pos, key.galley.size()),
                key_lines: key.galley.rows.len(),
                description: egui::Rect::from_min_size(description.pos, description.galley.size()),
                description_lines: description.galley.rows.len(),
            })
        })
        .collect()
}

/// Display scales help is checked at: whole and fractional, where text widths snap differently.
const HELP_DISPLAY_SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

/// Renders help in the app's default-size window and checks every row it lists.
///
/// Each key label and its description must be one line, the description must
/// start level with the key, and each row must keep the row gap below the
/// previous one. The list is taller than its scroll area, so the check
/// wheels through it until every key label has been drawn.
///
/// # Arguments
/// - `platform`: Whose key spelling help uses.
/// - `pixels_per_point`: Native display scale; the app's zoom stays at 1.0.
fn check_help_rows_at_default_window_size(
    platform: super::super::virtual_editor::PlatformFlavor,
    pixels_per_point: f32,
) {
    use super::super::shortcuts::{
        platform_shortcut_keys, RUNTIME_SHORTCUTS, STATIC_SHORTCUT_SECTIONS,
    };

    let macos = platform == super::super::virtual_editor::PlatformFlavor::Mac;
    let what = format!("macos={macos} scale={pixels_per_point}");
    let viewport = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(DEFAULT_WINDOW_SIZE[0], DEFAULT_WINDOW_SIZE[1]),
    );
    // The palette alias shares its primary chord's row, so it never keys one.
    let expected = RUNTIME_SHORTCUTS
        .iter()
        .map(|shortcut| shortcut.help())
        .chain(
            STATIC_SHORTCUT_SECTIONS
                .iter()
                .flat_map(|section| section.entries.iter().copied()),
        )
        .filter(|entry| entry.keys != "Ctrl/Cmd+Shift+P")
        .filter_map(|entry| {
            Some((
                platform_shortcut_keys(entry.keys, macos)?,
                entry.description,
            ))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    with_platform(platform, || {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        // Half-second steps let egui's smooth wheel scrolling finish between frames.
        let mut time = 0.0;
        let mut frame = |app: &mut LocalPasteApp, events: Vec<egui::Event>| {
            time += 0.5;
            let mut input = egui::RawInput {
                screen_rect: Some(viewport),
                time: Some(time),
                events,
                ..Default::default()
            };
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .native_pixels_per_point = Some(pixels_per_point);
            run_full_update_with_input(app, &ctx, input)
        };
        frame(
            &mut harness.app,
            vec![key_event(egui::Key::F1, egui::Modifiers::NONE)],
        );
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..12 {
            let mut output = frame(&mut harness.app, Vec::new());
            for _ in 0..2 {
                output = frame(&mut harness.app, Vec::new());
            }
            let window = ctx
                .memory(|memory| memory.area_rect(egui::Id::new(SHORTCUT_HELP_WINDOW_ID)))
                .expect("help should be open");
            assert!(viewport.contains_rect(window), "{what}: {window:?}");
            let rows = rendered_help_rows(&output, window, &expected);
            for row in &rows {
                assert_eq!(
                    row.key_lines, 1,
                    "{what}: {} should not wrap in the key column ({:?})",
                    row.label, row.key
                );
                assert_eq!(
                    row.description_lines, 1,
                    "{what}: the {} description should fit on one line ({:?})",
                    row.label, row.description
                );
                assert!(
                    (row.description.top() - row.key.top()).abs() <= 0.5,
                    "{what}: the {} description should start level with its key: key {:?}, \
                     description {:?}",
                    row.label,
                    row.key,
                    row.description
                );
                seen.insert(row.label.clone());
            }
            for pair in rows.windows(2) {
                let bottom = pair[0].key.bottom().max(pair[0].description.bottom());
                assert!(
                    pair[1].key.top() - bottom >= 8.0 - 0.5,
                    "{what}: the {} row should keep its gap below the {} row, which ends at \
                     {bottom}; it starts at {}",
                    pair[1].label,
                    pair[0].label,
                    pair[1].key.top()
                );
            }
            if seen.len() == expected.len() {
                break;
            }
            frame(
                &mut harness.app,
                vec![
                    egui::Event::PointerMoved(window.center()),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -200.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(
            seen,
            expected.keys().cloned().collect(),
            "{what}: every key label should be shown"
        );
    });
}

#[test]
fn shortcut_help_key_labels_stay_on_one_line_at_the_default_window_size() {
    use super::super::virtual_editor::PlatformFlavor;

    for platform in [PlatformFlavor::Mac, PlatformFlavor::Other] {
        for pixels_per_point in HELP_DISPLAY_SCALES {
            check_help_rows_at_default_window_size(platform, pixels_per_point);
        }
    }
}
