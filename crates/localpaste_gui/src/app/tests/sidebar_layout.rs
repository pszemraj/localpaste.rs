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
