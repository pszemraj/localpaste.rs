//! Visual-row (wrapped row) layout cache for true virtualization.
//!
//! Scroll domain is visual rows, not physical lines.

use super::buffer::{RopeBuffer, VirtualEditDelta};
use crate::app::MAX_RENDER_CHARS_PER_LINE;
use std::ops::Range;
use unicode_width::UnicodeWidthChar;

/// Wrap metrics for a single physical line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineWrapMetrics {
    /// Character count of the line excluding trailing CR/LF.
    pub(crate) chars: usize,
    /// Display column count (unicode-width aware).
    pub(crate) columns: usize,
    /// Number of visual rows occupied under current wrap columns.
    pub(crate) visual_rows: usize,
    /// True when char index and display column are guaranteed 1:1.
    pub(crate) ascii_only: bool,
    /// True when rendering is capped before physical end-of-line.
    pub(crate) render_capped: bool,
}

/// Visual-row layout cache with prefix-row mapping.
#[derive(Clone, Debug, Default)]
pub(crate) struct VisualRowLayoutCache {
    revision: u64,
    wrap_width: u32,
    line_height_bits: u32,
    char_width_bits: u32,
    wrap_cols: usize,
    line_metrics: Vec<LineWrapMetrics>,
    prefix_rows: Vec<usize>,
    render_capped_lines: usize,
}

impl VisualRowLayoutCache {
    /// Returns true when geometry/text keys no longer match.
    pub(crate) fn needs_rebuild(
        &self,
        revision: u64,
        wrap_width: f32,
        line_height: f32,
        char_width: f32,
        line_count: usize,
    ) -> bool {
        self.revision != revision
            || self.wrap_width != wrap_width.max(0.0).round() as u32
            || self.line_height_bits != line_height.to_bits()
            || self.char_width_bits != char_width.to_bits()
            || self.line_metrics.len() != line_count
            || self.prefix_rows.len() != line_count.saturating_add(1)
    }

    /// Rebuild all line metrics + prefix rows.
    pub(crate) fn rebuild(
        &mut self,
        buffer: &RopeBuffer,
        wrap_width: f32,
        line_height: f32,
        char_width: f32,
    ) {
        let wrap_width_u32 = wrap_width.max(0.0).round() as u32;
        self.revision = buffer.revision();
        self.wrap_width = wrap_width_u32;
        self.line_height_bits = line_height.to_bits();
        self.char_width_bits = char_width.to_bits();

        let cols = ((wrap_width_u32 as f32 / char_width.max(1.0)).floor() as usize).max(1);
        self.wrap_cols = cols;

        self.line_metrics.clear();
        self.prefix_rows.clear();
        self.prefix_rows.push(0);
        self.render_capped_lines = 0;

        let mut total_rows = 0usize;
        for line in 0..buffer.line_count() {
            let metrics = measure_line(buffer, line, cols);
            total_rows = total_rows.saturating_add(metrics.visual_rows);
            if metrics.render_capped {
                self.render_capped_lines = self.render_capped_lines.saturating_add(1);
            }
            self.line_metrics.push(metrics);
            self.prefix_rows.push(total_rows);
        }
    }

