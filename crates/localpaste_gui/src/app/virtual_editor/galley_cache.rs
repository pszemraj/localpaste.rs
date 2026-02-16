//! Per-line galley cache for the virtual editor render path.

use super::buffer::VirtualEditDelta;
use eframe::egui::{Color32, FontId, Galley};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct VirtualGalleyContext {
    wrap_width_bits: u32,
    highlight_version: u64,
    use_plain: bool,
    font_id: FontId,
    text_color: Color32,
}

impl VirtualGalleyContext {
    pub(crate) fn new(
        wrap_width: f32,
        highlight_version: u64,
        use_plain: bool,
        font_id: &FontId,
        text_color: Color32,
    ) -> Self {
        Self {
            wrap_width_bits: wrap_width.max(0.0).round().to_bits(),
            highlight_version,
            use_plain,
            font_id: font_id.clone(),
            text_color,
        }
    }
}

struct CachedVirtualGalley {
    galley: Arc<Galley>,
}

#[derive(Default)]
pub(crate) struct VirtualGalleyCache {
    entries: Vec<Option<CachedVirtualGalley>>,
    context: Option<VirtualGalleyContext>,
    revision: u64,
}

impl VirtualGalleyCache {
    pub(crate) fn prepare_frame(
        &mut self,
        line_count: usize,
        revision: u64,
        context: VirtualGalleyContext,
    ) {
        let context_changed = self
            .context
            .as_ref()
            .map(|current| current != &context)
            .unwrap_or(true);
        if context_changed || self.revision != revision {
            self.entries.clear();
            self.entries.resize_with(line_count, || None);
            self.context = Some(context);
            self.revision = revision;
            return;
        }

        if self.entries.len() != line_count {
            self.entries.clear();
            self.entries.resize_with(line_count, || None);
        }
        self.context = Some(context);
    }

    pub(crate) fn apply_delta(
        &mut self,
        delta: VirtualEditDelta,
        line_count: usize,
        revision: u64,
    ) {
        if !apply_delta_to_entries(&mut self.entries, delta, line_count) {
            self.entries.clear();
            self.entries.resize_with(line_count, || None);
        }
        self.revision = revision;
    }

    pub(crate) fn galley_for_line<F>(&mut self, line_idx: usize, build: F) -> Arc<Galley>
    where
        F: FnOnce() -> Arc<Galley>,
    {
        if let Some(Some(entry)) = self.entries.get(line_idx) {
            return entry.galley.clone();
        }

        let galley = build();
        if let Some(slot) = self.entries.get_mut(line_idx) {
            *slot = Some(CachedVirtualGalley {
                galley: galley.clone(),
            });
        }
        galley
    }
}

fn apply_delta_to_entries<T>(
    entries: &mut Vec<Option<T>>,
    delta: VirtualEditDelta,
    new_len: usize,
) -> bool {
    if entries.is_empty() {
        entries.resize_with(new_len, || None);
        return true;
    }

    let old_len = entries.len();
    let old_start = delta.start_line;
    let old_end_exclusive = delta.old_end_line.saturating_add(1);
    if old_start >= old_len || old_end_exclusive > old_len {
        return false;
    }
    if delta.new_end_line >= new_len {
        return false;
    }

    let new_count = delta
        .new_end_line
        .saturating_sub(delta.start_line)
        .saturating_add(1);
    let old_count = old_end_exclusive.saturating_sub(old_start);
    let expected_len = old_len.saturating_sub(old_count).saturating_add(new_count);
    if expected_len != new_len {
        return false;
    }

    let mut replacement: Vec<Option<T>> = Vec::with_capacity(new_count);
    replacement.resize_with(new_count, || None);
    entries.splice(old_start..old_end_exclusive, replacement);
    entries.len() == new_len
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker_vec(values: &[Option<u32>]) -> Vec<Option<u32>> {
        values.to_vec()
    }

    #[test]
    fn apply_delta_to_entries_handles_insert_delete_and_reject_cases() {
        struct Case {
            before: Vec<Option<u32>>,
            delta: VirtualEditDelta,
            new_len: usize,
            expected_ok: bool,
            after: Vec<Option<u32>>,
        }

        let cases = vec![
            Case {
                before: marker_vec(&[Some(10), Some(20), Some(30), Some(40)]),
                delta: VirtualEditDelta {
                    start_line: 1,
                    old_end_line: 1,
                    new_end_line: 2,
                    char_delta: 5,
                },
                new_len: 5,
                expected_ok: true,
                after: marker_vec(&[Some(10), None, None, Some(30), Some(40)]),
            },
            Case {
                before: marker_vec(&[Some(10), Some(20), Some(30), Some(40), Some(50)]),
                delta: VirtualEditDelta {
                    start_line: 1,
                    old_end_line: 3,
                    new_end_line: 1,
                    char_delta: -8,
                },
                new_len: 3,
                expected_ok: true,
                after: marker_vec(&[Some(10), None, Some(50)]),
            },
            Case {
                before: marker_vec(&[Some(1), Some(2)]),
                delta: VirtualEditDelta {
                    start_line: 5,
                    old_end_line: 5,
                    new_end_line: 5,
                    char_delta: 0,
                },
                new_len: 2,
                expected_ok: false,
                after: marker_vec(&[Some(1), Some(2)]),
            },
        ];

        for case in cases {
            let mut entries = case.before.clone();
            let ok = apply_delta_to_entries(&mut entries, case.delta, case.new_len);
            assert_eq!(ok, case.expected_ok);
            assert_eq!(entries, case.after);
        }
    }
}
