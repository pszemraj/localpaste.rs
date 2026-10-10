//! Rendered sidebar layout: smart-filter chips and list reveal.

use super::*;

const FILTER_LABELS: [&str; 10] = [
    "All",
    "Today",
    "This Week",
    "Recent (30d)",
    "Unfiled",
    "Documents",
    "Code",
    "Config",
    "Logs",
    "Links",
];

fn sidebar_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 800.0),
        )),
        ..Default::default()
    }
}

/// Returns the rendered rect of each exact text label inside the sidebar column.
fn sidebar_text_rects(output: &egui::FullOutput, label: &str) -> Vec<egui::Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.job.text == label && text.pos.x < 300.0 => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .collect()
}

/// Whether an exact label is painted fully inside its clip rect in the sidebar column.
///
/// Unlike a whole-window search, this ignores the same text shown in the editor
/// pane, so it only passes when the sidebar list itself scrolled to the label.
fn sidebar_label_is_visible(output: &egui::FullOutput, label: &str) -> bool {
    sidebar_visible_label_count(output, label) > 0
}

/// Counts sidebar-column text shapes with this exact label that are not clipped away.
fn sidebar_visible_label_count(output: &egui::FullOutput, label: &str) -> usize {
    output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => {
                text.galley.job.text == label
                    && text.pos.x < 300.0
                    && clipped
                        .clip_rect
                        .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => false,
        })
        .count()
}

#[test]
fn every_smart_filter_is_a_visible_chip_without_an_overflow_menu() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    harness.app.active_collection = crate::backend::SidebarCollection::Links;
    let _ = run_full_update_with_input(&mut harness.app, &ctx, sidebar_input());
    let output = run_full_update_with_input(&mut harness.app, &ctx, sidebar_input());

    let mut row_tops = Vec::new();
    for label in FILTER_LABELS {
        // "Today" is also a list header; the chip is the first, higher one.
        let rect = *sidebar_text_rects(&output, label)
            .first()
            .unwrap_or_else(|| panic!("missing filter chip {label}"));
        assert!(rect.right() <= 300.0, "{label} overflows the sidebar");
        if !row_tops
            .iter()
            .any(|top: &f32| (top - rect.top()).abs() < 1.0)
        {
            row_tops.push(rect.top());
        }
    }
    // The former layout used two four-chip rows plus a row for the `...` menu.
    assert!(row_tops.len() <= 3, "filter rows: {row_tops:?}");
    assert!(sidebar_text_rects(&output, "...").is_empty());
    assert!(sidebar_text_rects(&output, "Active filter is in ...").is_empty());
}

/// Sidebar list of `count` pastes named `Sidebar row N`, newest first.
fn long_sidebar_list(count: usize) -> Vec<PasteSummary> {
    (0..count)
        .map(|index| {
            test_summary(
                &format!("paste-{index}"),
                &format!("Sidebar row {index}"),
                None,
                1,
            )
        })
        .collect()
}

/// Drives sidebar frames with advancing time so smooth scrolling can settle.
struct SidebarFrames {
    ctx: egui::Context,
    time: f64,
}

impl SidebarFrames {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            time: 0.0,
        }
    }

    /// Runs two frames at half-second intervals, delivering `events` on the first.
    fn render(&mut self, app: &mut LocalPasteApp, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = None;
        for events in [events, Vec::new()] {
            self.time += 0.5;
            output = Some(run_full_update_with_input(
                app,
                &self.ctx,
                egui::RawInput {
                    time: Some(self.time),
                    events,
                    ..sidebar_input()
                },
            ));
        }
        output.expect("rendered frame")
    }

    /// Scrolls the sidebar list with a real mouse-wheel event over its rows.
    ///
    /// egui feeds a wheel delta in over many frames, so idle frames follow until
    /// the gesture has fully landed, as when a user pauses before the next action.
    fn wheel(&mut self, app: &mut LocalPasteApp, delta_y: f32) -> egui::FullOutput {
        let mut output = self.render(
            app,
            vec![
                egui::Event::PointerMoved(egui::pos2(150.0, 600.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, delta_y),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..4 {
            output = self.render(app, vec![]);
        }
        output
    }
}

#[test]
fn a_new_selection_scrolls_its_sidebar_row_into_view_once() {
    let mut harness = make_app();
    let rows = long_sidebar_list(60);
    harness.app.all_pastes = rows.clone();
    harness.app.pastes = rows;
    let mut frames = SidebarFrames::new();
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 0"));
    assert!(!sidebar_label_is_visible(&output, "Sidebar row 50"));

    assert!(harness.app.select_paste("paste-50".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(
        sidebar_label_is_visible(&output, "Sidebar row 50"),
        "selection revealed"
    );

    // Manual scrolling away from the selection sticks.
    let output = frames.wheel(&mut harness.app, 4000.0);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 0"));
    assert!(!sidebar_label_is_visible(&output, "Sidebar row 50"));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 0"));
    assert!(
        !sidebar_label_is_visible(&output, "Sidebar row 50"),
        "no re-reveal"
    );

    // The next selection change reveals again.
    assert!(harness.app.select_paste("paste-51".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 51"));
}

/// Builds a 60-row sidebar, selects its last row, and wheels the list to the bottom.
fn sidebar_wheeled_to_bottom() -> (TestHarness, SidebarFrames) {
    let mut harness = make_app();
    let rows = long_sidebar_list(60);
    harness.app.all_pastes = rows.clone();
    harness.app.pastes = rows;
    let mut frames = SidebarFrames::new();
    frames.render(&mut harness.app, vec![]);
    assert!(harness.app.select_paste("paste-59".to_string()));
    frames.render(&mut harness.app, vec![]);
    let output = frames.wheel(&mut harness.app, -4000.0);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 59"));
    assert!(!sidebar_label_is_visible(&output, "Sidebar row 0"));
    (harness, frames)
}

#[test]
fn choosing_a_paste_above_a_wheel_scrolled_list_scrolls_it_into_view() {
    let (mut harness, mut frames) = sidebar_wheeled_to_bottom();

    // The paste picker selects through the same path on Enter.
    assert!(harness.app.select_paste("paste-1".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 1"));
}

#[test]
fn a_new_paste_scrolls_a_wheel_scrolled_list_back_to_the_top() {
    let (mut harness, mut frames) = sidebar_wheeled_to_bottom();

    // Cmd+N: the created paste lands under the first date header and is selected.
    let mut created = Paste::new(String::new(), "Brand new paste".to_string());
    created.id = "new-id".to_string();
    harness
        .app
        .apply_event(CoreEvent::PasteCreated { paste: created });
    assert_eq!(harness.app.selected_id.as_deref(), Some("new-id"));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Brand new paste"));
    // "Today" is both a smart-filter chip and the first date header.
    assert_eq!(
        sidebar_visible_label_count(&output, "Today"),
        2,
        "date header revealed with the first row"
    );
}
