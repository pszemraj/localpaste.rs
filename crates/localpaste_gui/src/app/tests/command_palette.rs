//! Command palette availability across asynchronous selection loads.

use super::*;

#[test]
fn command_palette_has_toolbar_actions_and_never_dispatches_paste_search() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![command_key_event(egui::Key::K)],
    );
    assert!(harness.app.command_palette_open);
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
    harness.app.command_palette_query.clear();
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![egui::Event::Text("body-only-needle".into())],
    );
    assert_eq!(harness.app.command_palette_query, "body-only-needle");
    assert!(harness.app.command_palette_actions().is_empty());
    assert!(harness.cmd_rx.try_recv().is_err());

    // A selection id precedes its body load; the empty replacement buffer is
    // never a valid source for Duplicate.
    assert!(harness.app.select_paste("picked".into()));
    assert!(matches!(
        recv_cmd(&harness.cmd_rx),
        CoreCmd::GetPaste { .. }
    ));
    assert!(harness.app.selected_paste.is_none());
    harness.app.command_palette_query = "Duplicate".into();
    assert!(harness.app.command_palette_actions().is_empty());
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(harness.cmd_rx.try_recv().is_err());
    assert!(harness.app.command_palette_open);
    let mut paste = Paste::new("loaded body".into(), "Picked".into());
    paste.id = "picked".into();
    harness.app.apply_event(CoreEvent::PasteLoaded {
        paste,
        selection_epoch: harness.app.active_buffer_epoch,
    });
    assert_eq!(harness.app.command_palette_actions().len(), 1);
    set_active_content(&mut harness.app, "unsaved body");
    harness.app.save_status = SaveStatus::Dirty;
    run_full_update(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert!(
        matches!(recv_cmd(&harness.cmd_rx), CoreCmd::CreatePaste { content } if content == "unsaved body")
    );
    assert!(!harness.app.command_palette_open);
}
