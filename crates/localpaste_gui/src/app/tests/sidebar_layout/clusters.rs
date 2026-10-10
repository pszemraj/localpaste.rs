//! Rendered cluster gaps in the sidebar list and the interactions around them.

use super::*;

/// Rows per cluster in the 60-row [`clustered_sidebar_list`] most tests use.
const CLUSTER_SIZE: usize = 6;

/// Sidebar list of `count` pastes named `Sidebar row N`, newest first, in clusters
/// of `cluster_size` rows changed a minute apart.
///
/// The first cluster is ten days old and each later one twice the age of the one
/// before, so a cluster gap separates every cluster from the next. Every row falls
/// under the Earlier header whatever the local time of day, so date headers never
/// stand in for a gap.
fn clustered_sidebar_list(count: usize, cluster_size: usize) -> Vec<PasteSummary> {
    let newest = Utc::now();
    (0..count)
        .map(|index| {
            let cluster = (index / cluster_size) as u32;
            let age = chrono::Duration::days(10 * 2_i64.pow(cluster))
                + chrono::Duration::minutes((index % cluster_size) as i64);
            test_summary_at(
                &format!("paste-{index}"),
                &format!("Sidebar row {index}"),
                None,
                1,
                newest - age,
            )
        })
        .collect()
}

/// Row number of a `Sidebar row N` label.
fn row_number(label: &str) -> usize {
    label
        .trim_start_matches("Sidebar row ")
        .parse()
        .expect("sidebar row label")
}

/// Vertical sizes of sidebar rows in the app's style: the distance from one row
/// to the next, and the extra space a cluster gap adds.
fn row_metrics(frames: &SidebarFrames) -> (f32, f32) {
    let spacing = frames.ctx.style().spacing.clone();
    (
        spacing.interact_size.y + spacing.item_spacing.y,
        spacing.item_spacing.y,
    )
}

/// Asserts that neighbouring rows in view sit one row apart, plus one cluster gap
/// exactly where `gap_above` says the lower row starts a new cluster.
///
/// # Returns
/// How many gaps and how many gapless neighbours were checked.
fn assert_row_spacing(
    output: &egui::FullOutput,
    frames: &SidebarFrames,
    gap_above: impl Fn(usize) -> bool,
) -> (usize, usize) {
    let (stride, gap) = row_metrics(frames);
    let rows = visible_sidebar_rows(output);
    let (mut gaps, mut gapless) = (0, 0);
    for pair in rows.windows(2) {
        let (upper, lower) = (&pair[0], &pair[1]);
        let has_gap = gap_above(row_number(&lower.0));
        let expected = if has_gap { stride + gap } else { stride };
        assert!(
            (lower.1 - upper.1 - expected).abs() < 0.5,
            "{} to {}: {} pt apart, expected {expected} (gap: {has_gap}); rows {rows:?}",
            upper.0,
            lower.0,
            lower.1 - upper.1,
        );
        if has_gap {
            gaps += 1;
        } else {
            gapless += 1;
        }
    }
    (gaps, gapless)
}

/// Whether row `index` of a [`CLUSTER_SIZE`] list starts a new cluster.
fn starts_cluster(index: usize) -> bool {
    index > 0 && index.is_multiple_of(CLUSTER_SIZE)
}

/// The selected row's fill rect and the clip rect it was painted under.
fn selected_row_fill(
    output: &egui::FullOutput,
    frames: &SidebarFrames,
) -> (egui::Rect, egui::Rect) {
    let selection = frames.ctx.style().visuals.selection.bg_fill;
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) if rect.fill == selection && rect.rect.left() < 300.0 => {
                Some((rect.rect, clipped.clip_rect))
            }
            _ => None,
        })
        .expect("selected sidebar row painted")
}

/// Asserts the selected row is painted and its whole fill lies inside the list's view.
fn assert_selected_row_fully_in_view(output: &egui::FullOutput, frames: &SidebarFrames) {
    let (row, clip) = selected_row_fill(output, frames);
    assert!(clip.contains_rect(row), "row {row:?} outside view {clip:?}");
}

/// Presses and releases the primary button at `pos` on consecutive frames, then settles.
fn click(frames: &mut SidebarFrames, app: &mut LocalPasteApp, pos: egui::Pos2) {
    for pressed in [true, false] {
        frames.time += 0.05;
        run_full_update_with_input(
            app,
            &frames.ctx,
            egui::RawInput {
                time: Some(frames.time),
                events: primary_pointer_events(pos, pressed),
                ..sidebar_input()
            },
        );
    }
    frames.render(app, vec![]);
}

