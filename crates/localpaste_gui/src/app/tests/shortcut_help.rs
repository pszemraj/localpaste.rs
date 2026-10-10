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
