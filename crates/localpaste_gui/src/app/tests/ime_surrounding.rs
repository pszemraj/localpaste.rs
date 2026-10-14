//! Surrounding-text deletion must retain composition and undoable source text.

use super::*;

#[test]
fn ime_surrounding_deletion_preserves_preedit_and_undo_history() {
    for backwards in [false, true] {
        for (before, after, offset, visible, restored, committed) in [
            (1, 1, 1, "au\u{301}z", "aOLDz", "aOKz"),
            (usize::MAX, usize::MAX, 0, "u\u{301}", "OLD", "OK"),
            (usize::MAX, 0, 0, "u\u{301}Ωz", "OLDΩz", "OKΩz"),
            (0, usize::MAX, 2, "aéu\u{301}", "aéOLD", "aéOK"),
        ] {
            let mut harness = make_app();
            let app = &mut harness.app;
            app.reset_virtual_editor("aéOLDΩz");
            let (cursor, anchor) = if backwards { (2, 5) } else { (5, 2) };
            app.virtual_editor_state
                .restore_selection(cursor, Some(anchor), 7);
            let ctx = egui::Context::default();
            app.apply_virtual_commands(&ctx, &[VirtualInputCommand::ImePreedit("u\u{301}".into())]);

            let result = app.apply_virtual_commands(
                &ctx,
                &[VirtualInputCommand::ImeDeleteSurrounding {
                    before_chars: before,
                    after_chars: after,
                }],
            );
            assert!(result.changed);
            assert_eq!(app.active_snapshot(), visible);
            assert_eq!(app.virtual_editor_state.ime.preedit_text, "u\u{301}");
            assert_eq!(
                app.virtual_editor_state.ime.preedit_range,
                Some(offset..offset + 2)
            );
            assert!(app.virtual_editor_state.ime.enabled);
            assert_eq!(app.virtual_editor_history.perf_stats().undo_len, 1);

            app.apply_virtual_commands(&ctx, &[VirtualInputCommand::ImeCommit("OK".into())]);
            assert_eq!(app.active_snapshot(), committed);
            assert_eq!(app.virtual_editor_history.perf_stats().undo_len, 2);
            app.apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
            assert_eq!(app.active_snapshot(), restored);
            assert_eq!(app.virtual_editor_state.cursor(), cursor - 2 + offset);
            assert_eq!(app.virtual_editor_state.anchor(), Some(anchor - 2 + offset));
            app.apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
            assert_eq!(app.active_snapshot(), "aéOLDΩz");
            assert_eq!(app.virtual_editor_state.cursor(), cursor);
            assert_eq!(app.virtual_editor_state.anchor(), Some(anchor));
            app.apply_virtual_commands(
                &ctx,
                &[VirtualInputCommand::Redo, VirtualInputCommand::Redo],
            );
            assert_eq!(app.active_snapshot(), committed);
            assert_eq!(app.virtual_editor_state.selection_range(), None);
        }
    }
}

#[test]
fn ime_surrounding_deletion_cancellation_restores_displaced_text() {
    for (cursor, anchor, visible, restored) in [
        (5, Some(2), "aéΩd", "aOLDd"),
        (2, Some(5), "aéΩd", "aOLDd"),
        (2, None, "aéΩLDcd", "aLDcd"),
    ] {
        let mut harness = make_app();
        let app = &mut harness.app;
        app.reset_virtual_editor("abOLDcd");
        app.virtual_editor_state
            .restore_selection(cursor, anchor, 7);
        let ctx = egui::Context::default();
        app.apply_virtual_commands(
            &ctx,
            &[
                VirtualInputCommand::ImePreedit("éΩ".into()),
                VirtualInputCommand::ImeDeleteSurrounding {
                    before_chars: 1,
                    after_chars: 1,
                },
            ],
        );
        assert_eq!(app.active_snapshot(), visible);
        app.apply_virtual_commands(&ctx, &[VirtualInputCommand::ImePreedit(String::new())]);
        assert_eq!(app.active_snapshot(), restored);
        assert_eq!(app.virtual_editor_state.cursor(), cursor - 1);
        assert_eq!(
            app.virtual_editor_state.anchor(),
            anchor.map(|index| index - 1)
        );
        assert_eq!(app.virtual_editor_history.perf_stats().undo_len, 1);
        app.apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
        assert_eq!(app.active_snapshot(), "abOLDcd");
        assert_eq!(app.virtual_editor_state.cursor(), cursor);
        assert_eq!(app.virtual_editor_state.anchor(), anchor);
    }
}

