//! Rendered sidebar layout: filter controls, smart-filter chips, and list reveal.

use super::*;

/// Smart-filter chip labels, grouped by the row each renders on.
const FILTER_ROWS: [&[&str]; 2] = [
    &["All", "Today", "This Week", "Recent (30d)"],
    &["Unfiled", "Documents", "Code", "Config", "Logs", "Links"],
];

/// Left edge of sidebar content (8 pt panel margin).
const SIDEBAR_CONTENT_LEFT: f32 = 8.0;

/// Right edge of sidebar content at the default 300 pt panel width (8 pt side margins).
const SIDEBAR_CONTENT_RIGHT: f32 = 292.0;

/// Display scales the chip rows must fit at: macOS 1x and 2x, plus the
/// fractional scales common on Windows and Linux, where text widths snap differently.
const DISPLAY_SCALES: [f32; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];

/// Room each chip row must leave before the sidebar edge at every display scale,
/// so a small label, font, or scale change cannot silently wrap a row.
const CHIP_ROW_MIN_SLACK: f32 = 8.0;

/// Raw input for one frame of a `size` window at `pixels_per_point` display scale.
fn window_input(size: [f32; 2], pixels_per_point: f32) -> egui::RawInput {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(size[0], size[1]),
        )),
        ..Default::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(pixels_per_point);
    input
}

/// Renders two frames in a fresh context so layout settles, returning the second.
///
/// The app installs its theme once per lifetime, so the flag is cleared to give
/// each fresh context the app's fonts and spacing rather than egui's defaults.
fn settled_frame(
    app: &mut LocalPasteApp,
    size: [f32; 2],
    pixels_per_point: f32,
) -> egui::FullOutput {
    app.style_applied = false;
    let ctx = egui::Context::default();
    let _ = run_full_update_with_input(app, &ctx, window_input(size, pixels_per_point));
    run_full_update_with_input(app, &ctx, window_input(size, pixels_per_point))
}

fn sidebar_input() -> egui::RawInput {
    window_input([1100.0, 800.0], 1.0)
}

