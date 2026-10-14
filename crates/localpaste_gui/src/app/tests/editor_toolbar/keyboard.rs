//! Keyboard ownership around metadata fields and open popups.

use super::*;
use crate::app::virtual_editor::PlatformFlavor;

/// Clicks the rendered text field `id`, which gives it keyboard focus.
fn click_field(app: &mut LocalPasteApp, ctx: &egui::Context, id: &str) {
    let _ = render_at(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH);
    let center = ctx
        .read_response(egui::Id::new(id))
        .unwrap_or_else(|| panic!("{id} is rendered"))
        .rect
        .center();
    let _ = click(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH, center);
    assert!(field_focused(ctx, id), "{id} has focus after a click");
}

/// Clicks the field `id` with `typed` in the same input batch, as a fast typist's
/// first keystroke can arrive with the click that focuses the field.
fn click_field_typing(app: &mut LocalPasteApp, ctx: &egui::Context, id: &str, typed: &str) {
    let _ = render_at(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH);
    let center = ctx
        .read_response(egui::Id::new(id))
        .unwrap_or_else(|| panic!("{id} is rendered"))
        .rect
        .center();
    let mut events = primary_pointer_events(center, true);
    events.push(egui::Event::Text(typed.to_string()));
    for events in [events, primary_pointer_events(center, false)] {
        let _ = run_full_update_with_input(
            app,
            ctx,
            egui::RawInput {
                events,
                ..input_at(LANGUAGE_COMBO_FRAME_WIDTH)
            },
        );
    }
    let _ = render_at(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH);
    assert!(field_focused(ctx, id), "{id} has focus after a click");
}

fn field_focused(ctx: &egui::Context, id: &str) -> bool {
    ctx.memory(|memory| memory.has_focus(egui::Id::new(id)))
}

/// Metadata names sent since the last drain.
fn saved_names(cmd_rx: &Receiver<CoreCmd>) -> Vec<Option<String>> {
    cmd_rx
        .try_iter()
        .filter_map(|cmd| match cmd {
            CoreCmd::UpdatePasteMeta { name, .. } => Some(name),
            _ => None,
        })
        .collect()
}

#[test]
fn escape_in_the_title_restores_its_name_and_saves_nothing() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    click_field(&mut harness.app, &ctx, TITLE_INPUT_ID);
    let _ = type_keys(&mut harness.app, &ctx, "Z", KEYSTROKE_GAP);
    assert_eq!(harness.app.edit_name, "AlphaZ");
    assert!(harness.app.metadata_dirty);
    let _ = saved_names(&harness.cmd_rx);

    let output = press_key(&mut harness.app, &ctx, egui::Key::Escape);
    assert_eq!(harness.app.edit_name, "Alpha");
    assert!(!harness.app.metadata_dirty, "nothing is left to save");
    assert!(editor_text_rect(&output, "Alpha").is_some());
    assert!(
        !field_focused(&ctx, TITLE_INPUT_ID),
        "Escape releases focus"
    );
    assert!(
        saved_names(&harness.cmd_rx).is_empty(),
        "Escape saves nothing"
    );

    // Enter still commits a rename.
    click_field(&mut harness.app, &ctx, TITLE_INPUT_ID);
    let _ = type_keys(&mut harness.app, &ctx, "Z", KEYSTROKE_GAP);
    let _ = press_key(&mut harness.app, &ctx, egui::Key::Enter);
    assert_eq!(
        saved_names(&harness.cmd_rx),
        [Some("AlphaZ".to_string())],
        "Enter saves the rename once"
    );
}

#[test]
fn clicking_away_from_the_title_still_commits_the_rename() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    click_field(&mut harness.app, &ctx, TITLE_INPUT_ID);
    let _ = type_keys(&mut harness.app, &ctx, "Z", KEYSTROKE_GAP);
    let _ = saved_names(&harness.cmd_rx);

    // Empty toolbar space between the metadata and the actions.
    let output = render_at(&mut harness.app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH);
    let apply = editor_text_rect(&output, "Apply").expect("Apply");
    let copy = editor_text_rect(&output, "Copy").expect("Copy");
    let away = egui::pos2((apply.right() + copy.left()) / 2.0, apply.center().y);
    let _ = click(&mut harness.app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH, away);
    assert!(!field_focused(&ctx, TITLE_INPUT_ID));
    assert_eq!(saved_names(&harness.cmd_rx), [Some("AlphaZ".to_string())]);
}

