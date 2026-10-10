//! Rendered sidebar layout: smart-filter chips and list reveal.

use super::picker_scroll::row_is_visible;
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

#[test]
fn a_new_selection_scrolls_its_sidebar_row_into_view_once() {
    let mut harness = make_app();
    let ctx = egui::Context::default();
    let rows: Vec<_> = (0..60)
        .map(|index| {
            test_summary(
                &format!("paste-{index}"),
                &format!("Sidebar row {index}"),
                None,
                1,
            )
        })
        .collect();
    harness.app.all_pastes = rows.clone();
    harness.app.pastes = rows;
    let mut time = 0.0;
    // Two frames per step at half-second intervals let smooth scrolling settle.
    let mut render = |app: &mut LocalPasteApp, events: Vec<egui::Event>| {
        let mut output = None;
        for events in [events, Vec::new()] {
            time += 0.5;
            output = Some(run_full_update_with_input(
                app,
                &ctx,
                egui::RawInput {
                    time: Some(time),
                    events,
                    ..sidebar_input()
                },
            ));
        }
        output.unwrap()
    };
    let output = render(&mut harness.app, vec![]);
    assert!(row_is_visible(&output, "Sidebar row 0"));
    assert!(!row_is_visible(&output, "Sidebar row 50"));

    assert!(harness.app.select_paste("paste-50".to_string()));
    let output = render(&mut harness.app, vec![]);
    assert!(
        row_is_visible(&output, "Sidebar row 50"),
        "selection revealed"
    );

    // Manual scrolling away from the selection sticks.
    render(
        &mut harness.app,
        vec![
            egui::Event::PointerMoved(egui::pos2(150.0, 600.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 4000.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let output = render(&mut harness.app, vec![]);
    assert!(row_is_visible(&output, "Sidebar row 0"));
    assert!(!row_is_visible(&output, "Sidebar row 50"), "no re-reveal");

    // The next selection change reveals again.
    assert!(harness.app.select_paste("paste-51".to_string()));
    let output = render(&mut harness.app, vec![]);
    assert!(row_is_visible(&output, "Sidebar row 51"));
}