    /// Patch-update metrics by edit delta.
    ///
    /// Returns false when caller should do a full rebuild.
    pub(crate) fn apply_delta(&mut self, buffer: &RopeBuffer, delta: VirtualEditDelta) -> bool {
        if self.line_metrics.is_empty() || self.prefix_rows.len() != self.line_metrics.len() + 1 {
            return false;
        }
        if self.char_width_bits == 0 || self.line_height_bits == 0 {
            return false;
        }

        let old_len = self.line_metrics.len();
        let new_len = buffer.line_count();
        let old_start = delta.start_line;
        let old_end_excl = delta.old_end_line.saturating_add(1);

        if old_start >= old_len || old_end_excl > old_len || old_start >= old_end_excl {
            return false;
        }
        if delta.new_end_line >= new_len {
            return false;
        }

        let old_count = old_end_excl - old_start;
        let new_count = delta
            .new_end_line
            .saturating_sub(delta.start_line)
            .saturating_add(1);
        let expected_len = old_len.saturating_sub(old_count).saturating_add(new_count);
        if expected_len != new_len {
            return false;
        }

        let char_width = f32::from_bits(self.char_width_bits).max(1.0);
        let computed_cols = ((self.wrap_width as f32 / char_width).floor() as usize).max(1);
        if computed_cols != self.wrap_cols.max(1) {
            return false;
        }

        let mut replacement = Vec::with_capacity(new_count);
        for line in old_start..=delta.new_end_line {
            replacement.push(measure_line(buffer, line, self.wrap_cols.max(1)));
        }
        let removed_capped = self.line_metrics[old_start..old_end_excl]
            .iter()
            .filter(|metrics| metrics.render_capped)
            .count();
        let added_capped = replacement
            .iter()
            .filter(|metrics| metrics.render_capped)
            .count();
        let mut replacement_iter = replacement.into_iter();
        if !splice_vec_by_delta(&mut self.line_metrics, delta, new_len, || {
            replacement_iter
                .next()
                .expect("replacement count was validated against delta")
        }) {
            return false;
        }

        // We intentionally rebuild the tail prefix sums from `old_start` onward.
        // This is O(remaining_lines), but the loop is tiny integer math and keeps
        // the structure simple until million-line workloads justify a tree/indexed
        // variant.
        self.prefix_rows.truncate(old_start.saturating_add(1));
        let mut total = self.prefix_rows.last().copied().unwrap_or(0);
        for idx in old_start..self.line_metrics.len() {
            total = total.saturating_add(self.line_metrics[idx].visual_rows);
            self.prefix_rows.push(total);
        }
        if self.prefix_rows.len() != self.line_metrics.len() + 1 {
            return false;
        }
        self.render_capped_lines = self
            .render_capped_lines
            .saturating_sub(removed_capped)
            .saturating_add(added_capped);

        self.revision = buffer.revision();
        true
    }

    /// Number of monospace wrap columns.
    pub(crate) fn wrap_columns(&self) -> usize {
        self.wrap_cols.max(1)
    }

    /// Total visual row count.
    pub(crate) fn total_rows(&self) -> usize {
        self.prefix_rows.last().copied().unwrap_or(0)
    }

    /// Returns true when at least one line is render-capped.
    pub(crate) fn has_render_capped_lines(&self) -> bool {
        self.render_capped_lines > 0
    }

    /// Returns number of lines currently affected by render cap.
    pub(crate) fn render_capped_line_count(&self) -> usize {
        self.render_capped_lines
    }

    /// Cached character length for a line.
    pub(crate) fn line_chars(&self, line: usize) -> usize {
        self.line_metrics.get(line).map(|m| m.chars).unwrap_or(0)
    }

    /// Cached display column width for a line.
    pub(crate) fn line_columns(&self, buffer: &RopeBuffer, line: usize) -> usize {
        if line >= buffer.line_count() {
            return 0;
        }
        self.line_metrics
            .get(line)
            .map(|m| m.columns)
            .unwrap_or_else(|| {
                let line_chars = buffer.line_len_chars(line).min(MAX_RENDER_CHARS_PER_LINE);
                measure_line_columns(buffer, line, line_chars).0
            })
    }

    /// Convert a character offset within a line to display columns.
    pub(crate) fn line_char_to_display_column(
        &self,
        buffer: &RopeBuffer,
        line: usize,
        char_column: usize,
    ) -> usize {
        if line >= buffer.line_count() {
            return 0;
        }
        let line_chars = buffer.line_len_chars(line).min(MAX_RENDER_CHARS_PER_LINE);
        let metrics = self.line_metrics.get(line).copied().unwrap_or_else(|| {
            let (columns, ascii_only) = measure_line_columns(buffer, line, line_chars);
            LineWrapMetrics {
                chars: line_chars,
                columns,
                visual_rows: 1,
                ascii_only,
                render_capped: buffer.line_len_chars(line) > MAX_RENDER_CHARS_PER_LINE,
            }
        });
        let char_column = char_column.min(metrics.chars);
        if metrics.ascii_only {
            return char_column;
        }

        let mut consumed_columns = 0usize;
        let line_slice = buffer.rope().line(line).slice(..line_chars);
        for (idx, ch) in line_slice.chars().enumerate() {
            if idx >= char_column {
                break;
            }
            consumed_columns =
                consumed_columns.saturating_add(UnicodeWidthChar::width(ch).unwrap_or(1));
        }
        consumed_columns
    }

