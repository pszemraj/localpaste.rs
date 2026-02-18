//! Undo/redo history for the virtual editor.

use super::buffer::{RopeBuffer, VirtualEditDelta};
use super::state::VirtualEditorState;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const DEFAULT_MAX_OPS: usize = 500;
const DEFAULT_MAX_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_COALESCE_WINDOW: Duration = Duration::from_millis(750);

/// Mutation intent used for history coalescing rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditIntent {
    Insert,
    DeleteBackward,
    DeleteForward,
    Paste,
    Cut,
    ImeCommit,
    Other,
}

#[derive(Clone, Debug)]
struct EditRecord {
    start: usize,
    deleted: String,
    inserted: String,
    intent: EditIntent,
    before_cursor: usize,
    after_cursor: usize,
    at: Instant,
}

/// Captured mutation metadata to persist into undo history.
#[derive(Clone, Debug)]
pub(crate) struct RecordedEdit {
    pub(crate) start: usize,
    pub(crate) deleted: String,
    pub(crate) inserted: String,
    pub(crate) intent: EditIntent,
    pub(crate) before_cursor: usize,
    pub(crate) after_cursor: usize,
    pub(crate) at: Instant,
}

fn op_bytes(op: &EditRecord) -> usize {
    op.deleted.len().saturating_add(op.inserted.len())
}

/// Snapshot of undo/redo history counters for local perf logging.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HistoryPerfStats {
    pub(crate) undo_len: usize,
    pub(crate) redo_len: usize,
    pub(crate) undo_bytes: usize,
    pub(crate) redo_invalidations: u64,
    pub(crate) coalesced_edits: u64,
    pub(crate) trim_evictions: u64,
    pub(crate) redo_hits: u64,
    pub(crate) redo_misses: u64,
}

/// Operation-based undo/redo stack with bounded memory.
#[derive(Debug)]
pub(crate) struct VirtualEditorHistory {
    undo: VecDeque<EditRecord>,
    redo: Vec<EditRecord>,
    undo_bytes: usize,
    redo_invalidations: u64,
    coalesced_edits: u64,
    trim_evictions: u64,
    redo_hits: u64,
    redo_misses: u64,
    max_ops: usize,
    max_bytes: usize,
    coalesce_window: Duration,
}

impl Default for VirtualEditorHistory {
    fn default() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            undo_bytes: 0,
            redo_invalidations: 0,
            coalesced_edits: 0,
            trim_evictions: 0,
            redo_hits: 0,
            redo_misses: 0,
            max_ops: DEFAULT_MAX_OPS,
            max_bytes: DEFAULT_MAX_BYTES,
            coalesce_window: DEFAULT_COALESCE_WINDOW,
        }
    }
}

impl VirtualEditorHistory {
    /// Record a text mutation in undo history.
    pub(crate) fn record_edit(&mut self, edit: RecordedEdit) {
        if edit.deleted.is_empty() && edit.inserted.is_empty() {
            return;
        }
        if !self.redo.is_empty() {
            self.redo_invalidations = self
                .redo_invalidations
                .saturating_add(self.redo.len() as u64);
            self.redo.clear();
        }
        let incoming = EditRecord {
            start: edit.start,
            deleted: edit.deleted,
            inserted: edit.inserted,
            intent: edit.intent,
            before_cursor: edit.before_cursor,
            after_cursor: edit.after_cursor,
            at: edit.at,
        };
        if let Some(last) = self.undo.back_mut() {
            if Self::can_coalesce(last, &incoming, self.coalesce_window) {
                self.coalesced_edits = self.coalesced_edits.saturating_add(1);
                self.undo_bytes = self.undo_bytes.saturating_sub(op_bytes(last));
                Self::coalesce_into(last, incoming);
                self.undo_bytes = self.undo_bytes.saturating_add(op_bytes(last));
                self.trim_undo();
                return;
            }
        }
        self.undo_bytes = self.undo_bytes.saturating_add(op_bytes(&incoming));
        self.undo.push_back(incoming);
        self.trim_undo();
    }

    fn can_coalesce(previous: &EditRecord, next: &EditRecord, window: Duration) -> bool {
        if previous.intent != next.intent || next.at.saturating_duration_since(previous.at) > window
        {
            return false;
        }
        match previous.intent {
            EditIntent::Insert => {
                if !previous.deleted.is_empty() || !next.deleted.is_empty() {
                    return false;
                }
                next.start == previous.after_cursor
            }
            EditIntent::DeleteBackward => {
                if !previous.inserted.is_empty() || !next.inserted.is_empty() {
                    return false;
                }
                next.start.saturating_add(next.deleted.chars().count()) == previous.start
            }
            EditIntent::DeleteForward => {
                if !previous.inserted.is_empty() || !next.inserted.is_empty() {
                    return false;
                }
                next.start == previous.start
            }
            _ => false,
        }
    }