#[test]
fn rows_changed_far_apart_render_with_a_gap_between_them() {
    let (mut harness, _events, mut frames) =
        sidebar_from_backend(clustered_sidebar_list(60, CLUSTER_SIZE));
    let output = frames.render(&mut harness.app, vec![]);
    let (gaps, gapless) = assert_row_spacing(&output, &frames, starts_cluster);
    assert!(gaps >= 1 && gapless >= 4, "gaps {gaps}, gapless {gapless}");
    assert_eq!(
        sidebar_visible_label_count(&output, "Earlier"),
        1,
        "one date header above every cluster"
    );
}

#[test]
fn a_search_lists_its_results_without_cluster_gaps() {
    let rows = clustered_sidebar_list(60, CLUSTER_SIZE);
    let (mut harness, events, mut frames) = sidebar_from_backend(rows.clone());
    search_sidebar(&mut harness.app, "Sidebar row");
    frames.render(&mut harness.app, vec![]);
    answer_sidebar_search(&harness, &events, rows);
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 60);
    let (gaps, gapless) = assert_row_spacing(&output, &frames, |_| false);
    assert_eq!(gaps, 0);
    assert!(gapless >= 5, "gapless {gapless}");

    // Clearing the search brings the gaps back.
    harness.app.set_search_query(String::new());
    let output = frames.render(&mut harness.app, vec![]);
    let (gaps, _) = assert_row_spacing(&output, &frames, starts_cluster);
    assert!(gaps >= 1, "gaps {gaps}");
}

#[test]
fn smart_filters_keep_cluster_gaps_between_the_rows_they_show() {
    // Every other row is code, so the Code filter shows rows 0, 2, 4, ...
    let mut rows = clustered_sidebar_list(60, CLUSTER_SIZE);
    for row in rows.iter_mut().step_by(2) {
        row.derived.kind = localpaste_core::semantic::PasteKind::Code;
    }
    let (mut harness, _events, mut frames) = sidebar_from_backend(rows);
    harness.app.set_active_collection(SidebarCollection::Code);
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 30);
    // Row 6 starts the second cluster and follows row 4 in the filtered list.
    let (gaps, gapless) = assert_row_spacing(&output, &frames, starts_cluster);
    assert!(gaps >= 1 && gapless >= 2, "gaps {gaps}, gapless {gapless}");
}

