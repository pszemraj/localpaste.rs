//! Rendered pointer and floating-window interaction regressions.

use super::*;

fn review_frame(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let modifiers = events
        .iter()
        .rev()
        .find_map(|event| match event {
            egui::Event::PointerButton { modifiers, .. } | egui::Event::Key { modifiers, .. } => {
                Some(*modifiers)
            }
            _ => None,
        })
        .unwrap_or_default();
    run_full_update_with_input(
        app,
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            events,
            modifiers,
            ..Default::default()
        },
    )
}

fn rendered_label_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() / 2.0)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing rendered label {label}"))
}

fn review_click(app: &mut LocalPasteApp, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        review_frame(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn clipped_editor_row_does_not_accept_a_press_above_the_viewport() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    let text = "alpha beta gamma delta\n".repeat(120);
    harness.app.reset_virtual_editor(&text);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    let height = harness.app.virtual_line_height;
    harness.app.virtual_pending_scroll_offset_y = Some(height * 30.0 + height / 2.0);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    harness
        .app
        .virtual_editor_state
        .restore_selection(1200, Some(1100), text.len());
    let viewport = harness.app.virtual_viewport.rect.unwrap();
    let pos = egui::pos2(viewport.left() + 100.0, viewport.top() - height / 3.0);

    review_click(&mut harness.app, &ctx, pos);

    assert_eq!(
        (
            harness.app.virtual_editor_state.cursor(),
            harness.app.virtual_editor_state.anchor()
        ),
        (1200, Some(1100)),
        "clipped editor rows must not receive presses outside their viewport"
    );
}

#[test]
fn dragging_the_floating_scrollbar_preserves_editor_selection() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    let text = "alpha beta gamma delta omega\n".repeat(240);
    harness.app.reset_virtual_editor(&text);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    let len = harness.app.virtual_editor_buffer.len_chars();
    harness
        .app
        .virtual_editor_state
        .restore_selection(5, Some(24), len);
    let selection_before = harness.app.virtual_editor_state.selection_range();
    let viewport = harness.app.virtual_viewport.rect.unwrap();
    let thumb = egui::pos2(viewport.right() - 1.0, viewport.top() + 5.0);
    let dragged = thumb + egui::vec2(0.0, 140.0);
    let offset_before = harness.app.virtual_viewport.offset_y;

    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(thumb),
            egui::Event::PointerButton {
                pos: thumb,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    review_frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::PointerMoved(dragged)],
    );
    review_frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::PointerButton {
            pos: dragged,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );

    assert!(
        harness.app.virtual_viewport.offset_y > offset_before,
        "dragging the scrollbar thumb should move the viewport"
    );
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        selection_before,
        "scrollbar interaction must not become an editor row click"
    );
    assert!(!harness.app.virtual_drag_active);
}

#[test]
fn drag_start_survives_first_movement_crossing_multiple_rows() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    let text = "alpha beta gamma delta omega\n".repeat(80);
    harness.app.reset_virtual_editor(&text);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    let viewport = harness.app.virtual_viewport.rect.unwrap();
    let line_height = harness.app.virtual_line_height;
    let start = egui::pos2(viewport.left() + 70.0, viewport.top() + line_height * 1.5);
    let crossed_rows = start + egui::vec2(35.0, line_height * 5.0);

    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let anchor_cursor = harness.app.virtual_editor_state.cursor();
    review_frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::PointerMoved(crossed_rows)],
    );

    assert!(harness.app.virtual_drag_active);
    assert!(harness.app.virtual_editor_state.cursor() > anchor_cursor);
    assert!(harness.app.virtual_editor_state.selection_range().is_some());
}