/// Returns the rendered rect of each exact text label inside the sidebar column.
fn sidebar_text_rects(output: &egui::FullOutput, label: &str) -> Vec<egui::Rect> {
    label_rects(output, label, |pos| pos.x < 300.0)
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

/// Returns the top and the right chip edge of one rendered smart-filter row.
///
/// Chips are left-aligned from the content edge with equal horizontal padding,
/// so the first label's inset is the padding that also follows the last label.
fn chip_row_extent(output: &egui::FullOutput, row: &[&str]) -> (f32, f32) {
    let rects: Vec<egui::Rect> = row
        .iter()
        .map(|label| {
            // "Today" is also a list header; the chip is the first, higher one.
            *sidebar_text_rects(output, label)
                .first()
                .unwrap_or_else(|| panic!("missing filter chip {label}"))
        })
        .collect();
    let top = rects[0].top();
    assert!(
        rects.iter().all(|rect| (rect.top() - top).abs() < 1.0),
        "{row:?} wrapped: {rects:?}"
    );
    let padding = rects[0].left() - SIDEBAR_CONTENT_LEFT;
    (top, rects[rects.len() - 1].right() + padding)
}

#[test]
fn every_smart_filter_is_a_visible_chip_without_an_overflow_menu() {
    let mut harness = make_app();
    harness.app.active_collection = crate::backend::SidebarCollection::Links;
    // Default window at each display scale: time chips on one row, kinds on the next.
    for pixels_per_point in DISPLAY_SCALES {
        let output = settled_frame(&mut harness.app, DEFAULT_WINDOW_SIZE, pixels_per_point);
        let mut row_tops = Vec::new();
        for row in FILTER_ROWS {
            let (top, right) = chip_row_extent(&output, row);
            let slack = SIDEBAR_CONTENT_RIGHT - right;
            assert!(
                slack >= CHIP_ROW_MIN_SLACK,
                "{row:?} ends at x {right:.1} at {pixels_per_point}x, leaving {slack:.1} pt \
                 before the sidebar edge at {SIDEBAR_CONTENT_RIGHT}; keep at least \
                 {CHIP_ROW_MIN_SLACK} pt by shortening a label or tightening chip spacing"
            );
            row_tops.push(top);
        }
        assert!(row_tops[0] < row_tops[1], "filter rows: {row_tops:?}");
        assert!(sidebar_text_rects(&output, "...").is_empty());
        assert!(sidebar_text_rects(&output, "Active filter is in ...").is_empty());
    }
}

#[test]
fn search_scope_and_language_filter_share_one_row() {
    let mut harness = make_app();
    // No paste has a language yet, so the scope combo stands alone.
    let output = settled_frame(&mut harness.app, DEFAULT_WINDOW_SIZE, 1.0);
    assert_eq!(sidebar_text_rects(&output, "All fields").len(), 1);
    assert!(sidebar_text_rects(&output, "All languages").is_empty());

    harness
        .app
        .all_pastes
        .push(test_summary("rusty", "Rusty", Some("rust"), 1));
    let output = settled_frame(&mut harness.app, DEFAULT_WINDOW_SIZE, 1.0);
    let scope = sidebar_text_rects(&output, "All fields")[0];
    let language = sidebar_text_rects(&output, "All languages")[0];
    assert!(
        (scope.top() - language.top()).abs() < 1.0,
        "{scope:?} {language:?}"
    );
    assert!(scope.right() < language.left());
    assert!(language.right() <= SIDEBAR_CONTENT_RIGHT);
    // The combo text names the filter, so no separate label row precedes it.
    assert!(sidebar_text_rects(&output, "Language filter").is_empty());

    // A name longer than the half-width combo truncates instead of overflowing.
    let long_name = "an-improbably-long-language-name";
    harness.app.active_language_filter = Some(long_name.to_string());
    let output = settled_frame(&mut harness.app, DEFAULT_WINDOW_SIZE, 1.0);
    let language = sidebar_text_rects(&output, long_name)[0];
    assert!((scope.top() - language.top()).abs() < 1.0);
    assert!(language.right() <= SIDEBAR_CONTENT_RIGHT, "{language:?}");
}

#[test]
fn the_minimum_window_shows_six_sidebar_rows() {
    let mut harness = make_app();
    let mut rows = long_sidebar_list(60);
    // Detected languages bring up the language filter, as in a real store.
    for row in &mut rows {
        row.language = Some("rust".to_string());
    }
    harness.app.all_pastes = rows.clone();
    harness.app.pastes = rows;
    let output = settled_frame(&mut harness.app, MIN_WINDOW_SIZE, 1.0);
    let visible = (0..60)
        .filter(|index| sidebar_label_is_visible(&output, &format!("Sidebar row {index}")))
        .count();
    // Stacking each filter control on its own row left room for three.
    assert!(visible >= 6, "visible rows: {visible}");
}

/// Sidebar list of `count` pastes named `Sidebar row N`, newest first.
///
/// Update times step back one second per row, so a recomputed list keeps this order.
fn long_sidebar_list(count: usize) -> Vec<PasteSummary> {
    let newest = Utc::now();
    (0..count)
        .map(|index| {
            test_summary_at(
                &format!("paste-{index}"),
                &format!("Sidebar row {index}"),
                None,
                1,
                newest - chrono::Duration::seconds(index as i64),
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

/// Labels of the sidebar rows painted in view, top to bottom, with their vertical positions.
fn visible_sidebar_rows(output: &egui::FullOutput) -> Vec<(String, f32)> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text)
                if text.galley.job.text.starts_with("Sidebar row ")
                    && text.pos.x < 300.0
                    && clipped
                        .clip_rect
                        .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())) =>
            {
                Some((text.galley.job.text.clone(), text.pos.y))
            }
            _ => None,
        })
        .collect()
}

/// Enters `query` in the sidebar search with its debounce already elapsed.
fn search_sidebar(app: &mut LocalPasteApp, query: &str) {
    app.set_search_query(query.to_string());
    app.search_last_input_at = Some(Instant::now() - SEARCH_DEBOUNCE);
}

/// Answers the latest sidebar search the app dispatched with `items`, as the backend would.
fn answer_sidebar_search(
    harness: &TestHarness,
    events: &Sender<CoreEvent>,
    items: Vec<PasteSummary>,
) {
    let request = harness
        .cmd_rx
        .try_iter()
        .filter_map(|cmd| match cmd {
            CoreCmd::SearchPastes {
                scope,
                query,
                collection,
                folder_id,
                language,
                ..
            } => Some((scope, query, collection, folder_id, language)),
            _ => None,
        })
        .last();
    let (scope, query, collection, folder_id, language) =
        request.expect("a dispatched sidebar search");
    events
        .send(CoreEvent::SearchResults {
            collection,
            scope,
            query,
            folder_id,
            language,
            items,
        })
        .expect("app receives backend events");
}