#[test]
fn ime_surrounding_deletion_without_preedit_keeps_the_selection() {
    for (cursor, anchor, expected) in [
        (5, Some(2), "aOLDz"),
        (2, Some(5), "aOLDz"),
        (2, None, "aLDΩz"),
    ] {
        let mut harness = make_app();
        let app = &mut harness.app;
        app.reset_virtual_editor("aéOLDΩz");
        app.virtual_editor_state
            .restore_selection(cursor, anchor, 7);
        let ctx = egui::Context::default();
        app.apply_virtual_commands(
            &ctx,
            &[VirtualInputCommand::ImeDeleteSurrounding {
                before_chars: 1,
                after_chars: 1,
            }],
        );
        assert_eq!(app.active_snapshot(), expected);
        assert_eq!(app.virtual_editor_state.cursor(), cursor - 1);
        assert_eq!(
            app.virtual_editor_state.anchor(),
            anchor.map(|index| index - 1)
        );
        app.apply_virtual_commands(&ctx, &[VirtualInputCommand::Undo]);
        assert_eq!(app.active_snapshot(), "aéOLDΩz");
    }
}

#[test]
fn ime_surrounding_deletion_noop_retains_composition_and_revision() {
    for (text, cursor, anchor, count) in [("OLD", 3, 0, usize::MAX), ("abOLDcd", 5, 2, 0)] {
        let mut harness = make_app();
        let app = &mut harness.app;
        app.reset_virtual_editor(text);
        app.virtual_editor_state
            .restore_selection(cursor, Some(anchor), text.chars().count());
        let ctx = egui::Context::default();
        app.apply_virtual_commands(&ctx, &[VirtualInputCommand::ImePreedit("éΩ".into())]);
        let before = app.virtual_editor_state.clone();
        let revision = app.virtual_editor_buffer.revision();
        let snapshot = app.active_snapshot();
        let result = app.apply_virtual_commands(
            &ctx,
            &[VirtualInputCommand::ImeDeleteSurrounding {
                before_chars: count,
                after_chars: count,
            }],
        );
        assert!(!result.changed);
        assert_eq!(app.active_snapshot(), snapshot);
        assert_eq!(app.virtual_editor_buffer.revision(), revision);
        assert_eq!(app.virtual_editor_state, before);
        assert_eq!(app.virtual_editor_history.perf_stats().undo_len, 0);
    }
}

#[test]
fn ime_surrounding_deletion_routes_through_rendered_editor_events() {
    for same_frame in [false, true] {
        let mut harness = make_app();
        let app = &mut harness.app;
        app.reset_virtual_editor("abOLDcd");
        app.focus_editor_next = true;
        let ctx = egui::Context::default();
        configure_virtual_editor_test_ctx(&ctx);
        run_virtual_editor_frame(app, &ctx, vec![]);
        app.virtual_editor_state.restore_selection(5, Some(2), 7);
        let mut events = vec![egui::Event::Ime(egui::ImeEvent::Preedit {
            text: "éΩ".into(),
            active_range_chars: None,
        })];
        if !same_frame {
            run_virtual_editor_frame(app, &ctx, std::mem::take(&mut events));
        }
        events.push(egui::Event::Ime(egui::ImeEvent::DeleteSurrounding {
            before_chars: 1,
            after_chars: 1,
        }));
        run_virtual_editor_frame(app, &ctx, events);
        assert_eq!(app.active_snapshot(), "aéΩd");
        assert_eq!(app.virtual_editor_state.ime.preedit_range, Some(1..3));
        run_virtual_editor_frame(
            app,
            &ctx,
            vec![egui::Event::Ime(egui::ImeEvent::Commit("OK".into()))],
        );
        assert_eq!(app.active_snapshot(), "aOKd");
    }
}
