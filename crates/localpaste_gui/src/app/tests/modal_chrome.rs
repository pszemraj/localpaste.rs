//! Floating windows share one title bar style, and the page behind them stays put.

use super::*;

/// Each floating window that opens over the page.
#[derive(Clone, Copy, Debug)]
enum Surface {
    CommandPalette,
    PastePicker,
    ShortcutHelp,
    History,
    Diff,
}

impl Surface {
    const ALL: [Surface; 5] = [
        Surface::CommandPalette,
        Surface::PastePicker,
        Surface::ShortcutHelp,
        Surface::History,
        Surface::Diff,
    ];

    fn window_id(self) -> egui::Id {
        egui::Id::new(match self {
            Surface::CommandPalette => "command_palette_modal",
            Surface::PastePicker => "paste_picker_modal",
            Surface::ShortcutHelp => "Keyboard Shortcuts",
            Surface::History => "History",
            Surface::Diff => "Diff",
        })
    }

    fn open(self, app: &mut LocalPasteApp, ctx: &egui::Context) {
        match self {
            Surface::CommandPalette => app.command_palette_open = true,
            Surface::PastePicker => app.open_paste_picker(),
            Surface::ShortcutHelp => app.open_shortcut_help(ctx),
            Surface::History => app.open_history_modal(),
            Surface::Diff => app.open_diff_modal(),
        }
    }
}

/// Renders enough frames in the standard test viewport for windows to size, fade in, and settle.
fn settle(app: &mut LocalPasteApp, ctx: &egui::Context) -> egui::FullOutput {
    let mut output = run_discovery_frame_once(app, ctx, Vec::new());
    for _ in 0..20 {
        output = run_discovery_frame_once(app, ctx, Vec::new());
    }
    output
}

fn window_rect(ctx: &egui::Context, surface: Surface) -> egui::Rect {
    ctx.memory(|memory| memory.area_rect(surface.window_id()))
        .unwrap_or_else(|| panic!("{surface:?} window should be shown"))
}

/// Presses and releases the primary button at `pos` over two frames.
fn click(app: &mut LocalPasteApp, ctx: &egui::Context, pos: egui::Pos2) {
    for pressed in [true, false] {
        run_discovery_frame_once(app, ctx, primary_pointer_events(pos, pressed));
    }
}

#[test]
fn floating_windows_share_one_title_bar_without_a_collapse_toggle() {
    for surface in Surface::ALL {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        surface.open(&mut harness.app, &ctx);
        let output = settle(&mut harness.app, &ctx);
        let open = window_rect(&ctx, surface);
        let title_y = open.top() + 20.0;
        let fills = fills_behind(&output, egui::pos2(open.center().x, title_y));
        assert!(
            fills.contains(&COLOR_MODAL_CHROME) && !fills.contains(&COLOR_ACCENT_SURFACE),
            "{surface:?} title bar should use the shared modal chrome: {fills:?}"
        );

        // Neither the left edge, where egui puts a collapse arrow, nor a
        // double-click on the title may fold the window to its title bar.
        click(
            &mut harness.app,
            &ctx,
            egui::pos2(open.left() + 24.0, title_y),
        );
        let title = egui::pos2(open.center().x, title_y);
        click(&mut harness.app, &ctx, title);
        click(&mut harness.app, &ctx, title);
        settle(&mut harness.app, &ctx);
        let after = window_rect(&ctx, surface);
        assert!(
            (after.height() - open.height()).abs() <= 0.5,
            "{surface:?} must not collapse: {open:?} became {after:?}"
        );
    }
}

/// Where the open paste's sidebar row and its first editor line are drawn.
///
/// The editor line is told apart from History's preview of the same text by its font.
fn page_anchors(output: &egui::FullOutput) -> (egui::Rect, egui::Rect) {
    let editor_family = egui::FontFamily::Name(EDITOR_FONT_FAMILY.into());
    let editor_lines = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text)
                if text.galley.job.text == "content"
                    && text.galley.job.sections[0].format.font_id.family == editor_family =>
            {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let only = |rects: Vec<egui::Rect>, what: &str| match rects.as_slice() {
        [rect] => *rect,
        _ => panic!("expected one {what}, found {rects:?}"),
    };
    (
        only(
            label_rects(output, "Alpha", |pos| pos.x < 300.0),
            "sidebar row",
        ),
        only(editor_lines, "editor line"),
    )
}

#[test]
fn the_page_behind_history_and_diff_stays_put() {
    for surface in [Surface::History, Surface::Diff] {
        let mut harness = make_app();
        let ctx = egui::Context::default();
        let before = page_anchors(&settle(&mut harness.app, &ctx));

        surface.open(&mut harness.app, &ctx);
        assert!(harness.app.version_overlay_open(), "{surface:?}");
        let output = settle(&mut harness.app, &ctx);
        assert_eq!(
            page_anchors(&output),
            before,
            "{surface:?} moved the page behind it"
        );
        let id_row = label_rects(&output, "alpha", |pos| pos.x > 300.0);
        let reason = label_rects(
            &output,
            "Close History or Diff to create, edit, or delete pastes.",
            |_| true,
        );
        assert!(
            matches!((id_row.as_slice(), reason.as_slice()), ([id], [reason])
                if reason.left() > id.right() && id.y_range().contains(reason.center().y)),
            "the block reason should sit beside the paste id: {id_row:?} {reason:?}"
        );

        run_discovery_frame_once(
            &mut harness.app,
            &ctx,
            vec![key_event(egui::Key::Escape, egui::Modifiers::NONE)],
        );
        assert!(!harness.app.version_overlay_open(), "{surface:?}");
        let closed = page_anchors(&settle(&mut harness.app, &ctx));
        assert_eq!(closed, before, "closing {surface:?} moved the page");
    }
}