/// Starts a sidebar from a backend list, as at launch, with its first row selected.
fn sidebar_from_backend(
    rows: Vec<PasteSummary>,
) -> (TestHarness, Sender<CoreEvent>, SidebarFrames) {
    let (mut harness, events) = make_app_with_event_tx();
    let mut frames = SidebarFrames::new();
    events
        .send(CoreEvent::PasteList { items: rows })
        .expect("app receives backend events");
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.selected_id.as_deref(), Some("paste-0"));
    assert!(sidebar_label_is_visible(&output, "Sidebar row 0"));
    assert!(!sidebar_label_is_visible(&output, "Sidebar row 50"));
    (harness, events, frames)
}

#[test]
fn clearing_a_search_reveals_the_selected_row_in_the_full_list() {
    let rows = long_sidebar_list(60);
    let (mut harness, events, mut frames) = sidebar_from_backend(rows.clone());

    // Cmd+F, "row 5": rows 5 and 50-59 remain, and the user picks row 50.
    search_sidebar(&mut harness.app, "row 5");
    frames.render(&mut harness.app, vec![]);
    let matches = rows
        .iter()
        .filter(|row| row.name.contains("row 5"))
        .cloned()
        .collect();
    answer_sidebar_search(&harness, &events, matches);
    frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 11);
    assert!(harness.app.select_paste("paste-50".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 50"));

    // Cmd+A, Backspace: the full list returns with the selection still in view.
    harness.app.set_search_query(String::new());
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 60);
    assert_eq!(harness.app.selected_id.as_deref(), Some("paste-50"));
    assert!(
        sidebar_label_is_visible(&output, "Sidebar row 50"),
        "selection revealed in the restored list"
    );
}

/// Sidebar rows where rows 50-59 are code, so the Code smart filter keeps only those.
fn sidebar_list_with_code_tail() -> Vec<PasteSummary> {
    let mut rows = long_sidebar_list(60);
    for row in &mut rows[50..] {
        row.derived.kind = localpaste_core::semantic::PasteKind::Code;
    }
    rows
}

#[test]
fn returning_to_all_reveals_the_row_selected_under_a_smart_filter() {
    let (mut harness, _events, mut frames) = sidebar_from_backend(sidebar_list_with_code_tail());

    harness.app.set_active_collection(SidebarCollection::Code);
    frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 10);
    assert!(harness.app.select_paste("paste-55".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 55"));

    harness.app.set_active_collection(SidebarCollection::All);
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 60);
    assert!(
        sidebar_label_is_visible(&output, "Sidebar row 55"),
        "selection revealed in the All list"
    );
}

#[test]
fn a_filter_change_under_a_search_reveals_once_its_results_arrive() {
    let rows = sidebar_list_with_code_tail();
    let (mut harness, events, mut frames) = sidebar_from_backend(rows.clone());
    harness.app.set_active_collection(SidebarCollection::Code);
    search_sidebar(&mut harness.app, "Sidebar row");
    frames.render(&mut harness.app, vec![]);
    answer_sidebar_search(&harness, &events, rows[50..].to_vec());
    frames.render(&mut harness.app, vec![]);
    assert!(harness.app.select_paste("paste-55".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 55"));

    // All under an active search: the Code results stay on screen until the
    // new reply, with the selection already visible in them.
    harness.app.set_active_collection(SidebarCollection::All);
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 10);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 55"));

    answer_sidebar_search(&harness, &events, rows.clone());
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 60);
    assert!(
        sidebar_label_is_visible(&output, "Sidebar row 55"),
        "selection revealed in the rows the reply brought"
    );
}

