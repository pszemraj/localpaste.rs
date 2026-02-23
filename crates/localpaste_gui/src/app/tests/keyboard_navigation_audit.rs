//! Keyboard navigation audit tests for platform-keymap parity and selection behavior.

use super::*;

fn configure_virtual_editor_with_wrap(app: &mut LocalPasteApp, text: &str, wrap_width: f32) {
    app.reset_virtual_editor(text);
    app.virtual_layout
        .rebuild(&app.virtual_editor_buffer, wrap_width, 1.0, 1.0);
}

fn set_cursor(app: &mut LocalPasteApp, line: usize, col: usize) {
    let len = app.virtual_editor_buffer.len_chars();
    let pos = app.virtual_editor_buffer.line_col_to_char(line, col);
    app.virtual_editor_state.set_cursor(pos, len);
}

#[test]
fn shift_word_selection_extends_and_contracts_without_resetting_anchor() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "foo bar baz", 400.0);
    let ctx = egui::Context::default();

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveRight {
            select: true,
            word: true,
        }],
    );
    #[cfg(target_os = "macos")]
    let first_cursor = harness.app.virtual_editor_buffer.line_col_to_char(0, 3);
    #[cfg(not(target_os = "macos"))]
    let first_cursor = harness.app.virtual_editor_buffer.line_col_to_char(0, 4);
    assert_eq!(harness.app.virtual_editor_state.cursor(), first_cursor);
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(0..first_cursor)
    );

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveRight {
            select: true,
            word: true,
        }],
    );
    #[cfg(target_os = "macos")]
    let second_cursor = harness.app.virtual_editor_buffer.line_col_to_char(0, 7);
    #[cfg(not(target_os = "macos"))]
    let second_cursor = harness.app.virtual_editor_buffer.line_col_to_char(0, 8);
    assert_eq!(harness.app.virtual_editor_state.cursor(), second_cursor);
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(0..second_cursor)
    );

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveLeft {
            select: true,
            word: true,
        }],
    );
    assert_eq!(harness.app.virtual_editor_state.cursor(), first_cursor);
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(0..first_cursor)
    );

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveLeft {
            select: true,
            word: true,
        }],
    );
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);
    assert!(harness.app.virtual_editor_state.selection_range().is_none());
}

#[test]
fn plain_left_right_collapse_selection_to_expected_edge() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "0123456789", 400.0);
    let ctx = egui::Context::default();
    let len = harness.app.virtual_editor_buffer.len_chars();

    harness.app.virtual_editor_state.set_cursor(2, len);
    harness.app.virtual_editor_state.move_cursor(8, len, true);
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(2..8)
    );
    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveLeft {
            select: false,
            word: false,
        }],
    );
    assert_eq!(harness.app.virtual_editor_state.cursor(), 2);
    assert!(harness.app.virtual_editor_state.selection_range().is_none());

    harness.app.virtual_editor_state.set_cursor(2, len);
    harness.app.virtual_editor_state.move_cursor(8, len, true);
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(2..8)
    );
    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveRight {
            select: false,
            word: false,
        }],
    );
    assert_eq!(harness.app.virtual_editor_state.cursor(), 8);
    assert!(harness.app.virtual_editor_state.selection_range().is_none());
}

#[test]
fn word_navigation_crosses_lines_and_clamps_doc_boundaries() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "alpha\nbeta gamma\n", 400.0);
    let ctx = egui::Context::default();

    set_cursor(&mut harness.app, 1, 0);
    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveLeft {
            select: false,
            word: true,
        }],
    );
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveLeft {
            select: false,
            word: true,
        }],
    );
    assert_eq!(harness.app.virtual_editor_state.cursor(), 0);

    harness.app.virtual_editor_state.set_cursor(
        harness.app.virtual_editor_buffer.len_chars(),
        harness.app.virtual_editor_buffer.len_chars(),
    );
    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveRight {
            select: false,
            word: true,
        }],
    );
    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        harness.app.virtual_editor_buffer.len_chars()
    );
}

