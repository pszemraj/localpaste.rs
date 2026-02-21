//! Regression and correctness tests for visual-row layout behavior.

use super::*;

fn rebuild_cache_for(
    text: &str,
    wrap_width: f32,
    line_height: f32,
    char_width: f32,
) -> (RopeBuffer, VisualRowLayoutCache) {
    let buffer = RopeBuffer::new(text);
    let mut cache = VisualRowLayoutCache::default();
    cache.rebuild(&buffer, wrap_width, line_height, char_width);
    (buffer, cache)
}

fn assert_row_segments(
    text: &str,
    wrap_width: f32,
    expected_wrap_cols: usize,
    expected_segments: &[&str],
) {
    let (buffer, cache) = rebuild_cache_for(text, wrap_width, 10.0, 5.0);
    assert_eq!(cache.wrap_columns(), expected_wrap_cols);
    assert_eq!(cache.line_visual_rows(0), expected_segments.len());

    let mut previous_end = None;
    for (row, expected) in expected_segments.iter().enumerate() {
        let range = cache.row_char_range(&buffer, row);
        if let Some(prev) = previous_end {
            assert_eq!(prev, range.start);
        }
        assert_eq!(buffer.slice_chars(range.clone()), *expected);
        if !expected.is_empty() {
            assert!(range.end > range.start);
        }
        previous_end = Some(range.end);
    }
}

#[test]
fn row_mapping_matches_expected_prefix_sum() {
    let (_buffer, cache) = rebuild_cache_for("1234567890\n12\n123456", 30.0, 10.0, 5.0);
    assert_eq!(cache.total_rows(), 4);
    assert_eq!(cache.row_to_line(0), (0, 0));
    assert_eq!(cache.row_to_line(1), (0, 1));
    assert_eq!(cache.row_to_line(2), (1, 0));
    assert_eq!(cache.row_to_line(3), (2, 0));
    assert_eq!(cache.line_start_row(0), 0);
    assert_eq!(cache.line_start_row(1), 2);
    assert_eq!(cache.line_start_row(2), 3);
}

#[test]
fn apply_delta_matches_full_rebuild_matrix() {
    struct Case {
        text: &'static str,
        replace: Range<usize>,
        replacement: &'static str,
        wrap_width: f32,
        line_height: f32,
        char_width: f32,
    }

    let cases = [
        Case {
            text: "abcdef\nxy\nzz",
            replace: 7..9,
            replacement: "longer-line",
            wrap_width: 20.0,
            line_height: 10.0,
            char_width: 5.0,
        },
        Case {
            text: "one\ntwo\nthree",
            replace: 4..7,
            replacement: "dos\nzwei",
            wrap_width: 40.0,
            line_height: 10.0,
            char_width: 5.0,
        },
    ];

    for case in cases {
        let mut buffer = RopeBuffer::new(case.text);
        let mut cache = VisualRowLayoutCache::default();
        cache.rebuild(&buffer, case.wrap_width, case.line_height, case.char_width);
        let delta = buffer
            .replace_char_range(case.replace, case.replacement)
            .expect("delta");
        assert!(cache.apply_delta(&buffer, delta));

        let mut rebuilt = VisualRowLayoutCache::default();
        rebuilt.rebuild(&buffer, case.wrap_width, case.line_height, case.char_width);
        assert_eq!(cache.total_rows(), rebuilt.total_rows());
        assert_eq!(cache.wrap_columns(), rebuilt.wrap_columns());
        for row in 0..cache.total_rows() {
            assert_eq!(cache.row_to_line(row), rebuilt.row_to_line(row));
        }
    }
}

#[test]
fn rebuild_with_cached_geometry_recovers_after_cache_corruption() {
    let (buffer, mut cache) = rebuild_cache_for("one\ntwo\n", 50.0, 10.0, 5.0);
    let expected_total_rows = cache.total_rows();
    cache.row_index.clear();
    assert!(cache.rebuild_with_cached_geometry(&buffer));
    assert_eq!(cache.row_index.len(), buffer.line_count());
    assert_eq!(cache.total_rows(), expected_total_rows);
}

