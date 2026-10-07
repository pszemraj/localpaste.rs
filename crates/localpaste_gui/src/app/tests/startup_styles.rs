//! Real startup, first creation, and populated restart without a harness style.

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
