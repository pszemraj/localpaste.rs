//! Visual-row (wrapped row) layout cache for true virtualization.
//!
//! Scroll domain is visual rows, not physical lines.

use super::buffer::{RopeBuffer, VirtualEditDelta};
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
}

#[derive(Clone, Debug, Default)]
struct RowFenwick {
    tree: Vec<usize>,
}

impl RowFenwick {
    fn len(&self) -> usize {
        self.tree.len().saturating_sub(1)
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.tree.clear();
    }

    fn rebuild_from_metrics(&mut self, metrics: &[LineWrapMetrics]) {
        let len = metrics.len();
        self.tree.clear();
        self.tree.resize(len.saturating_add(1), 0);
        for (idx, entry) in metrics.iter().enumerate() {
            self.tree[idx.saturating_add(1)] = entry.visual_rows;
        }
        for idx in 1..=len {
            let parent = idx.saturating_add(idx & idx.wrapping_neg());
            if parent <= len {
                self.tree[parent] = self.tree[parent].saturating_add(self.tree[idx]);
            }
        }
    }

    fn total_rows(&self) -> usize {
        self.prefix_sum_exclusive(self.len())
    }

    fn prefix_sum_exclusive(&self, end: usize) -> usize {
        let mut idx = end.min(self.len());
        let mut sum = 0usize;
        while idx > 0 {
            sum = sum.saturating_add(self.tree[idx]);
            idx = idx.saturating_sub(idx & idx.wrapping_neg());
        }
        sum
    }

    fn add_signed(&mut self, line: usize, diff: isize) -> bool {
        if diff == 0 {
            return true;
        }
        let len = self.len();
        if line >= len {
            return false;
        }
        let delta = diff.unsigned_abs();
        let mut idx = line.saturating_add(1);
        while idx <= len {
            if diff > 0 {
                self.tree[idx] = self.tree[idx].saturating_add(delta);
            } else {
                let Some(next) = self.tree[idx].checked_sub(delta) else {
                    return false;
                };
                self.tree[idx] = next;
            }
            idx = idx.saturating_add(idx & idx.wrapping_neg());
        }
        true
    }

    fn line_for_row(&self, row: usize) -> Option<usize> {
        let len = self.len();
        if len == 0 {
            return None;
        }
        let mut bit = 1usize;
        while bit < len {
            bit <<= 1;
        }

        let mut idx = 0usize;
        let mut consumed = 0usize;
        let mut step = bit;
        while step > 0 {
            let next = idx.saturating_add(step);
            if next <= len {
                let candidate = consumed.saturating_add(self.tree[next]);
                if candidate <= row {
                    idx = next;
                    consumed = candidate;
                }
            }
            step >>= 1;
        }
        Some(idx.min(len.saturating_sub(1)))
    }
}

/// Visual-row layout cache with Fenwick-backed row index mapping.
#[derive(Clone, Debug, Default)]
pub(crate) struct VisualRowLayoutCache {
    revision: u64,
    wrap_width: u32,
    line_height_bits: u32,
    char_width_bits: u32,
    wrap_cols: usize,
    line_metrics: Vec<LineWrapMetrics>,
    // Optional per-line row boundaries (`row_start_char` for each visual row +
    // trailing sentinel end). ASCII-only lines use O(1) arithmetic and store `None`.
    line_row_boundaries: Vec<Option<Box<[usize]>>>,
    row_index: RowFenwick,
    #[cfg(test)]
    row_index_rebuilds: u64,
    #[cfg(test)]
    row_index_incremental_updates: u64,
}

impl VisualRowLayoutCache {
    fn rebuild_row_index_from_metrics(&mut self) {
        self.row_index.rebuild_from_metrics(&self.line_metrics);
        #[cfg(test)]
        {
            self.row_index_rebuilds = self.row_index_rebuilds.saturating_add(1);
        }
    }