#[test]
fn apply_delta_uses_incremental_row_index_updates_when_line_count_unchanged() {
    let data_lines = 200_000usize;
    let mut text = String::with_capacity(data_lines.saturating_mul(2).saturating_add(8));
    text.push_str("a\n");
    for _ in 1..data_lines {
        text.push_str("x\n");
    }

    let mut buffer = RopeBuffer::new(text.as_str());
    let mut cache = VisualRowLayoutCache::default();
    cache.rebuild(&buffer, 4.0, 10.0, 1.0);
    let initial_line_count = buffer.line_count();
    let initial_total_rows = cache.total_rows();
    let rebuilds_before = cache.row_index_rebuilds;
    let updates_before = cache.row_index_incremental_updates;

    let delta = buffer.replace_char_range(0..1, "aaaaa").expect("delta");
    assert!(cache.apply_delta(&buffer, delta));
    assert_eq!(buffer.line_count(), initial_line_count);
    assert_eq!(cache.row_index_rebuilds, rebuilds_before);
    assert!(cache.row_index_incremental_updates > updates_before);
    assert_eq!(cache.total_rows(), initial_total_rows.saturating_add(1));
}

#[test]
fn splice_vec_by_delta_preserves_unaffected_prefix_and_suffix() {
    let mut caches = vec![0u32, 1, 2, 3, 4];
    let delta = VirtualEditDelta {
        start_line: 1,
        old_end_line: 2,
        new_end_line: 3,
        char_delta: 0,
    };
    let mut next = 100u32;
    let ok = splice_vec_by_delta(&mut caches, delta, 6, || {
        let id = next;
        next = next.saturating_add(1);
        id
    });
    assert!(ok);
    assert_eq!(caches, vec![0, 100, 101, 102, 3, 4]);
}

#[test]
fn unicode_columns_measurement_uses_wide_char_width() {
    let (_buffer, cache) = rebuild_cache_for("abc\n你好\n🦀\n", 200.0, 10.0, 5.0);
    assert_eq!(cache.line_metrics[0].columns, 3);
    assert_eq!(cache.line_metrics[1].columns, 4);
    assert_eq!(cache.line_metrics[2].columns, 2);
}

#[test]
fn line_column_conversions_round_trip_for_wide_content() {
    let text = "🦀a你b\n";
    let (buffer, cache) = rebuild_cache_for(text, 200.0, 10.0, 5.0);
    assert_eq!(cache.line_columns(&buffer, 0), 6);
    assert_eq!(cache.line_char_to_display_column(&buffer, 0, 0), 0);
    assert_eq!(cache.line_char_to_display_column(&buffer, 0, 1), 2);
    assert_eq!(cache.line_char_to_display_column(&buffer, 0, 2), 3);
    assert_eq!(cache.line_char_to_display_column(&buffer, 0, 3), 5);
    assert_eq!(cache.line_char_to_display_column(&buffer, 0, 4), 6);

    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 0), 0);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 2), 1);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 3), 2);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 4), 2);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 6), 4);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 7), 4);
}

#[test]
fn missing_metrics_fallback_uses_buffer_line_length_for_ascii_lines() {
    let buffer = RopeBuffer::new("abcdef\n");
    let cache = VisualRowLayoutCache::default();

    assert_eq!(cache.line_columns(&buffer, 0), 6);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 3), 3);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 8), 6);
}

#[test]
fn row_char_range_matrix_keeps_wide_and_zero_width_content() {
    let cases = [
        ("🦀🦀🦀🦀🦀🦀\n", 50.0, 10, &["🦀🦀🦀🦀🦀", "🦀"][..]),
        ("🦀\n", 5.0, 1, &["🦀"][..]),
        ("🦀a\n", 5.0, 1, &["🦀", "a"][..]),
        ("\u{0301}ab\n", 5.0, 1, &["\u{0301}a", "b"][..]),
    ];
    for (text, wrap_width, expected_wrap_cols, expected_segments) in cases {
        assert_row_segments(text, wrap_width, expected_wrap_cols, expected_segments);
    }
}

