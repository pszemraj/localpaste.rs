//! Visual-row (wrapped row) layout cache for true virtualization.
//!
//! Scroll domain is visual rows, not physical lines.

use super::buffer::{RopeBuffer, VirtualEditDelta};
use std::ops::Range;

fn div_ceil(value: usize, divisor: usize) -> usize {
    if value == 0 {
        0
    } else {
        (value - 1) / divisor + 1
    }
}

/// Wrap metrics for a single physical line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineWrapMetrics {
    /// Character count of the line excluding trailing CR/LF.
    pub(crate) chars: usize,
    /// Display column count (unicode-width aware).
    pub(crate) columns: usize,
    /// Number of visual rows occupied under current wrap columns.
    pub(crate) visual_rows: usize,
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

        let mut total_rows = 0usize;
        for line in 0..buffer.line_count() {
            let metrics = measure_line(buffer, line, cols);
            total_rows = total_rows.saturating_add(metrics.visual_rows);
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
        let mut replacement_iter = replacement.into_iter();
        if !splice_vec_by_delta(&mut self.line_metrics, delta, new_len, || {
            replacement_iter
                .next()
                .expect("replacement count was validated against delta")
        }) {
            return false;
        }

        self.prefix_rows.truncate(old_start.saturating_add(1));
        let mut total = self.prefix_rows.last().copied().unwrap_or(0);
        for idx in old_start..self.line_metrics.len() {
            total = total.saturating_add(self.line_metrics[idx].visual_rows);
            self.prefix_rows.push(total);
        }
        if self.prefix_rows.len() != self.line_metrics.len() + 1 {
            return false;
        }

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

    /// Cached character length for a line.
    pub(crate) fn line_chars(&self, line: usize) -> usize {
        self.line_metrics.get(line).map(|m| m.chars).unwrap_or(0)
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
        let cols = self.wrap_cols.max(1);

        let line_len = buffer.line_len_chars(line);
        let start_col = row_in_line.saturating_mul(cols).min(line_len);
        let end_col = start_col.saturating_add(cols).min(line_len);
        let start = buffer.line_col_to_char(line, start_col);
        let end = buffer.line_col_to_char(line, end_col);
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
    let chars = buffer.line_len_chars(line);
    let columns = measure_line_columns(buffer, line, chars);
    let visual_rows = div_ceil(columns.max(1), cols).max(1);
    LineWrapMetrics {
        chars,
        columns,
        visual_rows,
    }
}

fn measure_line_columns(buffer: &RopeBuffer, idx: usize, chars: usize) -> usize {
    let line = buffer.rope().line(idx);
    if line.chunks().all(|chunk| chunk.is_ascii()) {
        return chars;
    }

    use unicode_width::UnicodeWidthChar;
    line.chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(1))
        .sum()
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
}
