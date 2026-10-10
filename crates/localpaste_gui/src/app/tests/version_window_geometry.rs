//! Diff and History open at their final size and position and keep them while their content changes.

use super::*;
use localpaste_core::models::paste::{VersionMeta, VersionSnapshot};

/// App window size, in points, where Diff was seen jumping on macOS.
const OBSERVED_VIEWPORT: [f32; 2] = [1400.0, 880.0];

/// Viewports that exercise both a roomy window and the clamped default one.
const VIEWPORTS: [[f32; 2]; 2] = [OBSERVED_VIEWPORT, DEFAULT_WINDOW_SIZE];

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

    /// Text painted only on frames where the window is visible, never in its sizing pass.
    fn marker(self) -> &'static str {
        match self {
            Modal::Diff => "Compare current paste against:",
            Modal::History => "Current working copy",
        }
    }

    fn open(self, app: &mut LocalPasteApp) {
        match self {
            Modal::Diff => app.open_diff_modal(),
            Modal::History => app.open_history_modal(),
        }
    }
}

/// Renders one full app frame in `viewport`.
fn frame(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    viewport: [f32; 2],
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    run_full_update_with_input(
        app,
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(viewport[0], viewport[1]),
            )),
            events,
            ..Default::default()
        },
    )
}

/// Renders enough frames for the window to size, fade in, and settle.
fn settle(app: &mut LocalPasteApp, ctx: &egui::Context, viewport: [f32; 2]) -> egui::FullOutput {
    let mut output = frame(app, ctx, viewport, Vec::new());
    for _ in 0..6 {
        output = frame(app, ctx, viewport, Vec::new());
    }
    output
}

fn window_rect(ctx: &egui::Context, modal: Modal) -> egui::Rect {
    ctx.memory(|memory| memory.area_rect(modal.window_id()))
        .unwrap_or_else(|| panic!("{modal:?} window should have an area"))
}

/// Opens `modal` and returns its rect on the first frame that paints it.
fn open_and_capture_first_visible_rect(
    app: &mut LocalPasteApp,
    ctx: &egui::Context,
    viewport: [f32; 2],
    modal: Modal,
) -> egui::Rect {
    modal.open(app);
    for _ in 0..4 {
        let output = frame(app, ctx, viewport, Vec::new());
        if !label_rects(&output, modal.marker(), |_| true).is_empty() {
            return window_rect(ctx, modal);
        }
    }
    panic!("{modal:?} never became visible");
}

/// Clicks the rendered label `text` with a press and a release over two frames.
fn click_label(app: &mut LocalPasteApp, ctx: &egui::Context, viewport: [f32; 2], text: &str) {
    let output = frame(app, ctx, viewport, Vec::new());
    let pos = rendered_label_center(&output, text);
    for pressed in [true, false] {
        frame(app, ctx, viewport, primary_pointer_events(pos, pressed));
    }
}

/// Picks `id` in the Diff candidate list and answers the backend requests it causes.
fn compare_with(harness: &mut TestHarness, ctx: &egui::Context, viewport: [f32; 2], id: &str) {
    let name = format!("Paste {id}");
    click_label(&mut harness.app, ctx, viewport, &name);
    assert_eq!(harness.app.version_ui.diff_target_id.as_deref(), Some(id));

    let mut target = Paste::new(format!("content\n{}", "changed line\n".repeat(80)), name);
    target.id = id.to_string();
    harness
        .app
        .apply_event(CoreEvent::DiffTargetLoaded { paste: target });
    frame(&mut harness.app, ctx, viewport, Vec::new());
    let request_id = harness
        .cmd_rx
        .try_iter()
        .find_map(|cmd| match cmd {
            CoreCmd::ComputeDiffPreview { request_id, .. } => Some(request_id),
            _ => None,
        })
        .expect("a loaded comparison target should request its diff preview");
    harness.app.apply_event(CoreEvent::DiffPreviewComputed {
        request_id,
        diff: localpaste_core::diff::DiffResponse {
            equal: false,
            unified: localpaste_core::diff::unified_diff_lines(
                "content",
                &format!("content\n{}", "changed line\n".repeat(80)),
            ),
        },
    });
}

