//! Per-line/per-visual-row galley cache for the virtual editor render path.

use super::buffer::VirtualEditDelta;
use super::visual_rows::splice_vec_by_delta;
use eframe::egui::{Color32, FontId, Galley};
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VirtualGalleyContext {
    wrap_width_bits: u32,
    // Highlight invalidation is driven explicitly from highlight_flow
    // via evict_line_range/evict_all; keep context geometry/style-only.
    use_plain: bool,
    pixels_per_point_bits: u32,
    font_id: FontId,
    text_color: Color32,
}

impl VirtualGalleyContext {
    pub(crate) fn new(
        wrap_width: f32,
        use_plain: bool,
        font_id: &FontId,
        text_color: Color32,
        pixels_per_point: f32,
    ) -> Self {
        Self {
            wrap_width_bits: wrap_width.max(0.0).round().to_bits(),
            use_plain,
            pixels_per_point_bits: pixels_per_point.to_bits(),
            font_id: font_id.clone(),
            text_color,
        }
    }
}

#[derive(Default)]
struct LineGalleyCache {
    rows: Vec<Option<Arc<Galley>>>,
}

#[derive(Default)]
pub(crate) struct VirtualGalleyCache {
    lines: Vec<LineGalleyCache>,
    context: Option<VirtualGalleyContext>,
}

impl VirtualGalleyCache {
    pub(crate) fn prepare_frame(&mut self, line_count: usize, context: VirtualGalleyContext) {
        let context_changed = self
            .context
            .as_ref()
            .map(|current| current != &context)
            .unwrap_or(true);
        if context_changed || self.lines.len() != line_count {
            self.lines.clear();
            self.lines.resize_with(line_count, LineGalleyCache::default);
        }
        self.context = Some(context);
    }

    pub(crate) fn apply_delta(&mut self, delta: VirtualEditDelta, line_count: usize) {
        if self.lines.is_empty() {
            self.lines.resize_with(line_count, LineGalleyCache::default);
            return;
        }
        if !splice_vec_by_delta(&mut self.lines, delta, line_count, LineGalleyCache::default) {
            self.lines.clear();
            self.lines.resize_with(line_count, LineGalleyCache::default);
        }
    }

    pub(crate) fn sync_line_rows(&mut self, line_idx: usize, visual_rows: usize) {
        let Some(line) = self.lines.get_mut(line_idx) else {
            return;
        };
        let visual_rows = visual_rows.max(1);
        if line.rows.len() != visual_rows {
            line.rows.clear();
            line.rows.resize_with(visual_rows, || None);
        }
    }

    pub(crate) fn get(&self, line_idx: usize, row_in_line: usize) -> Option<Arc<Galley>> {
        self.lines
            .get(line_idx)
            .and_then(|line| line.rows.get(row_in_line))
            .and_then(|entry| entry.clone())
    }

    pub(crate) fn insert(&mut self, line_idx: usize, row_in_line: usize, galley: Arc<Galley>) {
        let Some(line) = self.lines.get_mut(line_idx) else {
            return;
        };
        if let Some(slot) = line.rows.get_mut(row_in_line) {
            *slot = Some(galley);
        }
    }

    pub(crate) fn evict_all(&mut self) {
        for line in &mut self.lines {
            for row in &mut line.rows {
                *row = None;
            }
        }
    }

    pub(crate) fn evict_line_range(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let start = range.start.min(self.lines.len());
        let end = range.end.min(self.lines.len());
        for idx in start..end {
            if let Some(line) = self.lines.get_mut(idx) {
                for row in &mut line.rows {
                    *row = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shaped_test_galley() -> Arc<Galley> {
        let mut galley = None;
        eframe::egui::__run_test_ctx(|ctx| {
            galley = Some(ctx.fonts_mut(|fonts| {
                fonts.layout_no_wrap("x".to_owned(), FontId::monospace(14.0), Color32::LIGHT_GRAY)
            }));
        });
        galley.expect("test galley")
    }

    #[test]
    fn apply_delta_splices_line_cache_without_touching_suffix_prefix() {
        let mut cache = VirtualGalleyCache {
            lines: vec![
                LineGalleyCache::default(),
                LineGalleyCache::default(),
                LineGalleyCache::default(),
                LineGalleyCache::default(),
            ],
            context: None,
        };
        let delta = VirtualEditDelta {
            start_line: 1,
            old_end_line: 2,
            new_end_line: 3,
            char_delta: 0,
        };
        cache.apply_delta(delta, 5);
        assert_eq!(cache.lines.len(), 5);
    }

    #[test]
    fn sync_line_rows_resets_row_cache_when_row_count_changes() {
        let mut cache = VirtualGalleyCache::default();
        cache.prepare_frame(
            2,
            VirtualGalleyContext::new(300.0, false, &FontId::monospace(14.0), Color32::WHITE, 1.0),
        );
        cache.sync_line_rows(0, 2);
        assert_eq!(cache.lines[0].rows.len(), 2);
        cache.sync_line_rows(0, 1);
        assert_eq!(cache.lines[0].rows.len(), 1);
    }

    #[test]
    fn evict_line_range_only_clears_targeted_lines() {
        let mut cache = VirtualGalleyCache::default();
        cache.prepare_frame(
            3,
            VirtualGalleyContext::new(300.0, false, &FontId::monospace(14.0), Color32::WHITE, 1.0),
        );
        for idx in 0..3 {
            cache.sync_line_rows(idx, 1);
            cache.insert(idx, 0, shaped_test_galley());
        }

        cache.evict_line_range(1..2);
        assert!(cache.get(0, 0).is_some());
        assert!(cache.get(1, 0).is_none());
        assert!(cache.get(2, 0).is_some());
    }

    #[test]
    fn prepare_frame_invalidates_cached_rows_when_pixels_per_point_changes() {
        let mut cache = VirtualGalleyCache::default();
        cache.prepare_frame(
            1,
            VirtualGalleyContext::new(300.0, false, &FontId::monospace(14.0), Color32::WHITE, 1.0),
        );
        cache.sync_line_rows(0, 1);
        cache.insert(0, 0, shaped_test_galley());
        assert!(cache.get(0, 0).is_some());

        cache.prepare_frame(
            1,
            VirtualGalleyContext::new(300.0, false, &FontId::monospace(14.0), Color32::WHITE, 1.25),
        );
        cache.sync_line_rows(0, 1);
        assert!(cache.get(0, 0).is_none());
    }
}
