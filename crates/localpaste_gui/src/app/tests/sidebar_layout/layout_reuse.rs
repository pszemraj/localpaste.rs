//! The rendered sidebar keeps its list layout across frames until the rows change.

use super::*;
use std::rc::Rc;

#[test]
fn sidebar_frames_reuse_the_list_layout_until_a_row_changes() {
    let mut harness = make_app();
    let rows = long_sidebar_list(200);
    harness.app.all_pastes = rows.clone();
    harness.app.pastes = rows.into();
    harness.app.select_paste("paste-0".to_string());
    let ctx = egui::Context::default();
    let _ = run_full_update_with_input(&mut harness.app, &ctx, sidebar_input());
    let built = Rc::clone(
        harness
            .app
            .pastes
            .cached_layout()
            .expect("the first frame builds the layout"),
    );

    // No per-frame path may take the rows mutably, or every frame rebuilds the layout.
    for _ in 0..3 {
        let _ = run_full_update_with_input(&mut harness.app, &ctx, sidebar_input());
        let kept = harness.app.pastes.cached_layout().expect("layout kept");
        assert!(Rc::ptr_eq(&built, kept));
    }

    harness.app.pastes[199].updated_at -= chrono::Duration::days(400);
    assert!(harness.app.pastes.cached_layout().is_none());
    let output = run_full_update_with_input(&mut harness.app, &ctx, sidebar_input());
    let rebuilt = harness.app.pastes.cached_layout().expect("layout rebuilt");
    assert!(!Rc::ptr_eq(&built, rebuilt));
    assert!(sidebar_label_is_visible(&output, "Sidebar row 0"));
}