#[test]
fn escape_in_the_title_keeps_earlier_metadata_edits_staged() {
    // A rename staged before the title gained focus, and a language edit.
    for (name_before, language) in [("Alpha draft", None), ("Alpha", Some("python"))] {
        let mut harness = make_app();
        let app = &mut harness.app;
        let ctx = egui::Context::default();
        app.edit_name = name_before.to_string();
        if let Some(language) = language {
            app.edit_language = Some(language.to_string());
            app.edit_language_is_manual = true;
        }
        app.metadata_dirty = true;
        click_field(app, &ctx, TITLE_INPUT_ID);
        let _ = type_keys(app, &ctx, "Z", KEYSTROKE_GAP);
        let _ = saved_names(&harness.cmd_rx);

        let app = &mut harness.app;
        let _ = press_key(app, &ctx, egui::Key::Escape);
        assert_eq!(
            app.edit_name, name_before,
            "the name from focus time returns"
        );
        assert_eq!(app.edit_language.as_deref(), language);
        assert!(app.metadata_dirty, "the earlier edit still needs saving");
        assert!(
            saved_names(&harness.cmd_rx).is_empty(),
            "Escape saves nothing"
        );
    }
}

#[test]
fn escape_in_title_name_and_tags_restores_them() {
    for (id, typed, same_batch) in [
        (TITLE_INPUT_ID, "Z", false),
        (PROPERTIES_NAME_INPUT_ID, "Z", false),
        (PROPERTIES_TAGS_INPUT_ID, "x", false),
        // Typing that arrives with the focusing click is part of the edit Escape cancels.
        (TITLE_INPUT_ID, "Z", true),
        (PROPERTIES_NAME_INPUT_ID, "Z", true),
        (PROPERTIES_TAGS_INPUT_ID, "x", true),
    ] {
        let mut harness = make_app();
        let app = &mut harness.app;
        let ctx = egui::Context::default();
        app.properties_drawer_open = true;
        if same_batch {
            click_field_typing(app, &ctx, id, typed);
        } else {
            click_field(app, &ctx, id);
            let _ = type_keys(app, &ctx, typed, KEYSTROKE_GAP);
        }
        assert!(app.metadata_dirty, "{id}");
        assert!(
            app.edit_name == "AlphaZ" || app.edit_tags == "x",
            "{id} took the typing"
        );

        let _ = press_key(app, &ctx, egui::Key::Escape);
        assert_eq!(app.edit_name, "Alpha", "{id}");
        assert_eq!(app.edit_tags, "", "{id}");
        assert!(!app.metadata_dirty, "{id} leaves nothing to save");
        assert!(!field_focused(&ctx, id), "{id} releases focus");
        assert!(app.properties_drawer_open, "Escape leaves the drawer open");
        assert!(
            !metadata_save_dispatched(&harness.cmd_rx),
            "{id} saves nothing"
        );
    }
}

#[test]
fn escape_in_the_title_after_a_content_ack_leaves_nothing_to_save() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    load_language(&mut harness.app, None, false);
    click_field(&mut harness.app, &ctx, TITLE_INPUT_ID);
    let _ = type_keys(&mut harness.app, &ctx, "Z", KEYSTROKE_GAP);
    let _ = saved_names(&harness.cmd_rx);

    // A content save resolves the language while the title is being edited.
    let mut saved = harness.app.selected_paste.clone().expect("selected paste");
    saved.language = Some("python".to_string());
    saved.language_is_manual = true;
    harness
        .app
        .apply_event(CoreEvent::PasteSaved { paste: saved });
    assert_eq!(harness.app.edit_name, "AlphaZ", "the typed title stays");

    let output = press_key(&mut harness.app, &ctx, egui::Key::Escape);
    assert_eq!(harness.app.edit_name, "Alpha");
    assert!(!harness.app.metadata_dirty, "nothing is left to save");
    assert!(editor_text_rect(&output, "Python").is_some());
    assert!(
        saved_names(&harness.cmd_rx).is_empty(),
        "Escape saves nothing"
    );
}