#[test]
fn row_char_ranges_reassemble_original_line_for_mixed_width_content() {
    let (buffer, cache) = rebuild_cache_for("🦀a你b🦀z\n", 25.0, 10.0, 5.0);
    let rows = cache.line_visual_rows(0);
    assert!(rows >= 2);

    let mut rebuilt = String::new();
    let mut previous_end = None;
    for row in 0..rows {
        let range = cache.row_char_range(&buffer, row);
        if let Some(prev) = previous_end {
            assert_eq!(prev, range.start);
        }
        previous_end = Some(range.end);
        rebuilt.push_str(buffer.slice_chars(range).as_str());
    }
    assert_eq!(rebuilt, buffer.line_without_newline(0));
}

#[test]
fn line_display_column_to_char_preserves_leading_zero_width_prefix() {
    let text = "\u{0301}a\u{200D}b\n";
    let (buffer, cache) = rebuild_cache_for(text, 200.0, 10.0, 5.0);

    assert_eq!(cache.line_columns(&buffer, 0), 2);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 0), 0);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 1), 3);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 2), 4);
}

#[test]
fn mixed_width_equal_totals_do_not_trigger_unit_width_shortcuts() {
    let text = "你\u{0301}a\n";
    let (buffer, cache) = rebuild_cache_for(text, 200.0, 10.0, 5.0);
    assert_eq!(cache.line_chars(0), 3);
    assert_eq!(cache.line_columns(&buffer, 0), 3);

    assert_eq!(cache.line_char_to_display_column(&buffer, 0, 1), 2);
    assert_eq!(cache.line_char_to_display_column(&buffer, 0, 2), 2);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 1), 0);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 2), 2);
    assert_eq!(cache.line_display_column_to_char(&buffer, 0, 3), 3);
}

#[test]
fn long_ascii_line_metrics_and_column_mappings_track_full_line() {
    let text = format!("{}\n", "a".repeat(10_500));
    let (buffer, cache) = rebuild_cache_for(text.as_str(), 2000.0, 10.0, 1.0);
    assert_eq!(cache.line_chars(0), buffer.line_len_chars(0));

    let target = 10_400usize;
    assert_eq!(
        cache.line_char_to_display_column(&buffer, 0, target),
        target
    );
    assert_eq!(
        cache.line_display_column_to_char(&buffer, 0, target),
        target
    );
}

#[test]
fn long_non_ascii_line_column_mappings_track_full_line() {
    let text = format!("{}\n", "🦀".repeat(10_300));
    let (buffer, cache) = rebuild_cache_for(text.as_str(), 40_000.0, 10.0, 1.0);

    let target_chars = 10_200usize;
    let target_columns = target_chars.saturating_mul(2);
    assert_eq!(cache.line_chars(0), 10_300);
    assert_eq!(cache.line_columns(&buffer, 0), 20_600);
    assert_eq!(
        cache.line_char_to_display_column(&buffer, 0, target_chars),
        target_columns
    );
    assert_eq!(
        cache.line_display_column_to_char(&buffer, 0, target_columns),
        target_chars
    );
}

#[test]
fn deep_ascii_wrapped_rows_map_directly_to_char_ranges() {
    let text = format!("{}\n", "a".repeat(2_000));
    let (buffer, cache) = rebuild_cache_for(text.as_str(), 50.0, 10.0, 5.0);
    // wrap_width / char_width => 10 cols
    assert_eq!(cache.wrap_columns(), 10);

    let deep_row = 123usize;
    let range = cache.row_char_range(&buffer, deep_row);
    assert_eq!(range.end.saturating_sub(range.start), 10);
    assert_eq!(buffer.slice_chars(range), "a".repeat(10));
}

#[test]
fn word_wrap_prefers_whitespace_boundaries_before_mid_word_splits() {
    assert_row_segments("alpha beta gamma\n", 35.0, 7, &["alpha ", "beta ", "gamma"]);
}

#[test]
fn word_wrap_falls_back_to_mid_word_split_for_long_unbroken_tokens() {
    assert_row_segments(
        "supercalifragilistic\n",
        25.0,
        5,
        &["super", "calif", "ragil", "istic"],
    );
}