#[test]
fn word_left_handles_punctuation_and_spacing_matrix() {
    struct Case {
        text: &'static str,
        expected_after_first: usize,
        expected_after_second: usize,
    }
    let cases = [
        Case {
            text: "foo.bar",
            expected_after_first: 4,
            expected_after_second: 0,
        },
        Case {
            text: "foo::bar",
            expected_after_first: 5,
            expected_after_second: 0,
        },
        Case {
            text: "foo--bar",
            expected_after_first: 5,
            expected_after_second: 0,
        },
        Case {
            text: "foo   bar",
            expected_after_first: 6,
            expected_after_second: 0,
        },
    ];
    let ctx = egui::Context::default();

    for case in cases {
        let mut harness = make_app();
        configure_virtual_editor_with_wrap(&mut harness.app, case.text, 400.0);
        let len = harness.app.virtual_editor_buffer.len_chars();
        harness.app.virtual_editor_state.set_cursor(len, len);

        let _ = harness.app.apply_virtual_commands(
            &ctx,
            &[VirtualInputCommand::MoveLeft {
                select: false,
                word: true,
            }],
        );
        assert_eq!(
            harness.app.virtual_editor_state.cursor(),
            case.expected_after_first
        );

        let _ = harness.app.apply_virtual_commands(
            &ctx,
            &[VirtualInputCommand::MoveLeft {
                select: false,
                word: true,
            }],
        );
        assert_eq!(
            harness.app.virtual_editor_state.cursor(),
            case.expected_after_second
        );
    }
}

#[test]
fn word_right_skips_punctuation_and_whitespace_by_platform_semantics() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "foo.bar baz", 400.0);
    let ctx = egui::Context::default();
    harness
        .app
        .virtual_editor_state
        .set_cursor(0, harness.app.virtual_editor_buffer.len_chars());

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveRight {
            select: false,
            word: true,
        }],
    );
    #[cfg(target_os = "macos")]
    let first_expected = harness.app.virtual_editor_buffer.line_col_to_char(0, 3);
    #[cfg(not(target_os = "macos"))]
    let first_expected = harness.app.virtual_editor_buffer.line_col_to_char(0, 4);
    assert_eq!(harness.app.virtual_editor_state.cursor(), first_expected);

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveRight {
            select: false,
            word: true,
        }],
    );
    #[cfg(target_os = "macos")]
    let second_expected = harness.app.virtual_editor_buffer.line_col_to_char(0, 7);
    #[cfg(not(target_os = "macos"))]
    let second_expected = harness.app.virtual_editor_buffer.line_col_to_char(0, 8);
    assert_eq!(harness.app.virtual_editor_state.cursor(), second_expected);
}

#[test]
fn vertical_column_affinity_restores_target_after_short_line_and_resets_after_horizontal_move() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "1234567890\nx\nabcdefghij\n", 400.0);
    let ctx = egui::Context::default();

    set_cursor(&mut harness.app, 0, 8);
    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDown { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (1, 1));

    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDown { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (2, 8));

    let _ = harness.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveLeft {
            select: false,
            word: false,
        }],
    );
    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveUp { select: false }]);
    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveUp { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (0, 7));
}

#[test]
fn wrapped_lines_move_by_visual_rows_and_home_end_use_visual_row_bounds() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "abcdefghijkl\nz\n", 4.0);
    let ctx = egui::Context::default();

    set_cursor(&mut harness.app, 0, 2);
    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDown { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (0, 6));

    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDown { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (0, 10));

    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDown { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (1, 1));

    set_cursor(&mut harness.app, 0, 6);
    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveHome { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (0, 4));

    set_cursor(&mut harness.app, 0, 6);
    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveEnd { select: false }]);
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(harness.app.virtual_editor_state.cursor());
    assert_eq!((line, col), (0, 8));
}

#[test]
fn delete_with_active_selection_ignores_word_modifier() {
    let ctx = egui::Context::default();

    let mut backspace_case = make_app();
    configure_virtual_editor_with_wrap(&mut backspace_case.app, "alpha beta", 400.0);
    let len = backspace_case.app.virtual_editor_buffer.len_chars();
    backspace_case.app.virtual_editor_state.set_cursor(2, len);
    backspace_case
        .app
        .virtual_editor_state
        .move_cursor(7, len, true);
    let _ = backspace_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Backspace { word: true }]);
    assert_eq!(
        backspace_case.app.virtual_editor_buffer.to_string(),
        "aleta"
    );

    let mut delete_case = make_app();
    configure_virtual_editor_with_wrap(&mut delete_case.app, "alpha beta", 400.0);
    let len = delete_case.app.virtual_editor_buffer.len_chars();
    delete_case.app.virtual_editor_state.set_cursor(2, len);
    delete_case
        .app
        .virtual_editor_state
        .move_cursor(7, len, true);
    let _ = delete_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::DeleteForward { word: true }]);
    assert_eq!(delete_case.app.virtual_editor_buffer.to_string(), "aleta");
}

