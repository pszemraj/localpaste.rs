//! Independent discovery surfaces and stale scoped-response regressions.

use super::*;
use crate::app::paste_intent::KeyboardFocusState;

#[test]
fn sidebar_and_picker_reject_stale_scopes_and_keep_independent_queries() {
    let mut harness = make_app();
    harness.app.set_search_query("sidebar".into());
    harness.app.set_search_scope(SearchScope::Title);
    harness.app.maybe_dispatch_search();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::SearchPastes {
            scope: SearchScope::Title,
            ..
        }
    ));
    harness.app.paste_picker_open = true;
    harness.app.set_paste_picker_query("picker".into());
    harness.app.set_paste_picker_scope(SearchScope::Body);
    harness.app.maybe_dispatch_palette_search();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::SearchPalette {
            scope: SearchScope::Body,
            ..
        }
    ));
    for scope in [SearchScope::All, SearchScope::Body, SearchScope::Metadata] {
        harness.app.apply_event(CoreEvent::SearchResults {
            query: "sidebar".into(),
            scope,
            folder_id: None,
            language: None,
            items: vec![test_summary("stale", "stale", None, 1)],
        });
        assert!(harness.app.pastes.is_empty());
    }
    for scope in [SearchScope::All, SearchScope::Title, SearchScope::Metadata] {
        harness.app.apply_event(CoreEvent::PaletteSearchResults {
            query: "picker".into(),
            scope,
            items: vec![test_summary("stale", "stale", None, 1)],
        });
        assert!(harness.app.palette_search_results.is_empty());
    }
    harness.app.apply_event(CoreEvent::PaletteSearchResults {
        query: "picker".into(),
        scope: SearchScope::Body,
        items: vec![test_summary("valid", "valid", None, 1)],
    });
    assert_eq!(harness.app.palette_search_results[0].id, "valid");
    assert_eq!(harness.app.search_query, "sidebar");
    assert_eq!(harness.app.search_scope, SearchScope::Title);
    // Changing only the field scope must dispatch even with identical query text.
    harness.app.set_search_scope(SearchScope::Metadata);
    harness.app.maybe_dispatch_search();
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::SearchPastes {
            scope: SearchScope::Metadata,
            ..
        }
    ));
    assert_eq!(harness.app.paste_picker_query, "picker");
}

#[test]
fn command_palette_has_toolbar_actions_and_never_dispatches_paste_search() {
    let mut harness = make_app();
    harness.app.command_palette_open = true;
    for query in [
        "Export",
        "Duplicate",
        "Copy",
        "Copy link",
        "Find",
        "Properties",
        "History",
        "Diff",
    ] {
        harness.app.command_palette_query = query.into();
        assert!(
            !harness.app.command_palette_actions().is_empty(),
            "missing {query}"
        );
    }
    harness.app.command_palette_query = "body-only-needle".into();
    harness.app.palette_search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
    harness.app.maybe_dispatch_palette_search();
    assert!(harness.app.command_palette_actions().is_empty());
    assert!(harness.cmd_rx.try_recv().is_err());
}

#[test]
fn paste_picker_shortcut_and_modal_input_leave_editor_unchanged() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.focus_editor_next = true;
    run_full_update(&mut harness.app, &ctx, vec![]);
    let modifiers = egui::Modifiers {
        shift: true,
        ..primary_command_modifiers()
    };
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::K, modifiers)],
    );
    assert!(harness.app.paste_picker_open);
    assert!(!harness.app.command_palette_open);
    assert!(harness.app.keyboard_overlay_open());
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("query".into())],
    );
    assert_eq!(harness.app.paste_picker_query, "query");
    assert_eq!(harness.app.active_snapshot(), "content");
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![command_key_event(egui::Key::K)],
    );
    assert!(!harness.app.paste_picker_open);
    assert!(harness.app.command_palette_open);
    assert_eq!(harness.app.paste_picker_query, "query");
}

#[test]
fn help_focus_and_unfocused_modals_never_create_background_pastes() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.shortcut_help_open = true;
    run_full_update(&mut harness.app, &ctx, vec![]);
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Paste("undo".into())],
    );
    assert_eq!(harness.app.shortcut_help_query, "undo");
    assert_eq!(harness.app.active_snapshot(), "content");
    assert!(!harness.app.maybe_route_implicit_global_clipboard_create(
        Some("background".into()),
        false,
        false,
        false
    ));
    assert_eq!(
        harness
            .app
            .route_plain_paste_shortcut(KeyboardFocusState::Unfocused, false),
        (false, false)
    );
    assert!(!harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
}
