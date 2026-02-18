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
    ///
    /// # Arguments
    /// - `revision`: Expected rope revision.
    /// - `wrap_width`: Current wrap width in points.
    /// - `line_height`: Current line height in points.
    /// - `char_width`: Current monospace character width in points.
    /// - `line_count`: Current physical line count in the buffer.
    ///
    /// # Returns
    /// `true` when cached metrics are stale and should be rebuilt.
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
    ///
    /// # Arguments
    /// - `buffer`: Source text buffer.
    /// - `wrap_width`: Viewport wrap width in points.
    /// - `line_height`: Effective row height in points.
    /// - `char_width`: Monospace character width in points.
    ///
    /// # Panics
    /// Panics only if internal line iteration and metric vectors become inconsistent.
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
    ///
    /// # Arguments
    /// - `buffer`: Buffer after applying the text mutation.
    /// - `delta`: Line-aligned edit delta describing the mutation window.
    ///
    /// # Returns
    /// `true` when incremental patching succeeded, `false` when caller should rebuild.
    ///
    /// # Panics
    /// Panics only if `delta` validation and generated replacement metrics diverge.
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
    ///
    /// # Returns
    /// `true` when cached geometry was valid enough to rebuild, otherwise `false`.
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
    ///
    /// # Returns
    /// Effective wrap columns, clamped to at least `1`.
    pub(crate) fn wrap_columns(&self) -> usize {
        self.wrap_cols.max(1)
    }

    /// Total visual row count.
    ///
    /// # Returns
    /// Sum of wrapped rows across all physical lines.
    pub(crate) fn total_rows(&self) -> usize {
        self.row_index.total_rows()
    }

    /// Cached character length for a line.
    ///
    /// # Returns
    /// Cached visible char count for `line`, or `0` when out of bounds.
    pub(crate) fn line_chars(&self, line: usize) -> usize {
        self.line_metrics.get(line).map(|m| m.chars).unwrap_or(0)
    }

    /// Cached display column width for a line.
    ///
    /// # Arguments
    /// - `buffer`: Source buffer used for fallback measurement when cache is missing.
    /// - `line`: Physical line index.
    ///
    /// # Returns
    /// Display-column width for `line` under unicode width rules.
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
    ///
    /// # Arguments
    /// - `buffer`: Source buffer for per-character width measurement.
    /// - `line`: Physical line index.
    /// - `char_column`: Character offset within `line`.
    ///
    /// # Returns
    /// Display-column position corresponding to `char_column`.
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
    ///
    /// # Arguments
    /// - `buffer`: Source buffer for per-character width measurement.
    /// - `line`: Physical line index.
    /// - `target_columns`: Desired display-column offset within the line.
    ///
    /// # Returns
    /// Character offset that best matches `target_columns`.
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
    ///
    /// # Returns
    /// Number of wrapped rows occupied by `line`, clamped to at least `1`.
    pub(crate) fn line_visual_rows(&self, line: usize) -> usize {
        self.line_metrics
            .get(line)
            .map(|m| m.visual_rows.max(1))
            .unwrap_or(1)
    }

    /// Start visual row for a physical line.
    ///
    /// # Returns
    /// Global visual-row index where `line` begins.
    #[cfg(test)]
    pub(crate) fn line_start_row(&self, line: usize) -> usize {
        if line > self.line_metrics.len() {
            return self.total_rows();
        }
        self.row_index.prefix_sum_exclusive(line)
    }

    /// Map visual row to (physical line, row in that line).
    ///
    /// # Returns
    /// A tuple of `(line_index, row_within_line)`.
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
    ///
    /// # Arguments
    /// - `buffer`: Source buffer used for line/char coordinate conversion.
    /// - `row`: Global visual-row index.
    ///
    /// # Returns
    /// Global char range covered by the requested wrapped row.
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
///
/// # Arguments
/// - `vec`: Existing line-aligned vector to patch in place.
/// - `delta`: Line-aligned mutation window from the text edit.
/// - `new_len`: Expected vector length after patching.
/// - `make_new`: Factory called once for each inserted replacement slot.
///
/// # Returns
/// `true` when splice validation succeeded and vector length matches `new_len`.
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
#[path = "visual_rows_tests.rs"]
mod tests;