/// The popups the arrow and delete routing tests open over the editor.
#[derive(Clone, Copy, Debug)]
enum OpenPopup {
    ToolbarLanguage,
    DrawerLanguage,
    MoreMenu,
}

/// Opens `popup` without changing any metadata.
fn open_popup(app: &mut LocalPasteApp, ctx: &egui::Context, popup: OpenPopup) {
    match popup {
        OpenPopup::ToolbarLanguage => {
            let _ = open_combo_showing(app, ctx, LanguageCombo::Toolbar, "Auto");
        }
        OpenPopup::DrawerLanguage => {
            let _ = open_combo_showing(app, ctx, LanguageCombo::Drawer, "Auto");
        }
        OpenPopup::MoreMenu => {
            let output = render_at(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH);
            let more = editor_text_rect(&output, MORE_MENU_LABEL)
                .expect("More button")
                .center();
            let _ = click(app, ctx, LANGUAGE_COMBO_FRAME_WIDTH, more);
        }
    }
    assert!(egui::Popup::is_any_open(ctx), "{popup:?} is open");
}

#[test]
fn arrow_keys_leave_the_selection_alone_while_a_popup_is_open() {
    for popup in [
        OpenPopup::ToolbarLanguage,
        OpenPopup::DrawerLanguage,
        OpenPopup::MoreMenu,
    ] {
        let mut harness = make_app();
        harness
            .app
            .all_pastes
            .push(test_summary("beta", "Beta", None, 4));
        harness.app.pastes = harness.app.all_pastes.clone().into();
        let ctx = egui::Context::default();
        let (key, neighbor) = match harness.app.sidebar_arrow_target_id(1) {
            Some(id) => (egui::Key::ArrowDown, id),
            None => (
                egui::Key::ArrowUp,
                harness.app.sidebar_arrow_target_id(-1).expect("neighbor"),
            ),
        };
        open_popup(&mut harness.app, &ctx, popup);
        if !matches!(popup, OpenPopup::MoreMenu) {
            // A switch would save the type-ahead choice before leaving the paste.
            let _ = type_keys(&mut harness.app, &ctx, "py", TYPEAHEAD_PAUSE);
            assert!(harness.app.metadata_dirty);
        }
        let _ = metadata_save_dispatched(&harness.cmd_rx);

        for arrow in [egui::Key::ArrowDown, egui::Key::ArrowUp] {
            let _ = press_key(&mut harness.app, &ctx, arrow);
            assert_eq!(
                harness.app.selected_id.as_deref(),
                Some("alpha"),
                "{arrow:?} under {popup:?}"
            );
            assert!(egui::Popup::is_any_open(&ctx), "{arrow:?} under {popup:?}");
        }
        assert!(
            !metadata_save_dispatched(&harness.cmd_rx),
            "no switch saved the type-ahead choice under {popup:?}"
        );

        // Once the popup closes, arrows move the sidebar selection again.
        let _ = press_key(&mut harness.app, &ctx, egui::Key::Escape);
        assert!(!egui::Popup::is_any_open(&ctx), "{popup:?}");
        assert!(!harness.app.metadata_dirty, "{popup:?}");
        let _ = press_key(&mut harness.app, &ctx, key);
        assert_eq!(
            harness.app.selected_id.as_deref(),
            Some(neighbor.as_str()),
            "after {popup:?}"
        );
    }
}

#[test]
fn delete_selected_chord_waits_while_a_popup_is_open() {
    let mac_command = egui::Modifiers {
        command: true,
        mac_cmd: true,
        ..Default::default()
    };
    with_platform(PlatformFlavor::Mac, || {
        for popup in [OpenPopup::ToolbarLanguage, OpenPopup::MoreMenu] {
            for key in [egui::Key::Backspace, egui::Key::Delete] {
                let (mut harness, _events) = make_app_with_event_tx();
                let ctx = egui::Context::default();
                open_popup(&mut harness.app, &ctx, popup);
                let delete_sent = |cmd_rx: &Receiver<CoreCmd>| {
                    cmd_rx
                        .try_iter()
                        .any(|cmd| matches!(cmd, CoreCmd::DeletePaste { id } if id == "alpha"))
                };
                let _ = delete_sent(&harness.cmd_rx);

                let chord = egui::RawInput {
                    events: std::iter::once(egui::Event::ModifiersChanged(mac_command))
                        .chain(vec![key_event(key, mac_command)])
                        .collect(),

                    ..input_at(LANGUAGE_COMBO_FRAME_WIDTH)
                };
                let _ = run_full_update_with_input(&mut harness.app, &ctx, chord.clone());
                assert!(!delete_sent(&harness.cmd_rx), "Cmd+{key:?} under {popup:?}");

                // Once the popup closes, the chord deletes the selected paste again.
                let _ = press_key(&mut harness.app, &ctx, egui::Key::Escape);
                let _ = run_full_update_with_input(&mut harness.app, &ctx, chord);
                assert!(
                    delete_sent(&harness.cmd_rx),
                    "Cmd+{key:?} after {popup:?} closes"
                );
            }
        }
    });
}