#[test]
fn first_drag_motion_and_release_in_one_frame_extends_selection() {
    for (release_outside_window, shift) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let (mut harness, _event_tx) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        let text = "alpha beta gamma delta omega\n".repeat(160);
        harness.app.reset_virtual_editor(&text);
        for _ in 0..3 {
            review_frame(&mut harness.app, &ctx, vec![]);
        }
        let viewport = harness.app.virtual_viewport.rect.unwrap();
        let line_height = harness.app.virtual_line_height;
        let start = egui::pos2(viewport.left() + 70.0, viewport.top() + line_height * 1.5);
        let release = if release_outside_window {
            egui::pos2(viewport.center().x, 940.0)
        } else {
            egui::pos2(viewport.right() - 120.0, viewport.top() + line_height * 7.5)
        };
        if release_outside_window {
            assert!(ctx.layer_id_at(release).is_none());
        }
        if shift {
            harness
                .app
                .virtual_editor_state
                .restore_selection(8, Some(2), text.len());
        }
        let press_modifiers = if shift {
            egui::Modifiers::SHIFT
        } else {
            egui::Modifiers::NONE
        };

        review_frame(
            &mut harness.app,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: press_modifiers,
                },
            ],
        );
        let anchor_cursor = harness.app.virtual_editor_state.cursor();
        let expected_anchor = if shift { 2 } else { anchor_cursor };
        review_frame(
            &mut harness.app,
            &ctx,
            vec![
                egui::Event::PointerMoved(release),
                egui::Event::PointerButton {
                    pos: release,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );

        assert!(
            harness.app.virtual_editor_state.cursor() > anchor_cursor,
            "the release-frame drag endpoint must advance the cursor; outside={release_outside_window}, shift={shift}"
        );
        assert!(
            harness.app.virtual_editor_state.selection_range().is_some(),
            "the release-frame drag must preserve the press anchor; outside={release_outside_window}, shift={shift}"
        );
        assert_eq!(
            harness.app.virtual_editor_state.anchor(),
            Some(expected_anchor),
            "the accepted press must remain the drag anchor; outside={release_outside_window}, shift={shift}"
        );
        assert_eq!(
            harness
                .app
                .virtual_editor_state
                .selection_range()
                .unwrap()
                .start,
            expected_anchor,
            "the selection must begin at the accepted press anchor; outside={release_outside_window}, shift={shift}"
        );
        assert!(!harness.app.virtual_drag_active);
    }
}

#[test]
fn coalesced_drag_release_over_overlay_preserves_press_selection() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    let text = "alpha beta gamma delta omega\n".repeat(160);
    harness.app.reset_virtual_editor(&text);
    harness.app.open_shortcut_help(&ctx);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    let viewport = harness.app.virtual_viewport.rect.unwrap();
    let overlay = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("Keyboard Shortcuts")))
        .unwrap();
    let release = overlay.center();
    let overlay_layer = ctx.layer_id_at(release).unwrap();
    let start = (0..(viewport.height() / harness.app.virtual_line_height) as usize)
        .flat_map(|row| {
            let y = viewport.top() + harness.app.virtual_line_height * (row as f32 + 0.5);
            [
                egui::pos2(viewport.left() + 40.0, y),
                egui::pos2(viewport.right() - 120.0, y),
            ]
        })
        .find(|pos| {
            viewport.contains(*pos)
                && !overlay.contains(*pos)
                && ctx
                    .layer_id_at(*pos)
                    .is_some_and(|layer| layer != overlay_layer)
        })
        .expect("shortcut window must leave an editor row exposed");
    assert_ne!(ctx.layer_id_at(release), ctx.layer_id_at(start));

    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(harness.app.virtual_pointer_press_modifiers.is_some());
    let selection_after_press = harness.app.virtual_editor_state.selection_range();
    let cursor_after_press = harness.app.virtual_editor_state.cursor();
    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(release),
            egui::Event::PointerButton {
                pos: release,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );

    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        cursor_after_press
    );
    assert_eq!(
        harness.app.virtual_editor_state.selection_range(),
        selection_after_press
    );
    assert!(!harness.app.virtual_drag_active);
}

#[test]
fn pointer_release_applies_the_final_owned_drag_position() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    let text = "alpha beta gamma delta omega\nsecond line\n";
    harness.app.reset_virtual_editor(text);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    let origin = harness.app.virtual_viewport.caret.unwrap().center();
    let start = origin + egui::vec2(40.0, 0.0);
    let middle = origin + egui::vec2(85.0, 0.0);
    let end = origin + egui::vec2(155.0, 0.0);
    review_click(&mut harness.app, &ctx, end);
    let expected_end = harness.app.virtual_editor_state.cursor();
    harness.app.reset_virtual_click_streak();
    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    review_frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::PointerMoved(middle)],
    );
    review_frame(&mut harness.app, &ctx, vec![]);
    assert!(harness.app.virtual_drag_active);
    let intermediate = harness.app.virtual_editor_state.cursor();
    assert_ne!(intermediate, expected_end);

    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(end),
            egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );

    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        expected_end,
        "release must finish the owned drag at its final position"
    );
    assert!(!harness.app.virtual_drag_active);
}

