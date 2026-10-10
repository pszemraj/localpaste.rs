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
        let style = ctx.style();
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
    ctx.set_style(egui::Style::default());
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