#[test]
fn select_all_follow_on_behaviors_match_editor_conventions() {
    let ctx = egui::Context::default();

    let mut replace_case = make_app();
    configure_virtual_editor_with_wrap(&mut replace_case.app, "one two", 400.0);
    let _ = replace_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::SelectAll]);
    let _ = replace_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::InsertText("X".to_string())]);
    assert_eq!(replace_case.app.virtual_editor_buffer.to_string(), "X");

    let mut delete_case = make_app();
    configure_virtual_editor_with_wrap(&mut delete_case.app, "one two", 400.0);
    let _ = delete_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::SelectAll]);
    let _ = delete_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::Backspace { word: false }]);
    assert_eq!(delete_case.app.virtual_editor_buffer.to_string(), "");

    let mut collapse_left_case = make_app();
    configure_virtual_editor_with_wrap(&mut collapse_left_case.app, "one two", 400.0);
    let _ = collapse_left_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::SelectAll]);
    let _ = collapse_left_case.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveLeft {
            select: false,
            word: false,
        }],
    );
    assert_eq!(collapse_left_case.app.virtual_editor_state.cursor(), 0);
    assert!(collapse_left_case
        .app
        .virtual_editor_state
        .selection_range()
        .is_none());

    let mut collapse_right_case = make_app();
    configure_virtual_editor_with_wrap(&mut collapse_right_case.app, "one two", 400.0);
    let _ = collapse_right_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::SelectAll]);
    let _ = collapse_right_case.app.apply_virtual_commands(
        &ctx,
        &[VirtualInputCommand::MoveRight {
            select: false,
            word: false,
        }],
    );
    assert_eq!(
        collapse_right_case.app.virtual_editor_state.cursor(),
        collapse_right_case.app.virtual_editor_buffer.len_chars()
    );
    assert!(collapse_right_case
        .app
        .virtual_editor_state
        .selection_range()
        .is_none());
}

#[test]
fn word_navigation_matrix_matches_expected_token_boundaries() {
    struct Case {
        text: &'static str,
        left: &'static [usize],
        right_non_mac: &'static [usize],
        right_mac: &'static [usize],
    }

    let cases = [
        Case {
            text: "snake_case camelCase",
            left: &[11, 0],
            right_non_mac: &[11, 20],
            right_mac: &[10, 20],
        },
        Case {
            text: "foo.bar.baz",
            left: &[8, 4, 0],
            right_non_mac: &[4, 8, 11],
            right_mac: &[3, 7, 11],
        },
        Case {
            text: "foo::bar",
            left: &[5, 0],
            right_non_mac: &[5, 8],
            right_mac: &[3, 8],
        },
        Case {
            text: "foo--bar",
            left: &[5, 0],
            right_non_mac: &[5, 8],
            right_mac: &[3, 8],
        },
        Case {
            text: "  leading  spaces",
            left: &[11, 2, 0],
            right_non_mac: &[2, 11, 17],
            right_mac: &[9, 17],
        },
        Case {
            text: "trailing spaces   ",
            left: &[9, 0],
            right_non_mac: &[9, 18],
            right_mac: &[8, 15, 18],
        },
        Case {
            text: "foo...bar",
            left: &[6, 0],
            right_non_mac: &[6, 9],
            right_mac: &[3, 9],
        },
        Case {
            text: "foo___bar",
            left: &[0],
            right_non_mac: &[9],
            right_mac: &[9],
        },
    ];
    let ctx = egui::Context::default();

    for case in cases {
        let mut left_harness = make_app();
        configure_virtual_editor_with_wrap(&mut left_harness.app, case.text, 400.0);
        let len = left_harness.app.virtual_editor_buffer.len_chars();
        left_harness.app.virtual_editor_state.set_cursor(len, len);
        for expected in case.left {
            let _ = left_harness.app.apply_virtual_commands(
                &ctx,
                &[VirtualInputCommand::MoveLeft {
                    select: false,
                    word: true,
                }],
            );
            assert_eq!(
                left_harness.app.virtual_editor_state.cursor(),
                *expected,
                "left matrix case '{}' expected cursor {}",
                case.text,
                expected
            );
        }

        let mut right_harness = make_app();
        configure_virtual_editor_with_wrap(&mut right_harness.app, case.text, 400.0);
        right_harness
            .app
            .virtual_editor_state
            .set_cursor(0, right_harness.app.virtual_editor_buffer.len_chars());
        let expected_right = if cfg!(target_os = "macos") {
            case.right_mac
        } else {
            case.right_non_mac
        };
        for expected in expected_right {
            let _ = right_harness.app.apply_virtual_commands(
                &ctx,
                &[VirtualInputCommand::MoveRight {
                    select: false,
                    word: true,
                }],
            );
            assert_eq!(
                right_harness.app.virtual_editor_state.cursor(),
                *expected,
                "right matrix case '{}' expected cursor {}",
                case.text,
                expected
            );
        }
    }
}