#[test]
fn owned_drag_tracks_and_autoscrolls_outside_the_viewport_and_window() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    let text = "alpha beta gamma delta omega\n".repeat(160);
    harness.app.reset_virtual_editor(&text);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    let viewport = harness.app.virtual_viewport.rect.unwrap();
    let start = egui::pos2(viewport.left() + 40.0, viewport.center().y);
    let middle = start + egui::vec2(80.0, 12.0);
    let outside_window = egui::pos2(viewport.center().x, 940.0);
    assert!(ctx.layer_id_at(outside_window).is_none());

    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    review_frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::PointerMoved(middle)],
    );
    assert!(harness.app.virtual_drag_active);
    let cursor_inside = harness.app.virtual_editor_state.cursor();
    let offset_before = harness.app.virtual_viewport.offset_y;

    let endpoint_output = review_frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::PointerMoved(outside_window)],
    );
    let cursor_outside = harness.app.virtual_editor_state.cursor();
    assert!(endpoint_output
        .viewport_output
        .values()
        .any(|viewport| viewport.repaint_delay == Duration::ZERO));
    let first_repaint_output = review_frame(&mut harness.app, &ctx, vec![]);
    let offset_after_first_repaint = harness.app.virtual_viewport.offset_y;
    assert!(first_repaint_output
        .viewport_output
        .values()
        .any(|viewport| viewport.repaint_delay == Duration::ZERO));
    review_frame(&mut harness.app, &ctx, vec![]);

    assert!(harness.app.virtual_drag_active);
    assert!(
        cursor_outside > cursor_inside,
        "an owned drag outside the window should extend to the last rendered row"
    );
    assert!(
        offset_after_first_repaint > offset_before,
        "an owned drag below the window should start autoscrolling"
    );
    assert!(
        harness.app.virtual_viewport.offset_y > offset_after_first_repaint,
        "autoscroll should continue on repaint while the pointer stays held outside the window"
    );

    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(outside_window),
            egui::Event::PointerButton {
                pos: outside_window,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(!harness.app.virtual_drag_active);
}

#[test]
fn pointer_release_over_an_overlay_does_not_move_the_drag_selection() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    let text = "alpha beta gamma delta omega\nsecond line\n";
    harness.app.reset_virtual_editor(text);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    let origin = harness.app.virtual_viewport.caret.unwrap().center();
    let start = origin + egui::vec2(40.0, 0.0);
    let middle = origin + egui::vec2(85.0, 0.0);
    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    review_frame(
        &mut harness.app,
        &ctx,
        vec![egui::Event::PointerMoved(middle)],
    );
    review_frame(&mut harness.app, &ctx, vec![]);
    assert!(harness.app.virtual_drag_active);
    let cursor_before_overlay = harness.app.virtual_editor_state.cursor();
    let offset_before_overlay = harness.app.virtual_viewport.offset_y;

    harness.app.open_shortcut_help(&ctx);
    for _ in 0..3 {
        review_frame(&mut harness.app, &ctx, vec![]);
    }
    let overlay = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("Keyboard Shortcuts")))
        .unwrap();
    let release = overlay.center();
    review_frame(
        &mut harness.app,
        &ctx,
        vec![
            egui::Event::PointerMoved(release),
            egui::Event::PointerButton {
                pos: release,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );

    assert_eq!(
        harness.app.virtual_editor_state.cursor(),
        cursor_before_overlay,
        "an overlay-owned release must not update the editor drag"
    );
    assert_eq!(
        harness.app.virtual_viewport.offset_y, offset_before_overlay,
        "an overlay-owned release must not autoscroll the editor"
    );
    assert!(!harness.app.virtual_drag_active);
}

#[test]
fn find_mouse_navigation_retains_query_focus_for_enter_paste_and_escape() {
    let (mut harness, _event_tx) = make_app_with_event_tx();
    let ctx = egui::Context::default();
    harness.app.reset_virtual_editor("needle alpha needle beta");
    harness.app.open_editor_find();
    harness.app.set_editor_find_query("needle".into());
    review_frame(&mut harness.app, &ctx, vec![]);
    for (label, active) in [("Next", 1), ("Prev", 0)] {
        let output = review_frame(&mut harness.app, &ctx, vec![]);
        review_click(
            &mut harness.app,
            &ctx,
            rendered_label_center(&output, label),
        );
        assert_eq!(harness.app.editor_find.active_match, Some(active));
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(EDITOR_FIND_INPUT_ID))));
    }
    review_frame(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Enter, egui::Modifiers::NONE)],
    );
    assert_eq!(harness.app.editor_find.active_match, Some(1));
    review_frame(&mut harness.app, &ctx, vec![egui::Event::Paste("X".into())]);
    assert!(harness.app.editor_find.query.contains('X'));
    assert_eq!(harness.app.active_snapshot(), "needle alpha needle beta");
    assert!(!harness
        .cmd_rx
        .try_iter()
        .any(|cmd| matches!(cmd, CoreCmd::CreatePaste { .. })));
    review_frame(
        &mut harness.app,
        &ctx,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!harness.app.editor_find.open);
    assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(VIRTUAL_EDITOR_ID))));
}

