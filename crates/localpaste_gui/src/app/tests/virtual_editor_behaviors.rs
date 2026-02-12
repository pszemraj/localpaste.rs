//! Virtual editor input/editing tests including IME and selection behavior.

use super::*;

#[test]
fn virtual_copy_reports_copied_without_text_mutation() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("abcdef");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    harness.app.virtual_editor_state.move_cursor(4, len, true);
    let ctx = egui::Context::default();

    let result = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Copy]);
    assert!(!result.changed);
    assert!(result.copied);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "abcdef");
}

#[test]
fn virtual_cut_reports_copy_and_removes_selected_text() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("abcdef");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    harness.app.virtual_editor_state.move_cursor(4, len, true);
    let ctx = egui::Context::default();

    let result = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Cut]);
    assert!(result.changed);
    assert!(result.copied);
    assert!(result.cut);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "aef");
}

#[test]
fn ime_commit_replaces_preedit_and_clears_state() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("ab");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    let ctx = egui::Context::default();

    let result = harness.app.apply_virtual_commands(
        &ctx,
        &[
            VirtualInputCommand::ImeEnabled,
            VirtualInputCommand::ImePreedit("に".to_string()),
            VirtualInputCommand::ImeCommit("日".to_string()),
            VirtualInputCommand::ImeDisabled,
        ],
    );

    assert!(result.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "a日b");
    assert!(!harness.app.virtual_editor_state.ime.enabled);
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
    assert!(harness.app.virtual_editor_state.ime.preedit_text.is_empty());
}

#[test]
fn ime_disable_cancels_uncommitted_preedit_text() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("ab");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    let ctx = egui::Context::default();

    let result = harness.app.apply_virtual_commands(
        &ctx,
        &[
            VirtualInputCommand::ImeEnabled,
            VirtualInputCommand::ImePreedit("に".to_string()),
            VirtualInputCommand::ImeDisabled,
        ],
    );

    assert!(result.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "ab");
    assert!(!harness.app.virtual_editor_state.ime.enabled);
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
    assert!(harness.app.virtual_editor_state.ime.preedit_text.is_empty());
}

#[test]
fn empty_preedit_clears_composition_and_allows_insert_text() {
    let mut harness = make_app();
    harness.app.reset_virtual_editor("ab");
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness.app.virtual_editor_state.set_cursor(1, len);
    let ctx = egui::Context::default();

    let result = harness.app.apply_virtual_commands(
        &ctx,
        &[
            VirtualInputCommand::ImeEnabled,
            VirtualInputCommand::ImePreedit("に".to_string()),
            VirtualInputCommand::ImePreedit(String::new()),
            VirtualInputCommand::InsertText("x".to_string()),
        ],
    );

    assert!(result.changed);
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), "axb");
    assert!(harness.app.virtual_editor_state.ime.preedit_range.is_none());
    assert!(harness.app.virtual_editor_state.ime.preedit_text.is_empty());
}

#[test]
fn virtual_command_classification_respects_focus_policy() {
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::Copy, false),
        VirtualCommandBucket::DeferredCopy
    );
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::InsertText("x".to_string()), false),
        VirtualCommandBucket::DeferredFocus
    );
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::InsertText("x".to_string()), true),
        VirtualCommandBucket::ImmediateFocus
    );
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::Cut, true),
        VirtualCommandBucket::DeferredFocus
    );
    assert_eq!(
        classify_virtual_command(&VirtualInputCommand::Paste("x".to_string()), true),
        VirtualCommandBucket::DeferredFocus
    );
}

