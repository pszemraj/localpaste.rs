//! App styling: real startup without a harness style, and themed text colors.

use super::*;
use localpaste_core::env::{env_lock, EnvGuard};

fn pump_until(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    ready: impl Fn(&LocalPasteApp) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        run_full_update_with_input(
            app,
            ctx,
            egui::RawInput {
                system_theme: Some(egui::Theme::Light),
                ..Default::default()
            },
        );
        let style = ctx.global_style();
        assert!(
            style.visuals.dark_mode,
            "system theme must not replace app styling"
        );
        assert!(style
            .text_styles
            .contains_key(&egui::TextStyle::Name(EDITOR_TEXT_STYLE.into())));
        assert_eq!(style.spacing.item_spacing, egui::vec2(12.0, 8.0));
        if ready(app) {
            return;
        }
        assert!(Instant::now() < deadline, "startup did not settle");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn normal_startup_first_paste_and_populated_restart_render_without_test_styles() {
    let _lock = env_lock().lock().unwrap();
    let dir = TempDir::new().unwrap();
    let _db = EnvGuard::set("DB_PATH", dir.path().join("db").to_str().unwrap());
    let _port = EnvGuard::set("PORT", "0");
    let _probe = EnvGuard::remove("LOCALPASTE_NAV_PROBE_LOG");
    let ctx = egui::Context::default();
    let mut app = LocalPasteApp::new().unwrap();
    pump_until(&mut app, &ctx, |_| true);
    assert!(app.selected_id.is_none());
    app.create_new_paste_with_content("first paste café 🦀".into());
    pump_until(&mut app, &ctx, |app| app.selected_paste.is_some());
    let id = app.selected_id.clone().unwrap();
    assert_eq!(app.active_snapshot(), "first paste café 🦀");
    app.flush_pending_saves_for_shutdown();
    drop(app);

    let ctx = egui::Context::default();
    let mut app = LocalPasteApp::new().unwrap();
    pump_until(&mut app, &ctx, |app| app.selected_paste.is_some());
    assert_eq!(app.selected_id.as_deref(), Some(id.as_str()));
    assert_eq!(app.active_snapshot(), "first paste café 🦀");
    // Replacing a native theme can remove named text styles after initialization.
    ctx.set_global_style(egui::Style::default());
    run_full_update(&mut app, &ctx, vec![]);
    assert!(app.virtual_line_height > 0.0);
    app.flush_pending_saves_for_shutdown();
}

/// Returns the color of every rendered text shape whose content equals `text`.
fn rendered_text_colors(output: &egui::FullOutput, text: &str) -> Vec<egui::Color32> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.job.text == text => Some(
                shape
                    .galley
                    .job
                    .sections
                    .first()
                    .map_or(shape.fallback_color, |section| section.format.color),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn placeholders_render_muted_while_widget_text_keeps_body_color() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.properties_drawer_open = true;
    harness.app.edit_name.clear();
    harness.app.edit_tags.clear();
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        ..Default::default()
    };
    let _ = run_full_update_with_input(&mut harness.app, &ctx, input());
    let output = run_full_update_with_input(&mut harness.app, &ctx, input());

    let search_hint = format!(
        "Search pastes... ({})",
        shortcuts::runtime_shortcut_label(RuntimeShortcutAction::FocusSearch)
    );
    for hint in [
        "comma,separated,tags",
        "Untitled paste",
        search_hint.as_str(),
    ] {
        assert_eq!(
            rendered_text_colors(&output, hint),
            vec![COLOR_TEXT_MUTED],
            "placeholder {hint:?}"
        );
    }
    // Headings, buttons, and selectable chips keep the theme's body text color.
    for label in ["Properties", "Copy", "All", "+ New Paste"] {
        let colors = rendered_text_colors(&output, label);
        assert!(!colors.is_empty(), "missing {label:?}");
        assert!(
            colors.iter().all(|color| *color == COLOR_TEXT_PRIMARY),
            "{label:?} rendered {colors:?}"
        );
    }
}