#[test]
fn a_selection_below_cluster_gaps_is_revealed_in_full() {
    let (mut harness, _events, mut frames) =
        sidebar_from_backend(clustered_sidebar_list(60, CLUSTER_SIZE));

    // Nine gaps above the last row push it further down than uniform rows would.
    assert!(harness.app.select_paste("paste-59".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 59"));
    assert_selected_row_fully_in_view(&output, &frames);
    let (gaps, _) = assert_row_spacing(&output, &frames, starts_cluster);
    assert!(gaps >= 1, "gaps {gaps}");

    // Manual scrolling still sticks afterwards.
    let output = frames.wheel(&mut harness.app, 4000.0);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 0"));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(!sidebar_label_is_visible(&output, "Sidebar row 59"));
}

#[test]
fn returning_to_all_reveals_the_selection_in_full_below_cluster_gaps() {
    let mut rows = clustered_sidebar_list(60, CLUSTER_SIZE);
    for row in &mut rows[50..] {
        row.derived.kind = localpaste_core::semantic::PasteKind::Code;
    }
    let (mut harness, _events, mut frames) = sidebar_from_backend(rows);
    harness.app.set_active_collection(SidebarCollection::Code);
    frames.render(&mut harness.app, vec![]);
    assert!(harness.app.select_paste("paste-55".to_string()));
    let output = frames.render(&mut harness.app, vec![]);
    assert_selected_row_fully_in_view(&output, &frames);

    harness.app.set_active_collection(SidebarCollection::All);
    let output = frames.render(&mut harness.app, vec![]);
    assert_eq!(harness.app.pastes.len(), 60);
    assert!(sidebar_label_is_visible(&output, "Sidebar row 55"));
    assert_selected_row_fully_in_view(&output, &frames);
    assert_row_spacing(&output, &frames, starts_cluster);
}

#[test]
fn a_cluster_gap_takes_no_click_hover_or_selection_fill() {
    let (mut harness, _events, mut frames) =
        sidebar_from_backend(clustered_sidebar_list(60, CLUSTER_SIZE));
    let output = frames.render(&mut harness.app, vec![]);
    let above = rendered_label_center(&output, "Sidebar row 5");
    let below = rendered_label_center(&output, "Sidebar row 6");
    let (stride, gap) = row_metrics(&frames);
    assert!(
        (below.y - above.y - (stride + gap)).abs() < 0.5,
        "rows 5 and 6 are {} pt apart",
        below.y - above.y
    );
    // Halfway between the two rows lies in the spacing and gap, outside both rows.
    let in_gap = egui::pos2(above.x, (above.y + below.y) / 2.0);
    let selection = frames.ctx.style().visuals.selection.bg_fill;

    let output = frames.render(&mut harness.app, vec![egui::Event::PointerMoved(in_gap)]);
    let fills = fills_behind(&output, in_gap);
    assert!(
        !fills.contains(&COLOR_BORDER) && !fills.contains(&selection),
        "a hovered gap stays unlit: {fills:?}"
    );

    click(&mut frames, &mut harness.app, in_gap);
    assert_eq!(harness.app.selected_id.as_deref(), Some("paste-0"));

    // The row below the gap takes the click, and its selection fill stops at the row.
    click(&mut frames, &mut harness.app, below);
    assert_eq!(harness.app.selected_id.as_deref(), Some("paste-6"));
    let output = frames.render(&mut harness.app, vec![]);
    assert!(fills_behind(&output, below).contains(&selection));
    assert!(!fills_behind(&output, in_gap).contains(&selection));
}

#[test]
fn arrow_keys_move_the_selection_across_a_cluster_gap() {
    let (mut harness, _events, mut frames) =
        sidebar_from_backend(clustered_sidebar_list(60, CLUSTER_SIZE));
    assert!(harness.app.select_paste("paste-5".to_string()));
    frames.render(&mut harness.app, vec![]);

    let down = key_event(egui::Key::ArrowDown, egui::Modifiers::NONE);
    let output = frames.render(&mut harness.app, vec![down]);
    assert_eq!(harness.app.selected_id.as_deref(), Some("paste-6"));
    assert_selected_row_fully_in_view(&output, &frames);
    let (gaps, _) = assert_row_spacing(&output, &frames, starts_cluster);
    assert!(gaps >= 1, "the gap between rows 5 and 6 is in view");

    let up = key_event(egui::Key::ArrowUp, egui::Modifiers::NONE);
    frames.render(&mut harness.app, vec![up]);
    assert_eq!(harness.app.selected_id.as_deref(), Some("paste-5"));
}

#[test]
fn a_long_gapped_list_lays_out_only_the_rows_in_view() {
    // 5,000 rows in ten clusters of 500.
    let (mut harness, _events, mut frames) =
        sidebar_from_backend(clustered_sidebar_list(5000, 500));
    let painted = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    text.galley.job.text.starts_with("Sidebar row ") && text.pos.x < 300.0
                }
                _ => false,
            })
            .count()
    };

    let mut checks = vec![frames.render(&mut harness.app, vec![])];
    assert!(harness.app.select_paste("paste-4999".to_string()));
    checks.push(frames.render(&mut harness.app, vec![]));
    assert!(sidebar_label_is_visible(&checks[1], "Sidebar row 4999"));
    assert_selected_row_fully_in_view(&checks[1], &frames);
    // Revealed upwards, row 2495 lands at the top with the gap above row 2500 in view.
    assert!(harness.app.select_paste("paste-2495".to_string()));
    checks.push(frames.render(&mut harness.app, vec![]));
    assert!(sidebar_label_is_visible(&checks[2], "Sidebar row 2495"));
    let (gaps, _) = assert_row_spacing(&checks[2], &frames, |index| index == 2500);
    assert_eq!(gaps, 1, "row 2500 starts a cluster");

    // Only rows overlapping the view are laid out: at most one partly hidden row at each end.
    for output in &checks {
        let in_view = visible_sidebar_rows(output).len();
        let laid_out = painted(output);
        assert!(
            in_view >= 6 && laid_out <= in_view + 2,
            "{laid_out} rows laid out for {in_view} in view"
        );
    }
}