    fn coalesce_into(previous: &mut EditRecord, next: EditRecord) {
        match previous.intent {
            EditIntent::Insert => {
                previous.inserted.push_str(next.inserted.as_str());
                previous.after_cursor = next.after_cursor;
                previous.at = next.at;
            }
            EditIntent::DeleteBackward => {
                let mut merged = String::with_capacity(next.deleted.len() + previous.deleted.len());
                merged.push_str(next.deleted.as_str());
                merged.push_str(previous.deleted.as_str());
                previous.start = next.start;
                previous.deleted = merged;
                previous.after_cursor = next.after_cursor;
                previous.at = next.at;
            }
            EditIntent::DeleteForward => {
                previous.deleted.push_str(next.deleted.as_str());
                previous.after_cursor = next.after_cursor;
                previous.at = next.at;
            }
            _ => {}
        }
    }

    fn trim_undo(&mut self) {
        while self.undo.len() > self.max_ops || self.undo_bytes > self.max_bytes {
            let Some(removed) = self.undo.pop_front() else {
                break;
            };
            self.undo_bytes = self.undo_bytes.saturating_sub(op_bytes(&removed));
            self.trim_evictions = self.trim_evictions.saturating_add(1);
        }
    }

    /// Undo the most recent mutation.
    ///
    /// # Arguments
    /// - `buffer`: Mutable text buffer to revert.
    /// - `state`: Editor interaction state to restore cursor/selection position.
    ///
    /// # Returns
    /// The applied edit delta, or `None` when undo history is empty.
    pub(crate) fn undo(
        &mut self,
        buffer: &mut RopeBuffer,
        state: &mut VirtualEditorState,
    ) -> Option<VirtualEditDelta> {
        let op = self.undo.pop_back()?;
        self.undo_bytes = self.undo_bytes.saturating_sub(op_bytes(&op));
        let inserted_chars = op.inserted.chars().count();
        let end = op.start.saturating_add(inserted_chars);
        let delta = buffer.replace_char_range(op.start..end, op.deleted.as_str());
        state.set_cursor(op.before_cursor, buffer.len_chars());
        self.redo.push(op);
        delta
    }

    /// Redo the next mutation, if available.
    ///
    /// # Arguments
    /// - `buffer`: Mutable text buffer to reapply into.
    /// - `state`: Editor interaction state to move to the post-edit cursor.
    ///
    /// # Returns
    /// The reapplied edit delta, or `None` when redo history is empty.
    pub(crate) fn redo(
        &mut self,
        buffer: &mut RopeBuffer,
        state: &mut VirtualEditorState,
    ) -> Option<VirtualEditDelta> {
        let Some(op) = self.redo.pop() else {
            self.redo_misses = self.redo_misses.saturating_add(1);
            return None;
        };
        self.redo_hits = self.redo_hits.saturating_add(1);
        let deleted_chars = op.deleted.chars().count();
        let end = op.start.saturating_add(deleted_chars);
        let delta = buffer.replace_char_range(op.start..end, op.inserted.as_str());
        state.set_cursor(op.after_cursor, buffer.len_chars());
        self.undo_bytes = self.undo_bytes.saturating_add(op_bytes(&op));
        self.undo.push_back(op);
        self.trim_undo();
        delta
    }

    /// Return a point-in-time snapshot of history counters.
    ///
    /// # Returns
    /// Current queue lengths and counters used by perf logging.
    pub(crate) fn perf_stats(&self) -> HistoryPerfStats {
        HistoryPerfStats {
            undo_len: self.undo.len(),
            redo_len: self.redo.len(),
            undo_bytes: self.undo_bytes,
            redo_invalidations: self.redo_invalidations,
            coalesced_edits: self.coalesced_edits,
            trim_evictions: self.trim_evictions,
            redo_hits: self.redo_hits,
            redo_misses: self.redo_misses,
        }
    }

    #[cfg(test)]
    fn undo_len(&self) -> usize {
        self.undo.len()
    }