/// Selects row 50, then wheels the list part way back up, away from it.
///
/// # Returns
/// The sidebar rows in view afterwards, with their positions.
fn wheel_away_from_row_50(
    harness: &mut TestHarness,
    frames: &mut SidebarFrames,
) -> Vec<(String, f32)> {
    assert!(harness.app.select_paste("paste-50".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 50"));
    let output = frames.wheel(&mut harness.app, 200.0);
    assert!(!sidebar_label_is_visible(&output, "Sidebar row 50"));
    assert!(!sidebar_label_is_visible(&output, "Sidebar row 0"));
    visible_sidebar_rows(&output)
}

/// `rows` as a periodic refresh returns them after an API write: one row's
/// metadata changed and an older paste was added at the end.
fn externally_updated(rows: &[PasteSummary]) -> Vec<PasteSummary> {
    let mut refreshed = rows.to_vec();
    refreshed[10].content_len = 99;
    let oldest = rows[rows.len() - 1].updated_at - chrono::Duration::seconds(1);
    refreshed.push(test_summary_at(
        "paste-60",
        "Sidebar row 60",
        None,
        1,
        oldest,
    ));
    refreshed
}

#[test]
fn a_background_refresh_leaves_a_manually_scrolled_list_in_place() {
    let rows = long_sidebar_list(60);
    let (mut harness, events, mut frames) = sidebar_from_backend(rows.clone());
    let before = wheel_away_from_row_50(&mut harness, &mut frames);

    events
        .send(CoreEvent::PasteList {
            items: externally_updated(&rows),
        })
        .expect("app receives backend events");
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 61);
    assert_eq!(visible_sidebar_rows(&output), before);
}

#[test]
fn a_background_refresh_under_a_search_leaves_a_manually_scrolled_list_in_place() {
    let rows = long_sidebar_list(60);
    let (mut harness, events, mut frames) = sidebar_from_backend(rows.clone());
    search_sidebar(&mut harness.app, "Sidebar row");
    frames.render(&mut harness.app, vec![]);
    answer_sidebar_search(&harness, &events, rows.clone());
    frames.render(&mut harness.app, vec![]);
    let before = wheel_away_from_row_50(&mut harness, &mut frames);

    // A refresh with an external change re-runs the unchanged search.
    let refreshed = externally_updated(&rows);
    events
        .send(CoreEvent::PasteList {
            items: refreshed.clone(),
        })
        .expect("app receives backend events");
    frames.render(&mut harness.app, vec![]);
    answer_sidebar_search(&harness, &events, refreshed);
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 61);
    assert_eq!(visible_sidebar_rows(&output), before);
}

#[test]
fn a_selection_outside_the_search_results_is_revealed_when_the_search_is_cleared() {
    let rows = long_sidebar_list(60);
    let (mut harness, events, mut frames) = sidebar_from_backend(rows.clone());
    search_sidebar(&mut harness.app, "Sidebar row");
    frames.render(&mut harness.app, vec![]);
    answer_sidebar_search(&harness, &events, rows.clone());
    frames.render(&mut harness.app, vec![]);
    assert!(harness.app.select_paste("paste-50".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 50"));

    // A Body-scope search leaves the selection open but out of the results.
    harness.app.set_search_scope(SearchScope::Body);
    frames.render(&mut harness.app, vec![]);
    answer_sidebar_search(&harness, &events, rows[..10].to_vec());
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 10);
    assert_eq!(harness.app.selected_id.as_deref(), Some("paste-50"));
    assert!(!sidebar_label_is_visible(&output, "Sidebar row 50"));

    // Clearing the search brings it back into the list, and into view.
    harness.app.set_search_query(String::new());
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 60);
    assert_eq!(harness.app.selected_id.as_deref(), Some("paste-50"));
    assert!(
        sidebar_label_is_visible(&output, "Sidebar row 50"),
        "selection revealed once it is listed again"
    );
}

#[test]
fn sidebar_row_hover_is_neutral_and_weaker_than_the_selected_row() {
    let mut harness = make_app();
    let rows = long_sidebar_list(10);
    harness.app.all_pastes = rows.clone();
    harness.app.pastes = rows;
    assert!(harness.app.select_paste("paste-0".to_string()));
    let mut frames = SidebarFrames::new();
    let output = frames.render(&mut harness.app, vec![]);
    let resting = rendered_label_center(&output, "Sidebar row 1");
    let resting_fills = fills_behind(&output, resting);

    // Hover resolves against the previous frame's widget rects.
    let output = frames.render(&mut harness.app, vec![egui::Event::PointerMoved(resting)]);
    let fills = fills_behind(&output, resting);
    assert!(
        fills.contains(&COLOR_BORDER) && !resting_fills.contains(&COLOR_BORDER),
        "hover lifts the row from its resting fill: resting {resting_fills:?}, hovered {fills:?}"
    );
    assert!(
        !fills.contains(&COLOR_ACCENT_SURFACE_HOVER) && !fills.contains(&COLOR_ACCENT_SURFACE),
        "sidebar hover must not use the toolbar accent: {fills:?}"
    );
    assert_eq!(
        harness.app.selected_id.as_deref(),
        Some("paste-0"),
        "hover keeps selection"
    );

    // Hovering the selected row leaves it at the selection fill.
    let selected = rendered_label_center(&output, "Sidebar row 0");
    let output = frames.render(&mut harness.app, vec![egui::Event::PointerMoved(selected)]);
    let fills = fills_behind(&output, selected);
    let selection = frames.ctx.style().visuals.selection.bg_fill;
    assert!(
        fills.contains(&selection) && !fills.contains(&COLOR_BORDER),
        "hovered selected row keeps the selection fill: {fills:?}"
    );
}