#[test]
fn shift_vertical_selection_preserves_anchor_when_reversing_direction() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "0123456789\nshort\nabcdefghij\n", 400.0);
    let ctx = egui::Context::default();

    set_cursor(&mut harness.app, 0, 6);
    let anchor = harness.app.virtual_editor_state.cursor();

    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDown { select: true }]);
    let first_cursor = harness.app.virtual_editor_state.cursor();
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(anchor..first_cursor)
    );

    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDown { select: true }]);
    let second_cursor = harness.app.virtual_editor_state.cursor();
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(anchor..second_cursor)
    );

    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveUp { select: true }]);
    assert_eq!(harness.app.virtual_editor_state.cursor(), first_cursor);
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(anchor..first_cursor)
    );
}

#[test]
fn wrapped_line_shift_selection_tracks_visual_rows() {
    let mut harness = make_app();
    configure_virtual_editor_with_wrap(&mut harness.app, "abcdefghijkl\n", 4.0);
    let ctx = egui::Context::default();

    set_cursor(&mut harness.app, 0, 2);
    let anchor = harness.app.virtual_editor_state.cursor();

    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDown { select: true }]);
    let first_cursor = harness.app.virtual_editor_state.cursor();
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(first_cursor);
    assert_eq!((line, col), (0, 6));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(anchor..first_cursor)
    );

    let _ = harness
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::MoveDown { select: true }]);
    let second_cursor = harness.app.virtual_editor_state.cursor();
    let (line, col) = harness
        .app
        .virtual_editor_buffer
        .char_to_line_col(second_cursor);
    assert_eq!((line, col), (0, 10));
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        Some(anchor..second_cursor)
    );
}

#[test]
fn delete_to_line_boundaries_obey_selection_and_cursor_cases() {
    let ctx = egui::Context::default();

    let mut line_start_case = make_app();
    configure_virtual_editor_with_wrap(&mut line_start_case.app, "abc def", 400.0);
    set_cursor(&mut line_start_case.app, 0, 4);
    let _ = line_start_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::DeleteToLineStart]);
    assert_eq!(line_start_case.app.virtual_editor_buffer.to_string(), "def");
    assert_eq!(line_start_case.app.virtual_editor_state.cursor(), 0);

    let mut line_end_case = make_app();
    configure_virtual_editor_with_wrap(&mut line_end_case.app, "abc def", 400.0);
    set_cursor(&mut line_end_case.app, 0, 3);
    let _ = line_end_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::DeleteToLineEnd]);
    assert_eq!(line_end_case.app.virtual_editor_buffer.to_string(), "abc");
    assert_eq!(line_end_case.app.virtual_editor_state.cursor(), 3);

    let mut selection_case = make_app();
    configure_virtual_editor_with_wrap(&mut selection_case.app, "abc def", 400.0);
    let len = selection_case.app.virtual_editor_buffer.len_chars();
    selection_case.app.virtual_editor_state.set_cursor(1, len);
    selection_case
        .app
        .virtual_editor_state
        .move_cursor(5, len, true);
    let _ = selection_case
        .app
        .apply_virtual_commands(&ctx, &[VirtualInputCommand::DeleteToLineStart]);
    assert_eq!(selection_case.app.virtual_editor_buffer.to_string(), "aef");
}