/// Closes the open version window with `Escape` and lets the page settle.
fn close_with_escape(app: &mut LocalPasteApp, ctx: &egui::Context, viewport: [f32; 2]) {
    frame(
        app,
        ctx,
        viewport,
        vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
    );
    assert!(!app.version_overlay_open());
    settle(app, ctx, viewport);
}

#[test]
fn diff_opens_at_its_final_rect_and_keeps_it_while_comparing() {
    for viewport in VIEWPORTS {
        for candidate_count in [3, 60] {
            let what = format!("{candidate_count} candidates at {viewport:?}");
            let mut harness = make_app();
            let ctx = egui::Context::default();
            harness.app.all_pastes = (0..candidate_count)
                .map(|index| {
                    let id = format!("p{index}");
                    test_summary(&id, &format!("Paste {id}"), Some("rust"), 10)
                })
                .collect();
            settle(&mut harness.app, &ctx, viewport);

            let opened =
                open_and_capture_first_visible_rect(&mut harness.app, &ctx, viewport, Modal::Diff);

            compare_with(&mut harness, &ctx, viewport, "p1");
            let output = settle(&mut harness.app, &ctx, viewport);
            assert!(
                !label_rects(&output, "+changed line", |_| true).is_empty(),
                "{what}: the line diff should be showing"
            );
            assert_eq!(
                window_rect(&ctx, Modal::Diff),
                opened,
                "{what}: picking a candidate moved or resized Diff"
            );

            compare_with(&mut harness, &ctx, viewport, "p2");
            settle(&mut harness.app, &ctx, viewport);
            assert_eq!(
                window_rect(&ctx, Modal::Diff),
                opened,
                "{what}: changing the comparison moved or resized Diff"
            );

            close_with_escape(&mut harness.app, &ctx, viewport);
            let reopened =
                open_and_capture_first_visible_rect(&mut harness.app, &ctx, viewport, Modal::Diff);
            assert_eq!(reopened, opened, "{what}: reopening moved or resized Diff");
        }
    }
}

#[test]
fn history_opens_at_its_final_rect_and_keeps_it_while_loading_versions() {
    for viewport in VIEWPORTS {
        let what = format!("History at {viewport:?}");
        let mut harness = make_app();
        let ctx = egui::Context::default();
        settle(&mut harness.app, &ctx, viewport);

        let opened =
            open_and_capture_first_visible_rect(&mut harness.app, &ctx, viewport, Modal::History);

        let now = chrono::Utc::now();
        let content = "snapshot line\n".repeat(80);
        let meta = |version_id_ms: u64| VersionMeta {
            version_id_ms,
            created_at: now,
            content_hash: format!("hash-{version_id_ms}"),
            len: content.len(),
            language: Some("text".to_string()),
            language_is_manual: false,
        };
        harness.app.apply_event(CoreEvent::PasteVersionsLoaded {
            id: "alpha".to_string(),
            items: (1..=40).rev().map(meta).collect(),
        });
        settle(&mut harness.app, &ctx, viewport);
        assert_eq!(
            window_rect(&ctx, Modal::History),
            opened,
            "{what}: loading the version list moved or resized History"
        );

        harness.app.set_history_selected_index(1);
        frame(&mut harness.app, &ctx, viewport, Vec::new());
        assert_eq!(
            window_rect(&ctx, Modal::History),
            opened,
            "{what}: a loading snapshot moved or resized History"
        );
        harness.app.apply_event(CoreEvent::PasteVersionLoaded {
            snapshot: VersionSnapshot {
                paste_id: "alpha".to_string(),
                version_id_ms: 40,
                created_at: now,
                content_hash: "hash-40".to_string(),
                len: content.len(),
                language: Some("text".to_string()),
                language_is_manual: false,
                content: content.clone(),
            },
        });
        settle(&mut harness.app, &ctx, viewport);
        assert!(harness.app.version_ui.history_snapshot.is_some(), "{what}");
        assert_eq!(
            window_rect(&ctx, Modal::History),
            opened,
            "{what}: showing a snapshot moved or resized History"
        );

        close_with_escape(&mut harness.app, &ctx, viewport);
        let reopened =
            open_and_capture_first_visible_rect(&mut harness.app, &ctx, viewport, Modal::History);
        assert_eq!(
            reopened, opened,
            "{what}: reopening moved or resized History"
        );
    }
}