/// Screen bounds of an exact text label; centered galleys extend left of their anchor.
fn text_bounds(output: &egui::FullOutput, text: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.job.text == text => {
                Some(shape.galley.rect.translate(shape.pos.to_vec2()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing rendered text {text:?}"))
}

#[test]
fn empty_editor_centers_its_message_over_muted_shortcut_hints() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.selected_id = None;
    harness.app.selected_paste = None;
    let input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 800.0),
        )),
        ..Default::default()
    };
    let _ = run_full_update_with_input(&mut harness.app, &ctx, input());
    let output = run_full_update_with_input(&mut harness.app, &ctx, input());

    let message = "Select a paste from the sidebar.";
    assert_eq!(
        rendered_text_colors(&output, message),
        vec![COLOR_TEXT_SECONDARY]
    );
    let center = text_bounds(&output, message).center();
    // The editor spans the 300-point sidebar's right edge to the window edge.
    assert!((center.x - 700.0).abs() < 2.0, "message x {}", center.x);
    assert!((center.y - 400.0).abs() < 120.0, "message y {}", center.y);

    let mut last_y = center.y;
    for (description, action) in [
        ("New paste", RuntimeShortcutAction::NewPaste),
        ("Paste as new paste", RuntimeShortcutAction::PlainPaste),
        ("Open a paste", RuntimeShortcutAction::TogglePastePicker),
        (
            "Command palette",
            RuntimeShortcutAction::ToggleCommandPalette,
        ),
        (
            "Keyboard shortcuts",
            RuntimeShortcutAction::ToggleShortcutHelp,
        ),
    ] {
        let keys = shortcuts::runtime_shortcut_label(action);
        for text in [description, keys.as_str()] {
            assert_eq!(rendered_text_colors(&output, text), vec![COLOR_TEXT_MUTED]);
        }
        let description = text_bounds(&output, description);
        let keys = text_bounds(&output, &keys);
        assert!(
            description.right() < keys.left(),
            "keys sit right of {description:?}"
        );
        assert!((description.center().y - keys.center().y).abs() < 1.0);
        assert!(keys.center().y > last_y, "hint lines run top to bottom");
        last_y = keys.center().y;
    }
    let new_paste = shortcuts::runtime_shortcut_label(RuntimeShortcutAction::NewPaste);
    let expected = if cfg!(target_os = "macos") {
        "Cmd+N"
    } else {
        "Ctrl+N"
    };
    assert_eq!(new_paste, expected);
}

#[test]
fn palette_hints_show_the_primary_chord_on_each_platform() {
    for (platform, primary) in [
        (super::virtual_editor::PlatformFlavor::Mac, "Cmd+K"),
        (super::virtual_editor::PlatformFlavor::Other, "Ctrl+K"),
    ] {
        with_platform(platform, || {
            let mut harness = make_app();
            let ctx = egui::Context::default();
            harness.app.selected_id = None;
            harness.app.selected_paste = None;
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = run_full_update_with_input(&mut harness.app, &ctx, input());
            let output = run_full_update_with_input(&mut harness.app, &ctx, input());
            let description = text_bounds(&output, "Command palette");
            let keys = text_bounds(&output, primary);
            assert!(
                (description.center().y - keys.center().y).abs() < 1.0,
                "{primary} labels the Command palette hint"
            );
        });
    }

    // Shortcut help keys the palette row by the primary chord and names the
    // Shift+P alias after its description.
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.shortcut_help_open = true;
    let mut output = run_full_update_with_input(&mut harness.app, &ctx, Default::default());
    for _ in 0..3 {
        output = run_full_update_with_input(&mut harness.app, &ctx, Default::default());
    }
    let (primary, alias) = if cfg!(target_os = "macos") {
        ("Cmd+K", "Cmd+Shift+P")
    } else {
        ("Ctrl+K", "Ctrl+Shift+P")
    };
    let keys = text_bounds(&output, primary);
    let row = text_bounds(&output, &format!("Toggle command palette  also {alias}"));
    assert!(
        keys.right() < row.left() && (keys.center().y - row.center().y).abs() < 2.0,
        "{primary} should key the palette row that names {alias}: {keys:?} {row:?}"
    );
}