/// A keyboard shortcut that opens a surface with its own text field.
#[derive(Clone, Copy, Debug)]
enum Surface {
    Palette,
    Picker,
    Help,
    SidebarSearch,
}

impl Surface {
    /// The surface's opening key and modifiers.
    fn chord(self) -> (egui::Key, egui::Modifiers) {
        let command = primary_command_modifiers();
        match self {
            Self::Palette => (egui::Key::K, command),
            Self::Picker => (
                egui::Key::K,
                egui::Modifiers {
                    shift: true,
                    ..command
                },
            ),
            Self::Help => (egui::Key::F1, egui::Modifiers::NONE),
            Self::SidebarSearch => (egui::Key::F, command),
        }
    }

    /// The text the surface's field holds.
    fn query(self, app: &LocalPasteApp) -> &str {
        match self {
            Self::Palette => &app.command_palette_query,
            Self::Picker => &app.paste_picker_query,
            Self::Help => &app.shortcut_help_query,
            Self::SidebarSearch => &app.search_query,
        }
    }
}

#[test]
fn a_surface_opened_over_a_language_popup_takes_the_typing() {
    for surface in [
        Surface::Palette,
        Surface::Picker,
        Surface::Help,
        Surface::SidebarSearch,
    ] {
        for combo in [LanguageCombo::Toolbar, LanguageCombo::Drawer] {
            let mut harness = make_app();
            let app = &mut harness.app;
            let ctx = egui::Context::default();
            let _ = open_combo_showing(app, &ctx, combo, "Auto");

            let (key, modifiers) = surface.chord();
            let _ = run_full_update_with_input(
                app,
                &ctx,
                egui::RawInput {
                    events: std::iter::once(egui::Event::ModifiersChanged(modifiers))
                        .chain(vec![key_event(key, modifiers)])
                        .collect(),

                    ..input_at(LANGUAGE_COMBO_FRAME_WIDTH)
                },
            );
            let _ = render_at(app, &ctx, LANGUAGE_COMBO_FRAME_WIDTH);
            assert!(
                !egui::Popup::is_any_open(&ctx),
                "{surface:?} closes the {combo:?} popup"
            );

            let _ = type_keys(app, &ctx, "py", TYPEAHEAD_PAUSE);
            assert_eq!(surface.query(app), "py", "{surface:?} over {combo:?}");
            assert_eq!(
                app.edit_language, None,
                "{surface:?} typing reached the {combo:?} language"
            );
            assert!(!app.edit_language_is_manual, "{surface:?} over {combo:?}");
            assert!(!app.metadata_dirty, "{surface:?} over {combo:?}");
        }
    }
}

#[test]
fn a_keyboard_opened_language_popup_keeps_its_type_ahead() {
    let mut harness = make_app();
    let app = &mut harness.app;
    let ctx = egui::Context::default();
    click_field(app, &ctx, TITLE_INPUT_ID);
    let _ = press_key(app, &ctx, egui::Key::Tab);
    let combo = ctx
        .memory(|memory| memory.focused())
        .expect("Tab focuses the language combo");
    let _ = press_key(app, &ctx, egui::Key::Space);
    assert!(
        egui::ComboBox::is_open(&ctx, combo),
        "Space opens the popup"
    );

    // The focused combo button leaves typing to its popup.
    let _ = type_keys(app, &ctx, "py", TYPEAHEAD_PAUSE);
    assert!(egui::ComboBox::is_open(&ctx, combo));
    assert_eq!(app.edit_language.as_deref(), Some("python"));
    assert_eq!(app.edit_name, "Alpha");
}
