//! Diff and History windows stay inside the viewport at every supported window size.

use super::super::ui::WINDOW_EDGE_MARGIN;
use super::*;
use localpaste_core::models::paste::{VersionMeta, VersionSnapshot};

#[derive(Clone, Copy, Debug)]
enum Modal {
    Diff,
    History,
}

impl Modal {
    fn window_id(self) -> egui::Id {
        egui::Id::new(match self {
            Modal::Diff => "Diff",
            Modal::History => "History",
        })
    }
}

fn viewport(size: [f32; 2]) -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]))
}

/// Renders enough frames for the window to size, fade in, and settle.
fn settle(app: &mut LocalPasteApp, ctx: &egui::Context, size: [f32; 2]) {
    for _ in 0..8 {
        let _ = run_full_update_with_input(
            app,
            ctx,
            egui::RawInput {
                screen_rect: Some(viewport(size)),
                ..Default::default()
            },
        );
    }
}

/// Opens `modal` over content wide and tall enough to stress its minimum size.
fn open_modal(harness: &mut TestHarness, modal: Modal) {
    let long_name = "an extremely long paste name ".repeat(12);
    harness.app.all_pastes = (0..60)
        .map(|index| test_summary(&format!("p{index}"), long_name.trim(), Some("rust"), 10))
        .collect();
    set_active_content(&mut harness.app, &"wide line of text ".repeat(40));
    match modal {
        Modal::Diff => {
            harness.app.open_diff_modal();
            assert!(harness.app.version_ui.diff_modal_open);
        }
        Modal::History => {
            let now = chrono::Utc::now();
            let content = "wide snapshot line ".repeat(60);
            let meta = |version_id_ms: u64| VersionMeta {
                version_id_ms,
                created_at: now,
                content_hash: format!("hash-{version_id_ms}"),
                len: content.len(),
                language: Some("text".to_string()),
                language_is_manual: false,
            };
            harness.app.open_history_modal();
            assert!(harness.app.version_ui.history_modal_open);
            harness.app.version_ui.history_versions = (1..=40).map(meta).collect();
            harness.app.version_ui.history_selected_index = 1;
            harness.app.version_ui.history_snapshot = Some(VersionSnapshot {
                paste_id: "alpha".to_string(),
                version_id_ms: 1,
                created_at: now,
                content_hash: "hash-1".to_string(),
                len: content.len(),
                language: Some("text".to_string()),
                language_is_manual: false,
                content,
            });
        }
    }
}

fn window_rect(ctx: &egui::Context, modal: Modal) -> egui::Rect {
    ctx.memory(|memory| memory.area_rect(modal.window_id()))
        .expect("modal window")
}

/// Slack for the window frame's stroke, which may sit just outside the fitted bounds.
const STROKE_TOLERANCE: f32 = 1.0;

/// The window, close button included, must keep the promised edge margin inside the viewport.
fn assert_fits(rect: egui::Rect, size: [f32; 2], what: &str) {
    let allowed = viewport(size).shrink(WINDOW_EDGE_MARGIN - STROKE_TOLERANCE);
    assert!(
        allowed.contains_rect(rect),
        "{what}: window {rect:?} must fit inside {allowed:?}"
    );
}

#[test]
fn diff_and_history_fit_the_default_and_minimum_window_sizes() {
    for size in [DEFAULT_WINDOW_SIZE, MIN_WINDOW_SIZE] {
        for modal in [Modal::Diff, Modal::History] {
            let mut harness = make_app();
            let ctx = egui::Context::default();
            open_modal(&mut harness, modal);
            settle(&mut harness.app, &ctx, size);
            assert_fits(
                window_rect(&ctx, modal),
                size,
                &format!("{modal:?} at {size:?}"),
            );
        }
    }
}

#[test]
fn diff_and_history_shrink_when_the_window_shrinks_while_open() {
    let large = [1700.0, 1000.0];
    for modal in [Modal::Diff, Modal::History] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        open_modal(&mut harness, modal);
        settle(&mut harness.app, &ctx, large);
        let roomy = window_rect(&ctx, modal);
        assert_fits(roomy, large, &format!("{modal:?} at {large:?}"));

        for size in [DEFAULT_WINDOW_SIZE, MIN_WINDOW_SIZE] {
            settle(&mut harness.app, &ctx, size);
            assert_fits(
                window_rect(&ctx, modal),
                size,
                &format!("{modal:?} shrunk from {large:?} to {size:?}"),
            );
        }
    }
}

#[test]
fn diff_and_history_keep_their_preferred_size_in_a_large_viewport() {
    let large = [1700.0, 1000.0];
    for (modal, preferred_width) in [(Modal::Diff, 1180.0), (Modal::History, 1080.0)] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        open_modal(&mut harness, modal);
        settle(&mut harness.app, &ctx, large);
        let rect = window_rect(&ctx, modal);
        assert!(
            (rect.width() - preferred_width).abs() < 48.0,
            "{modal:?} should keep its {preferred_width} point content width: {rect:?}"
        );
        assert!(
            rect.height() > 700.0,
            "{modal:?} should keep its tall default: {rect:?}"
        );
    }
}