    /// Convert display columns within a line to a character offset.
    pub(crate) fn line_display_column_to_char(
        &self,
        buffer: &RopeBuffer,
        line: usize,
        target_columns: usize,
    ) -> usize {
        if line >= buffer.line_count() {
            return 0;
        }
        let fallback_chars = buffer.line_len_chars(line);
        let fallback_chars = fallback_chars.min(MAX_RENDER_CHARS_PER_LINE);
        let metrics = self.line_metrics.get(line).copied().unwrap_or_else(|| {
            let (columns, ascii_only) = measure_line_columns(buffer, line, fallback_chars);
            LineWrapMetrics {
                chars: fallback_chars,
                columns,
                visual_rows: 1,
                ascii_only,
                render_capped: buffer.line_len_chars(line) > MAX_RENDER_CHARS_PER_LINE,
            }
        });
        let target_columns = target_columns.min(metrics.columns);
        line_char_for_display_columns(buffer, line, metrics, target_columns)
    }

    /// Cached visual-row count for a line.
    pub(crate) fn line_visual_rows(&self, line: usize) -> usize {
        self.line_metrics
            .get(line)
            .map(|m| m.visual_rows.max(1))
            .unwrap_or(1)
    }

    /// Start visual row for a physical line.
    #[cfg(test)]
    pub(crate) fn line_start_row(&self, line: usize) -> usize {
        self.prefix_rows
            .get(line)
            .copied()
            .unwrap_or_else(|| self.total_rows())
    }

    /// Map visual row to (physical line, row in that line).
    pub(crate) fn row_to_line(&self, row: usize) -> (usize, usize) {
        if self.line_metrics.is_empty() || self.prefix_rows.len() != self.line_metrics.len() + 1 {
            return (0, 0);
        }
        let total = self.total_rows().max(1);
        let row = row.min(total.saturating_sub(1));

        let mut lo = 0usize;
        let mut hi = self.line_metrics.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.prefix_rows[mid + 1] <= row {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let line = lo.min(self.line_metrics.len().saturating_sub(1));
        let max_row = self.line_visual_rows(line).saturating_sub(1);
        let row_in_line = row.saturating_sub(self.prefix_rows[line]).min(max_row);
        (line, row_in_line)
    }

    /// Global char range covered by a visual row.
    pub(crate) fn row_char_range(&self, buffer: &RopeBuffer, row: usize) -> Range<usize> {
        let (line, row_in_line) = self.row_to_line(row);
        let metrics = self
            .line_metrics
            .get(line)
            .copied()
            .unwrap_or_else(|| LineWrapMetrics {
                chars: buffer.line_len_chars(line).min(MAX_RENDER_CHARS_PER_LINE),
                columns: buffer.line_len_chars(line).min(MAX_RENDER_CHARS_PER_LINE),
                visual_rows: 1,
                ascii_only: false,
                render_capped: buffer.line_len_chars(line) > MAX_RENDER_CHARS_PER_LINE,
            });
        let local_range = line_row_char_range(
            buffer,
            line,
            metrics.chars,
            self.wrap_cols.max(1),
            row_in_line,
        );
        let start = buffer.line_col_to_char(line, local_range.start);
        let end = buffer.line_col_to_char(line, local_range.end.max(local_range.start));
        start..end
    }
}

/// Generic splice helper for line-aligned vectors patched by VirtualEditDelta.
pub(crate) fn splice_vec_by_delta<T, F>(
    vec: &mut Vec<T>,
    delta: VirtualEditDelta,
    new_len: usize,
    mut make_new: F,
) -> bool
where
    F: FnMut() -> T,
{
    let old_len = vec.len();
    if old_len == 0 {
        return false;
    }
    let old_start = delta.start_line;
    let old_end_excl = delta.old_end_line.saturating_add(1);
    if old_start >= old_len || old_end_excl > old_len || old_start >= old_end_excl {
        return false;
    }

    let old_count = old_end_excl - old_start;
    let new_count = delta
        .new_end_line
        .saturating_sub(delta.start_line)
        .saturating_add(1);
    let expected_len = old_len.saturating_sub(old_count).saturating_add(new_count);
    if expected_len != new_len {
        return false;
    }

    vec.splice(old_start..old_end_excl, (0..new_count).map(|_| make_new()));
    vec.len() == new_len
}

fn measure_line(buffer: &RopeBuffer, line: usize, cols: usize) -> LineWrapMetrics {
    let full_chars = buffer.line_len_chars(line);
    let chars = full_chars.min(MAX_RENDER_CHARS_PER_LINE);
    let (columns, ascii_only) = measure_line_columns(buffer, line, chars);
    let visual_rows = measure_line_visual_rows(buffer, line, chars, cols.max(1));
    LineWrapMetrics {
        chars,
        columns,
        visual_rows,
        ascii_only,
        render_capped: full_chars > MAX_RENDER_CHARS_PER_LINE,
    }
}

fn measure_line_columns(buffer: &RopeBuffer, idx: usize, chars: usize) -> (usize, bool) {
    let line_slice = buffer.rope().line(idx).slice(..chars);
    if line_slice.chunks().all(|chunk| chunk.is_ascii()) {
        return (chars, true);
    }

    use unicode_width::UnicodeWidthChar;
    let columns = line_slice
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(1))
        .sum();
    (columns, false)
}

