//! Undo/redo history for the virtual editor.

use super::buffer::{RopeBuffer, VirtualEditDelta};
use super::state::VirtualEditorState;
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

/// Operation-based undo/redo stack with bounded memory.
#[derive(Debug)]
pub(crate) struct VirtualEditorHistory {
    undo: Vec<EditRecord>,
    redo: Vec<EditRecord>,
    undo_bytes: usize,
    max_ops: usize,
    max_bytes: usize,
    coalesce_window: Duration,
}

impl Default for VirtualEditorHistory {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            undo_bytes: 0,
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
        self.redo.clear();
        let incoming = EditRecord {
            start: edit.start,
            deleted: edit.deleted,
            inserted: edit.inserted,
            intent: edit.intent,
            before_cursor: edit.before_cursor,
            after_cursor: edit.after_cursor,
            at: edit.at,
        };
        if let Some(last) = self.undo.last_mut() {
            if Self::can_coalesce(last, &incoming, self.coalesce_window) {
                self.undo_bytes = self.undo_bytes.saturating_sub(op_bytes(last));
                last.inserted.push_str(incoming.inserted.as_str());
                last.after_cursor = incoming.after_cursor;
                last.at = incoming.at;
                self.undo_bytes = self.undo_bytes.saturating_add(op_bytes(last));
                self.trim_undo();
                return;
            }
        }
        self.undo_bytes = self.undo_bytes.saturating_add(op_bytes(&incoming));
        self.undo.push(incoming);
        self.trim_undo();
    }

    fn can_coalesce(previous: &EditRecord, next: &EditRecord, window: Duration) -> bool {
        if previous.intent != next.intent || next.at.saturating_duration_since(previous.at) > window
        {
            return false;
        }
        if previous.intent != EditIntent::Insert {
            return false;
        }
        if !previous.deleted.is_empty() || !next.deleted.is_empty() {
            return false;
        }
        let prev_inserted_chars = previous.inserted.chars().count();
        next.start == previous.start.saturating_add(prev_inserted_chars)
    }

    fn trim_undo(&mut self) {
        while self.undo.len() > self.max_ops || self.undo_bytes > self.max_bytes {
            if self.undo.is_empty() {
                break;
            }
            let removed = self.undo.remove(0);
            self.undo_bytes = self.undo_bytes.saturating_sub(op_bytes(&removed));
        }
    }

    /// Undo the most recent mutation.
    pub(crate) fn undo(
        &mut self,
        buffer: &mut RopeBuffer,
        state: &mut VirtualEditorState,
    ) -> Option<VirtualEditDelta> {
        let op = self.undo.pop()?;
        self.undo_bytes = self.undo_bytes.saturating_sub(op_bytes(&op));
        let inserted_chars = op.inserted.chars().count();
        let end = op.start.saturating_add(inserted_chars);
        let delta = buffer.replace_char_range(op.start..end, op.deleted.as_str());
        state.set_cursor(op.before_cursor, buffer.len_chars());
        self.redo.push(op);
        delta
    }

    /// Redo the next mutation, if available.
    pub(crate) fn redo(
        &mut self,
        buffer: &mut RopeBuffer,
        state: &mut VirtualEditorState,
    ) -> Option<VirtualEditDelta> {
        let op = self.redo.pop()?;
        let deleted_chars = op.deleted.chars().count();
        let end = op.start.saturating_add(deleted_chars);
        let delta = buffer.replace_char_range(op.start..end, op.inserted.as_str());
        state.set_cursor(op.after_cursor, buffer.len_chars());
        self.undo_bytes = self.undo_bytes.saturating_add(op_bytes(&op));
        self.undo.push(op);
        self.trim_undo();
        delta
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
    }
}
