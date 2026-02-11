//! Wrap and viewport metrics for virtualized rendering.

use super::buffer::RopeBuffer;
use std::ops::Range;

fn div_ceil(value: usize, divisor: usize) -> usize {
    if value == 0 {
        0
    } else {
        (value - 1) / divisor + 1
    }
}

/// Wrap metrics for a single physical line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WrapLineMetrics {
    /// Number of visual rows occupied by this physical line.
    pub(crate) visual_rows: usize,
    /// Total painted height for this physical line.
    pub(crate) height_px: f32,
    /// Character count of the source line (without newline).
    pub(crate) chars: usize,
}

impl Default for WrapLineMetrics {
    fn default() -> Self {
        Self {
            visual_rows: 1,
            height_px: 0.0,
            chars: 0,
        }
    }
}

/// Cache of variable-height line metrics for viewport virtualization.
#[derive(Clone, Debug, Default)]
pub(crate) struct WrapLayoutCache {
    revision: u64,
    wrap_width: u32,
    line_height_bits: u32,
    char_width_bits: u32,
    highlight_epoch: u64,
    wrap_cols: usize,
    line_metrics: Vec<WrapLineMetrics>,
    prefix_heights: Vec<f32>,
}

impl WrapLayoutCache {
    /// Returns `true` when cache keys no longer match rendering context.
    pub(crate) fn needs_rebuild(
        &self,
        revision: u64,
        wrap_width: f32,
        line_height: f32,
        char_width: f32,
        highlight_epoch: u64,
        line_count: usize,
    ) -> bool {
        self.revision != revision
            || self.wrap_width != wrap_width.max(0.0).round() as u32
            || self.line_height_bits != line_height.to_bits()
            || self.char_width_bits != char_width.to_bits()
            || self.highlight_epoch != highlight_epoch
            || self.line_metrics.len() != line_count
    }

    /// Rebuild metrics for all physical lines.
    pub(crate) fn rebuild(
        &mut self,
        buffer: &RopeBuffer,
        wrap_width: f32,
        line_height: f32,
        char_width: f32,
        highlight_epoch: u64,
    ) {
        let wrap_width = wrap_width.max(0.0).round() as u32;
        self.revision = buffer.revision();
        self.wrap_width = wrap_width;
        self.line_height_bits = line_height.to_bits();
        self.char_width_bits = char_width.to_bits();
        self.highlight_epoch = highlight_epoch;
        self.line_metrics.clear();
        self.prefix_heights.clear();
        self.prefix_heights.push(0.0);

        let cols = ((wrap_width as f32 / char_width.max(1.0)).floor() as usize).max(1);
        self.wrap_cols = cols;
        let mut total = 0.0f32;
        for idx in 0..buffer.line_count() {
            let chars = buffer.line_len_chars(idx);
            let visual_rows = div_ceil(chars.max(1), cols).max(1);
            let height_px = visual_rows as f32 * line_height.max(1.0);
            self.line_metrics.push(WrapLineMetrics {
                visual_rows,
                height_px,
                chars,
            });
            total += height_px;
            self.prefix_heights.push(total);
        }
    }

    /// Total scrollable content height in pixels.
    pub(crate) fn total_height(&self) -> f32 {
        self.prefix_heights.last().copied().unwrap_or(0.0)
    }

    /// Top y-offset for a physical line.
    pub(crate) fn line_top(&self, line: usize) -> f32 {
        self.prefix_heights
            .get(line)
            .copied()
            .unwrap_or_else(|| self.total_height())
    }

    /// Bottom y-offset for a physical line.
    pub(crate) fn line_bottom(&self, line: usize) -> f32 {
        self.prefix_heights
            .get(line + 1)
            .copied()
            .unwrap_or_else(|| self.total_height())
    }

    /// Returns metrics for a line index.
    pub(crate) fn line_metrics(&self, line: usize) -> Option<WrapLineMetrics> {
        self.line_metrics.get(line).copied()
    }

    /// Number of monospace columns used by the current wrap configuration.
    pub(crate) fn wrap_columns(&self) -> usize {
        self.wrap_cols.max(1)
    }

    fn line_at_y(&self, y: f32) -> usize {
        if self.line_metrics.is_empty() {
            return 0;
        }
        let y = y.clamp(0.0, self.total_height());
        let mut lo = 0usize;
        let mut hi = self.line_metrics.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            let next_top = self.prefix_heights[mid + 1];
            if next_top <= y {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo.min(self.line_metrics.len().saturating_sub(1))
    }

    /// Returns visible line range with overscan in physical line units.
    pub(crate) fn visible_range(
        &self,
        viewport_top: f32,
        viewport_height: f32,
        overscan_lines: usize,
    ) -> Range<usize> {
        if self.line_metrics.is_empty() {
            return 0..0;
        }
        let start = self
            .line_at_y(viewport_top)
            .saturating_sub(overscan_lines)
            .min(self.line_metrics.len());
        let mut end = self
            .line_at_y(viewport_top + viewport_height.max(0.0))
            .saturating_add(1)
            .saturating_add(overscan_lines)
            .min(self.line_metrics.len());
        if end < start {
            end = start;
        }
        start..end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_range_tracks_wrapped_heights() {
        let buffer = RopeBuffer::new("1234567890\n12\n123456");
        let mut cache = WrapLayoutCache::default();
        cache.rebuild(&buffer, 30.0, 10.0, 5.0, 0);
        // Wrap columns = 6, heights: [20,10,10]
        assert_eq!(cache.visible_range(0.0, 9.0, 0), 0..1);
        assert_eq!(cache.visible_range(21.0, 9.0, 0), 1..3);
        assert_eq!(cache.visible_range(35.0, 20.0, 1), 1..3);
    }
}