fn measure_line_visual_rows(buffer: &RopeBuffer, line: usize, chars: usize, cols: usize) -> usize {
    if chars == 0 {
        return 1;
    }

    let cols = cols.max(1);
    let mut rows = 1usize;
    let mut row_columns = 0usize;
    let line_slice = buffer.rope().line(line).slice(..chars);
    for ch in line_slice.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if width == 0 {
            continue;
        }
        // Wrap only after at least one visible glyph has been placed. This
        // lets over-wide glyphs at row start consume the current row.
        if row_columns > 0 && row_columns.saturating_add(width) > cols {
            rows = rows.saturating_add(1);
            row_columns = 0;
        }
        row_columns = row_columns.saturating_add(width);
    }

    rows.max(1)
}

fn line_row_char_range(
    buffer: &RopeBuffer,
    line: usize,
    chars: usize,
    cols: usize,
    row_in_line: usize,
) -> Range<usize> {
    if chars == 0 {
        return 0..0;
    }

    let cols = cols.max(1);
    let mut current_row = 0usize;
    let mut row_start = 0usize;
    let mut row_columns = 0usize;
    let line_slice = buffer.rope().line(line).slice(..chars);
    for (idx, ch) in line_slice.chars().enumerate() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(1);
        // Only wrap after at least one visible glyph has been placed in this row.
        // This prevents empty leading rows when a single glyph is wider than `cols`.
        if width > 0 && row_columns > 0 && row_columns.saturating_add(width) > cols {
            if current_row == row_in_line {
                return row_start..idx;
            }
            current_row = current_row.saturating_add(1);
            row_start = idx;
            row_columns = 0;
        }
        if width > 0 {
            row_columns = row_columns.saturating_add(width);
        }
    }

    if current_row == row_in_line {
        row_start..chars
    } else {
        chars..chars
    }
}

