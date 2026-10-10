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