#[test]
fn off_focus_commands_do_not_mutate_virtual_editor_with_selection() {
    fn setup_selection(app: &mut LocalPasteApp) {
        app.reset_virtual_editor("abcdef");
        let len = app.virtual_editor_buffer.len_chars();
        app.virtual_editor_state.set_cursor(1, len);
        app.virtual_editor_state.move_cursor(4, len, true);
    }

    fn merge_apply(target: &mut VirtualApplyResult, src: VirtualApplyResult) {
        target.changed |= src.changed;
        target.copied |= src.copied;
        target.cut |= src.cut;
        target.pasted |= src.pasted;
    }

    fn route_and_apply(
        app: &mut LocalPasteApp,
        ctx: &egui::Context,
        command: VirtualInputCommand,
        focus_active_pre: bool,
        focus_active_post: bool,
        copy_ready_post: bool,
    ) -> VirtualApplyResult {
        let mut immediate = Vec::new();
        let mut deferred_focus = Vec::new();
        let mut deferred_copy = Vec::new();
        match classify_virtual_command(&command, focus_active_pre) {
            VirtualCommandBucket::ImmediateFocus => immediate.push(command),
            VirtualCommandBucket::DeferredFocus => deferred_focus.push(command),
            VirtualCommandBucket::DeferredCopy => deferred_copy.push(command),
        }

        let mut result = app.apply_virtual_commands(ctx, &immediate);
        if focus_active_post {
            merge_apply(
                &mut result,
                app.apply_virtual_commands(ctx, &deferred_focus),
            );
        }
        if copy_ready_post {
            merge_apply(&mut result, app.apply_virtual_commands(ctx, &deferred_copy));
        }
        result
    }

    let mut harness = make_app();
    let ctx = egui::Context::default();
    let blocked_commands = [
        VirtualInputCommand::InsertText("X".to_string()),
        VirtualInputCommand::Backspace { word: false },
        VirtualInputCommand::DeleteForward { word: false },
        VirtualInputCommand::Cut,
        VirtualInputCommand::Paste("ZZ".to_string()),
        VirtualInputCommand::ImeEnabled,
        VirtualInputCommand::ImePreedit("Z".to_string()),
        VirtualInputCommand::ImeCommit("Z".to_string()),
        VirtualInputCommand::ImeDisabled,
        VirtualInputCommand::MoveLeft {
            select: false,
            word: false,
        },
        VirtualInputCommand::SelectAll,
    ];

    for command in blocked_commands {
        setup_selection(&mut harness.app);
        let before_text = harness.app.virtual_editor_buffer.to_string();
        let before_cursor = harness.app.virtual_editor_state.cursor();
        let before_selection = harness.app.virtual_editor_state.selection_range();
        let before_ime = harness.app.virtual_editor_state.ime.clone();
        let result = route_and_apply(
            &mut harness.app,
            &ctx,
            command,
            false,
            false,
            true, // copy-ready because selection exists
        );

        assert_eq!(harness.app.virtual_editor_buffer.to_string(), before_text);
        assert_eq!(harness.app.virtual_editor_state.cursor(), before_cursor);
        assert_eq!(
            harness.app.virtual_editor_state.selection_range(),
            before_selection
        );
        assert_eq!(harness.app.virtual_editor_state.ime, before_ime);
        assert!(!result.changed);
        assert!(!result.cut);
        assert!(!result.pasted);
    }

    setup_selection(&mut harness.app);
    let before_text = harness.app.virtual_editor_buffer.to_string();
    let result = route_and_apply(
        &mut harness.app,
        &ctx,
        VirtualInputCommand::Copy,
        false,
        false,
        true,
    );
    assert_eq!(harness.app.virtual_editor_buffer.to_string(), before_text);
    assert!(!result.changed);
    assert!(result.copied);
}

#[test]
fn virtual_click_counter_promotes_to_triple_and_resets() {
    let now = Instant::now();
    let p = egui::pos2(100.0, 200.0);
    let c1 = next_virtual_click_count(None, None, None, 0, 5, p, now);
    assert_eq!(c1, 1);
    let c2 = next_virtual_click_count(Some(now), Some(p), Some(5), c1, 5, p, now);
    assert_eq!(c2, 2);
    let c3 = next_virtual_click_count(Some(now), Some(p), Some(5), c2, 5, p, now);
    assert_eq!(c3, 3);

    let changed_line = next_virtual_click_count(Some(now), Some(p), Some(5), c3, 6, p, now);
    assert_eq!(changed_line, 3);

    let expired = next_virtual_click_count(
        Some(now),
        Some(p),
        Some(5),
        c3,
        5,
        p,
        now + EDITOR_DOUBLE_CLICK_WINDOW + Duration::from_millis(1),
    );
    assert_eq!(expired, 1);

    let far = egui::pos2(100.0 + EDITOR_DOUBLE_CLICK_DISTANCE + 1.0, 200.0);
    let distant = next_virtual_click_count(Some(now), Some(p), Some(5), c3, 5, far, now);
    assert_eq!(distant, 1);
}

#[test]
fn drag_autoscroll_delta_scrolls_up_when_pointer_above() {
    let delta = drag_autoscroll_delta(80.0, 100.0, 220.0, 20.0);
    assert!(delta > 0.0);
}

#[test]
fn drag_autoscroll_delta_scrolls_down_when_pointer_below() {
    let delta = drag_autoscroll_delta(260.0, 100.0, 220.0, 20.0);
    assert!(delta < 0.0);
}

#[test]
fn drag_autoscroll_delta_is_zero_inside_viewport() {
    let delta = drag_autoscroll_delta(150.0, 100.0, 220.0, 20.0);
    assert_eq!(delta, 0.0);
}

#[test]
fn word_range_at_selects_word() {
    let text = "hello world";
    let (start, end) = word_range_at(text, 1).expect("range");
    let selected: String = text.chars().skip(start).take(end - start).collect();
    assert_eq!(selected, "hello");
}