    fn apply_row_index_delta(&mut self, line: usize, diff: isize) -> bool {
        if diff == 0 {
            return true;
        }
        if !self.row_index.add_signed(line, diff) {
            return false;
        }
        #[cfg(test)]
        {
            self.row_index_incremental_updates =
                self.row_index_incremental_updates.saturating_add(1);
        }
        true
    }

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
            || self.line_row_boundaries.len() != line_count
            || self.row_index.len() != line_count
    }

    /// Rebuild all line metrics + row index.
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
        self.line_row_boundaries.clear();

        for line in 0..buffer.line_count() {
            let (metrics, row_boundaries) = measure_line(buffer, line, cols);
            self.line_metrics.push(metrics);
            self.line_row_boundaries.push(row_boundaries);
        }
        self.rebuild_row_index_from_metrics();
    }

    /// Patch-update metrics by edit delta.
    ///
    /// Returns false when caller should do a full rebuild.
    pub(crate) fn apply_delta(&mut self, buffer: &RopeBuffer, delta: VirtualEditDelta) -> bool {
        if self.line_metrics.is_empty()
            || self.row_index.len() != self.line_metrics.len()
            || self.line_row_boundaries.len() != self.line_metrics.len()
        {
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

        let old_visual_rows: Vec<usize> = self.line_metrics[old_start..old_end_excl]
            .iter()
            .map(|metrics| metrics.visual_rows)
            .collect();
        let mut replacement = Vec::with_capacity(new_count);
        let mut replacement_row_boundaries = Vec::with_capacity(new_count);
        for line in old_start..=delta.new_end_line {
            let (metrics, row_boundaries) = measure_line(buffer, line, self.wrap_cols.max(1));
            replacement.push(metrics);
            replacement_row_boundaries.push(row_boundaries);
        }
        let mut replacement_iter = replacement.into_iter();
        if !splice_vec_by_delta(&mut self.line_metrics, delta, new_len, || {
            replacement_iter
                .next()
                .expect("replacement count was validated against delta")
        }) {
            return false;
        }
        let mut row_boundaries_iter = replacement_row_boundaries.into_iter();
        if !splice_vec_by_delta(&mut self.line_row_boundaries, delta, new_len, || {
            row_boundaries_iter
                .next()
                .expect("replacement count was validated against delta")
        }) {
            return false;
        }
        if old_count != new_count {
            self.rebuild_row_index_from_metrics();
        } else {
            for (offset, old_rows) in old_visual_rows.iter().enumerate() {
                let new_rows = self.line_metrics[old_start.saturating_add(offset)].visual_rows;
                let diff = if new_rows >= *old_rows {
                    let delta = new_rows.saturating_sub(*old_rows);
                    match isize::try_from(delta) {
                        Ok(value) => value,
                        Err(_) => return false,
                    }
                } else {
                    let delta = old_rows.saturating_sub(new_rows);
                    match isize::try_from(delta) {
                        Ok(value) => -value,
                        Err(_) => return false,
                    }
                };
                if !self.apply_row_index_delta(old_start.saturating_add(offset), diff) {
                    return false;
                }
            }
        }

        self.revision = buffer.revision();
        true
    }

    /// Rebuild from cached geometry if prior measurements are available.
    pub(crate) fn rebuild_with_cached_geometry(&mut self, buffer: &RopeBuffer) -> bool {
        if self.char_width_bits == 0 || self.line_height_bits == 0 {
            return false;
        }
        let wrap_width = self.wrap_width as f32;
        let line_height = f32::from_bits(self.line_height_bits);
        let char_width = f32::from_bits(self.char_width_bits);
        if !line_height.is_finite()
            || line_height <= 0.0
            || !char_width.is_finite()
            || char_width <= 0.0
        {
            return false;
        }
        self.rebuild(buffer, wrap_width, line_height, char_width);
        true
    }

    /// Number of monospace wrap columns.
    pub(crate) fn wrap_columns(&self) -> usize {
        self.wrap_cols.max(1)
    }

    /// Total visual row count.
    pub(crate) fn total_rows(&self) -> usize {
        self.row_index.total_rows()
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
                let line_chars = buffer.line_len_chars(line);
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
        let line_chars = buffer.line_len_chars(line);
        let metrics = self.line_metrics.get(line).copied().unwrap_or_else(|| {
            let (columns, ascii_only) = measure_line_columns(buffer, line, line_chars);
            LineWrapMetrics {
                chars: line_chars,
                columns,
                visual_rows: 1,
                ascii_only,
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
        let metrics = self.line_metrics.get(line).copied().unwrap_or_else(|| {
            let (columns, ascii_only) = measure_line_columns(buffer, line, fallback_chars);
            LineWrapMetrics {
                chars: fallback_chars,
                columns,
                visual_rows: 1,
                ascii_only,
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
        if line > self.line_metrics.len() {
            return self.total_rows();
        }
        self.row_index.prefix_sum_exclusive(line)
    }

    /// Map visual row to (physical line, row in that line).
    pub(crate) fn row_to_line(&self, row: usize) -> (usize, usize) {
        if self.line_metrics.is_empty() || self.row_index.len() != self.line_metrics.len() {
            return (0, 0);
        }
        let total = self.total_rows().max(1);
        let row = row.min(total.saturating_sub(1));
        let line = self
            .row_index
            .line_for_row(row)
            .unwrap_or(0)
            .min(self.line_metrics.len().saturating_sub(1));
        let max_row = self.line_visual_rows(line).saturating_sub(1);
        let line_start = self.row_index.prefix_sum_exclusive(line);
        let row_in_line = row.saturating_sub(line_start).min(max_row);
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
                chars: buffer.line_len_chars(line),
                columns: buffer.line_len_chars(line),
                visual_rows: 1,
                ascii_only: false,
            });
        let local_range = if metrics.ascii_only {
            let cols = self.wrap_cols.max(1);
            let start = row_in_line.saturating_mul(cols).min(metrics.chars);
            let end = start.saturating_add(cols).min(metrics.chars);
            start..end
        } else if let Some(row_boundaries) = self
            .line_row_boundaries
            .get(line)
            .and_then(|boundaries| boundaries.as_ref())
        {
            let max_row = row_boundaries.len().saturating_sub(2);
            let row = row_in_line.min(max_row);
            let start = row_boundaries.get(row).copied().unwrap_or(metrics.chars);
            let end = row_boundaries
                .get(row.saturating_add(1))
                .copied()
                .unwrap_or(metrics.chars)
                .max(start);
            start..end
        } else {
            line_row_char_range(
                buffer,
                line,
                metrics.chars,
                self.wrap_cols.max(1),
                row_in_line,
            )
        };
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

fn measure_line(
    buffer: &RopeBuffer,
    line: usize,
    cols: usize,
) -> (LineWrapMetrics, Option<Box<[usize]>>) {
    let chars = buffer.line_len_chars(line);
    let (columns, ascii_only) = measure_line_columns(buffer, line, chars);
    let visual_rows = measure_line_visual_rows(buffer, line, chars, cols.max(1));
    (
        LineWrapMetrics {
            chars,
            columns,
            visual_rows,
            ascii_only,
        },
        measure_line_row_boundaries(buffer, line, chars, cols.max(1), ascii_only),
    )
}

fn measure_line_row_boundaries(
    buffer: &RopeBuffer,
    line: usize,
    chars: usize,
    cols: usize,
    ascii_only: bool,
) -> Option<Box<[usize]>> {
    if ascii_only {
        return None;
    }
    if chars == 0 {
        return Some(vec![0usize, 0usize].into_boxed_slice());
    }

    let cols = cols.max(1);
    let mut row_starts = Vec::new();
    row_starts.push(0usize);
    let mut row_columns = 0usize;
    let line_slice = buffer.rope().line(line).slice(..chars);
    for (idx, ch) in line_slice.chars().enumerate() {
        let width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if width > 0 && row_columns > 0 && row_columns.saturating_add(width) > cols {
            row_starts.push(idx);
            row_columns = 0;
        }
        if width > 0 {
            row_columns = row_columns.saturating_add(width);
        }
    }
    row_starts.push(chars);
    Some(row_starts.into_boxed_slice())
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
    fn long_line_metrics_keep_full_char_count() {
        let text = format!("{}\n", "a".repeat(10_250));
        let (buffer, cache) = rebuild_cache_for(text.as_str(), 200.0, 10.0, 5.0);

        assert_eq!(cache.line_chars(0), buffer.line_len_chars(0));
    }

    #[test]
    fn long_line_column_mappings_track_full_line() {
        let text = format!("{}\n", "a".repeat(10_500));
        let (buffer, cache) = rebuild_cache_for(text.as_str(), 2000.0, 10.0, 1.0);

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
}