#[test]
fn floating_help_buttons_preserve_the_underlying_caret_and_selection() {
    for button in ["Clear", "Close", "left edge", "right edge"] {
        let (mut harness, _event_tx) = make_app_with_event_tx();
        let ctx = egui::Context::default();
        let text = "editor text below help\n".repeat(80);
        harness.app.reset_virtual_editor(&text);
        harness
            .app
            .virtual_editor_state
            .restore_selection(300, Some(290), text.chars().count());
        harness.app.focus_editor_next = true;
        review_frame(&mut harness.app, &ctx, vec![]);
        harness.app.open_shortcut_help(&ctx);
        harness.app.shortcut_help_query = "undo".into();
        for _ in 0..3 {
            review_frame(&mut harness.app, &ctx, vec![]);
        }
        let output = review_frame(&mut harness.app, &ctx, vec![]);
        let pos = if button.ends_with("edge") {
            let window = ctx
                .memory(|memory| memory.area_rect(egui::Id::new("Keyboard Shortcuts")))
                .unwrap();
            egui::pos2(
                if button == "left edge" {
                    window.left() + 2.0
                } else {
                    window.right() - 2.0
                },
                window.center().y,
            )
        } else {
            rendered_label_center(&output, button)
        };
        review_click(&mut harness.app, &ctx, pos);
        assert_eq!(
            (
                harness.app.virtual_editor_state.cursor(),
                harness.app.virtual_editor_state.anchor()
            ),
            (300, Some(290)),
            "{button}"
        );
        if harness.app.shortcut_help_open {
            harness.app.close_shortcut_help(&ctx);
        }
        review_frame(&mut harness.app, &ctx, vec![egui::Event::Text("W".into())]);
        assert_eq!(harness.app.virtual_editor_buffer.slice_chars(290..291), "W");
    }
}

#[test]
fn shift_click_extends_editor_selection_from_its_existing_anchor() {
    for (cursor, anchor) in [(1, None), (8, Some(2)), (2, Some(8))] {
        for (drag, shift) in [(false, true), (true, true), (true, false)] {
            let (mut harness, _event_tx) = make_app_with_event_tx();
            let ctx = egui::Context::default();
            let text = "alpha beta gamma\nnext line\n";
            harness.app.reset_virtual_editor(text);
            for _ in 0..3 {
                review_frame(&mut harness.app, &ctx, vec![]);
            }
            let pos = harness.app.virtual_viewport.caret.unwrap().center() + egui::vec2(105.0, 0.0);
            let mut expected_anchor = anchor.unwrap_or(cursor);
            harness
                .app
                .virtual_editor_state
                .restore_selection(cursor, anchor, text.len());
            review_frame(
                &mut harness.app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: if shift {
                            egui::Modifiers::SHIFT
                        } else {
                            egui::Modifiers::NONE
                        },
                    },
                ],
            );
            if !shift {
                expected_anchor = harness.app.virtual_editor_state.cursor();
            }
            if drag {
                // Shift is released before egui recognizes the drag start.
                review_frame(
                    &mut harness.app,
                    &ctx,
                    vec![egui::Event::PointerMoved(pos + egui::vec2(25.0, 0.0))],
                );
            }
            let end = if drag {
                pos + egui::vec2(25.0, 0.0)
            } else {
                pos
            };
            review_frame(
                &mut harness.app,
                &ctx,
                vec![egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(
                harness.app.virtual_editor_state.anchor(),
                Some(expected_anchor),
                "cursor={cursor}, anchor={anchor:?}, drag={drag}"
            );
            assert!(harness.app.virtual_editor_state.cursor() > expected_anchor);
        }
    }
}