fn line_char_for_display_columns(
    buffer: &RopeBuffer,
    line: usize,
    metrics: LineWrapMetrics,
    target_columns: usize,
) -> usize {
    if metrics.ascii_only {
        return target_columns.min(metrics.chars);
    }

    use unicode_width::UnicodeWidthChar;

    let mut consumed_columns = 0usize;
    let mut consumed_chars = 0usize;
    let line_slice = buffer.rope().line(line).slice(..metrics.chars);
    for ch in line_slice.chars() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if width == 0 {
            // Keep leading zero-width codepoints anchored to visual column 0 so
            // row starts/cursor mapping never skip them.
            if target_columns == 0 && consumed_columns == 0 {
                continue;
            }
            consumed_chars = consumed_chars.saturating_add(1);
            continue;
        }
        if consumed_columns.saturating_add(width) > target_columns {
            break;
        }
        consumed_columns = consumed_columns.saturating_add(width);
        consumed_chars = consumed_chars.saturating_add(1);
    }
    consumed_chars
}

#[cfg(test)]
mod tests {
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
    fn row_char_range_for_wide_glyph_lines_does_not_drop_second_row_content() {
        assert_row_segments("🦀🦀🦀🦀🦀🦀\n", 50.0, 10, &["🦀🦀🦀🦀🦀", "🦀"]);
    }

    #[test]
    fn row_char_range_does_not_emit_empty_first_row_for_single_wide_glyph() {
        assert_row_segments("🦀\n", 5.0, 1, &["🦀"]);
    }

    #[test]
    fn row_char_range_wrap_cols_one_wide_plus_ascii_preserves_both_rows() {
        assert_row_segments("🦀a\n", 5.0, 1, &["🦀", "a"]);
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
    fn row_char_range_keeps_leading_zero_width_codepoints_in_first_row() {
        assert_row_segments("\u{0301}ab\n", 5.0, 1, &["\u{0301}a", "b"]);
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
    fn long_line_metrics_cap_rendered_chars_and_report_capped_lines() {
        let text = format!(
            "{}\n",
            "a".repeat(MAX_RENDER_CHARS_PER_LINE.saturating_add(250))
        );
        let (buffer, cache) = rebuild_cache_for(text.as_str(), 200.0, 10.0, 5.0);

        assert_eq!(buffer.line_len_chars(0), MAX_RENDER_CHARS_PER_LINE + 250);
        assert_eq!(cache.line_chars(0), MAX_RENDER_CHARS_PER_LINE);
        assert!(cache.has_render_capped_lines());
        assert_eq!(cache.render_capped_line_count(), 1);
    }

    #[test]
    fn long_line_column_mappings_clamp_at_render_cap() {
        let text = format!(
            "{}\n",
            "a".repeat(MAX_RENDER_CHARS_PER_LINE.saturating_add(500))
        );
        let (buffer, cache) = rebuild_cache_for(text.as_str(), 2000.0, 10.0, 1.0);

        let beyond = MAX_RENDER_CHARS_PER_LINE.saturating_add(400);
        assert_eq!(
            cache.line_char_to_display_column(&buffer, 0, beyond),
            MAX_RENDER_CHARS_PER_LINE
        );
        assert_eq!(
            cache.line_display_column_to_char(&buffer, 0, beyond),
            MAX_RENDER_CHARS_PER_LINE
        );
    }

    #[test]
    fn long_non_ascii_line_column_mappings_clamp_at_render_cap() {
        let text = format!(
            "{}\n",
            "🦀".repeat(MAX_RENDER_CHARS_PER_LINE.saturating_add(300))
        );
        let (buffer, cache) = rebuild_cache_for(text.as_str(), 40_000.0, 10.0, 1.0);

        let capped_columns = MAX_RENDER_CHARS_PER_LINE.saturating_mul(2);
        let beyond_chars = MAX_RENDER_CHARS_PER_LINE.saturating_add(200);
        let beyond_columns = capped_columns.saturating_add(400);

        assert_eq!(cache.line_chars(0), MAX_RENDER_CHARS_PER_LINE);
        assert_eq!(cache.line_columns(&buffer, 0), capped_columns);
        assert_eq!(
            cache.line_char_to_display_column(&buffer, 0, beyond_chars),
            capped_columns
        );
        assert_eq!(
            cache.line_display_column_to_char(&buffer, 0, beyond_columns),
            MAX_RENDER_CHARS_PER_LINE
        );
    }
}
