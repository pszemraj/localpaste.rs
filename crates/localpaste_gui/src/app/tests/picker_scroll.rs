//! Picker viewport behavior when selection resets before asynchronous results arrive.

use super::*;

fn row_is_visible(output: &egui::FullOutput, label: &str) -> bool {
    output.shapes.iter().any(|clipped| {
        if let egui::Shape::Text(text) = &clipped.shape {
            text.galley.job.text == label
                && clipped
                    .clip_rect
                    .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size()))
        } else {
            false
        }
    })
}

fn deliver_results(app: &mut LocalPasteApp, items: Vec<PasteSummary>) {
    app.paste_picker_sent_scope = app.paste_picker_scope;
    app.palette_search_last_sent = app.paste_picker_query.clone();
    app.apply_event(CoreEvent::PaletteSearchResults {
        query: app.paste_picker_query.clone(),
        scope: app.paste_picker_scope,
        items,
    });
}

#[test]
fn picker_selection_resets_reveal_first_result_after_loading() {
    for reset in [
        "reopen",
        "query",
        "scope",
        "clear query",
        "reopen during scroll",
    ] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        harness.app.ensure_style(&ctx);
        let items: Vec<_> = (0..40)
            .map(|index| {
                test_summary(
                    &format!("paste-{index}"),
                    &format!("Picker row {index}"),
                    Some("text"),
                    1,
                )
            })
            .collect();
        harness.app.all_pastes = items.clone();
        harness.app.open_paste_picker();
        harness.app.set_paste_picker_query("needle".into());
        deliver_results(&mut harness.app, items.clone());
        let mut time = 0.0;
        let mut render = |app: &mut LocalPasteApp, events| {
            time += if reset == "reopen during scroll" {
                1.0 / 60.0
            } else {
                0.5
            };
            ctx.run(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100.0, 800.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| app.render_paste_picker(ctx),
            )
        };
        render(&mut harness.app, vec![]);
        render(&mut harness.app, vec![]);
        for _ in 0..39 {
            render(
                &mut harness.app,
                vec![key_event(egui::Key::ArrowDown, egui::Modifiers::NONE)],
            );
        }
        render(&mut harness.app, vec![]);
        let scrolled = render(&mut harness.app, vec![]);
        if reset != "reopen during scroll" {
            assert!(row_is_visible(&scrolled, "Picker row 39"), "{reset}");
        }
        assert!(!row_is_visible(&scrolled, "Picker row 0"), "{reset}");

        match reset {
            "reopen" | "reopen during scroll" => {
                harness.app.close_paste_picker();
                render(&mut harness.app, vec![]);
                harness.app.open_paste_picker();
            }
            "query" => harness.app.set_paste_picker_query("changed".into()),
            "scope" => harness.app.set_paste_picker_scope(SearchScope::Body),
            "clear query" => harness.app.set_paste_picker_query(String::new()),
            _ => unreachable!(),
        }
        // Loading can span several frames before any rows can consume a reset.
        render(&mut harness.app, vec![]);
        render(&mut harness.app, vec![]);
        if !harness.app.paste_picker_query.is_empty() {
            deliver_results(&mut harness.app, items);
        }
        render(&mut harness.app, vec![]);
        let reopened = render(&mut harness.app, vec![]);
        assert_eq!(harness.app.paste_picker_selected, 0, "{reset}");
        assert!(row_is_visible(&reopened, "Picker row 0"), "{reset}");

        render(
            &mut harness.app,
            vec![
                egui::Event::PointerMoved(egui::pos2(550.0, 250.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -400.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        render(&mut harness.app, vec![]);
        let manually_scrolled = render(&mut harness.app, vec![]);
        assert_eq!(harness.app.paste_picker_selected, 0, "{reset}");
        assert!(
            !row_is_visible(&manually_scrolled, "Picker row 0"),
            "{reset}: idle frames must preserve manual scrolling"
        );
    }
}