    #[cfg(test)]
    fn redo_len(&self) -> usize {
        self.redo.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_contiguous_delete_coalesces(
        intent: EditIntent,
        edits: [(usize, &str, usize, usize); 2],
    ) {
        let mut buffer = RopeBuffer::new("abcde");
        let mut state = VirtualEditorState::default();
        let mut history = VirtualEditorHistory::default();
        let now = Instant::now();

        for (index, (start, deleted, before_cursor, after_cursor)) in edits.into_iter().enumerate()
        {
            let end = start.saturating_add(deleted.chars().count());
            let _ = buffer.replace_char_range(start..end, "");
            history.record_edit(RecordedEdit {
                start,
                deleted: deleted.to_string(),
                inserted: String::new(),
                intent,
                before_cursor,
                after_cursor,
                at: now + Duration::from_millis((index as u64) * 10),
            });
        }

        assert_eq!(history.undo_len(), 1);
        assert!(history.undo(&mut buffer, &mut state).is_some());
        assert_eq!(buffer.to_string(), "abcde");
    }

    #[test]
    fn coalesces_adjacent_typing() {
        let mut history = VirtualEditorHistory::default();
        let now = Instant::now();
        history.record_edit(RecordedEdit {
            start: 0,
            deleted: String::new(),
            inserted: "h".to_string(),
            intent: EditIntent::Insert,
            before_cursor: 0,
            after_cursor: 1,
            at: now,
        });
        history.record_edit(RecordedEdit {
            start: 1,
            deleted: String::new(),
            inserted: "i".to_string(),
            intent: EditIntent::Insert,
            before_cursor: 1,
            after_cursor: 2,
            at: now + Duration::from_millis(10),
        });
        assert_eq!(history.undo_len(), 1);
    }

    #[test]
    fn undo_and_redo_roundtrip() {
        let mut buffer = RopeBuffer::new("abc");
        let mut state = VirtualEditorState::default();
        let mut history = VirtualEditorHistory::default();
        let now = Instant::now();
        let _ = buffer.replace_char_range(1..2, "XYZ");
        history.record_edit(RecordedEdit {
            start: 1,
            deleted: "b".to_string(),
            inserted: "XYZ".to_string(),
            intent: EditIntent::Other,
            before_cursor: 1,
            after_cursor: 4,
            at: now,
        });

        assert!(history.undo(&mut buffer, &mut state).is_some());
        assert_eq!(buffer.to_string(), "abc");
        assert!(history.redo(&mut buffer, &mut state).is_some());
        assert_eq!(buffer.to_string(), "aXYZc");
        assert_eq!(history.redo_len(), 0);
        let perf = history.perf_stats();
        assert_eq!(perf.redo_hits, 1);
        assert_eq!(perf.redo_misses, 0);
    }

    #[test]
    fn redo_cache_invalidation_and_miss_are_counted() {
        let mut buffer = RopeBuffer::new("ab");
        let mut state = VirtualEditorState::default();
        let mut history = VirtualEditorHistory::default();
        let now = Instant::now();

        let _ = buffer.replace_char_range(1..2, "X");
        history.record_edit(RecordedEdit {
            start: 1,
            deleted: "b".to_string(),
            inserted: "X".to_string(),
            intent: EditIntent::Other,
            before_cursor: 1,
            after_cursor: 2,
            at: now,
        });
        assert!(history.undo(&mut buffer, &mut state).is_some());

        history.record_edit(RecordedEdit {
            start: 1,
            deleted: String::new(),
            inserted: "y".to_string(),
            intent: EditIntent::Insert,
            before_cursor: 1,
            after_cursor: 2,
            at: now + Duration::from_millis(5),
        });

        let perf = history.perf_stats();
        assert_eq!(perf.redo_invalidations, 1);
        assert!(history.redo(&mut buffer, &mut state).is_none());
        assert_eq!(history.perf_stats().redo_misses, 1);
    }

    #[test]
    fn coalesces_contiguous_deletes() {
        let cases = [
            (EditIntent::DeleteBackward, [(4, "e", 5, 4), (3, "d", 4, 3)]),
            (EditIntent::DeleteForward, [(2, "c", 2, 2), (2, "d", 2, 2)]),
        ];
        for (intent, edits) in cases {
            assert_contiguous_delete_coalesces(intent, edits);
        }
    }

    #[test]
    fn does_not_coalesce_non_contiguous_backspace_deletes() {
        let mut history = VirtualEditorHistory::default();
        let now = Instant::now();

        history.record_edit(RecordedEdit {
            start: 4,
            deleted: "x".to_string(),
            inserted: String::new(),
            intent: EditIntent::DeleteBackward,
            before_cursor: 5,
            after_cursor: 4,
            at: now,
        });
        history.record_edit(RecordedEdit {
            start: 1,
            deleted: "y".to_string(),
            inserted: String::new(),
            intent: EditIntent::DeleteBackward,
            before_cursor: 2,
            after_cursor: 1,
            at: now + Duration::from_millis(10),
        });

        assert_eq!(history.undo_len(), 2);
    }

    #[test]
    fn trim_evicts_oldest_undo_operations() {
        let mut buffer = RopeBuffer::new("");
        let mut state = VirtualEditorState::default();
        let mut history = VirtualEditorHistory::default();
        let now = Instant::now();

        for index in 0..=500 {
            let inserted = if index == 0 { "A" } else { "b" };
            let start = buffer.len_chars();
            let _ = buffer.replace_char_range(start..start, inserted);
            history.record_edit(RecordedEdit {
                start,
                deleted: String::new(),
                inserted: inserted.to_string(),
                intent: EditIntent::Other,
                before_cursor: start,
                after_cursor: start + 1,
                at: now + Duration::from_millis(index as u64),
            });
        }

        assert_eq!(history.undo_len(), 500);
        assert_eq!(history.perf_stats().trim_evictions, 1);

        for _ in 0..500 {
            assert!(history.undo(&mut buffer, &mut state).is_some());
        }
        assert_eq!(buffer.to_string(), "A");
        assert!(history.undo(&mut buffer, &mut state).is_none());
    }
}
