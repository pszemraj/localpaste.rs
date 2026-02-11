//! Rope-backed text storage for the virtual editor.

use ropey::Rope;
use std::ops::Range;

/// Delta summary for a virtual editor text mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VirtualEditDelta {
    /// Line where the mutation started in the pre-edit buffer.
    pub(crate) start_line: usize,
    /// Last impacted line index in the pre-edit buffer.
    pub(crate) old_end_line: usize,
    /// Last impacted line index in the post-edit buffer.
    pub(crate) new_end_line: usize,
    /// Character delta (`new_chars - old_chars`) from the mutation.
    pub(crate) char_delta: isize,
}

fn line_for_char(rope: &Rope, char_index: usize) -> usize {
    rope.char_to_line(char_index.min(rope.len_chars()))
}

/// Rope-backed content buffer used by the virtualized editor path.
#[derive(Clone, Default)]
pub(crate) struct RopeBuffer {
    rope: Rope,
    revision: u64,
    char_len: usize,
}

impl RopeBuffer {
    /// Create a new buffer from UTF-8 text.
    pub(crate) fn new(text: &str) -> Self {
        let rope = Rope::from_str(text);
        let char_len = rope.len_chars();
        Self {
            rope,
            revision: 0,
            char_len,
        }
    }

    /// Returns a borrowed rope handle.
    pub(crate) fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Returns the current revision of the buffer.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the content length in characters.
    pub(crate) fn len_chars(&self) -> usize {
        self.char_len
    }

    /// Returns the content length in bytes.
    pub(crate) fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Returns the number of physical lines in the rope.
    pub(crate) fn line_count(&self) -> usize {
        self.rope.len_lines().max(1)
    }

    /// Returns a UTF-8 snapshot of the whole buffer.
    pub(crate) fn to_string(&self) -> String {
        self.rope.to_string()
    }

    /// Replace the full buffer text with a fresh snapshot.
    pub(crate) fn reset(&mut self, text: &str) {
        self.rope = Rope::from_str(text);
        self.char_len = self.rope.len_chars();
        self.revision = 0;
    }

    /// Convert a global char index into `(line, column)` coordinates.
    pub(crate) fn char_to_line_col(&self, char_index: usize) -> (usize, usize) {
        let clamped = char_index.min(self.char_len);
        let line = line_for_char(&self.rope, clamped);
        let line_start = self.rope.line_to_char(line);
        let col = clamped
            .saturating_sub(line_start)
            .min(self.line_len_chars(line));
        (line, col)
    }

    /// Convert `(line, column)` into a global char index.
    pub(crate) fn line_col_to_char(&self, line: usize, column: usize) -> usize {
        if line >= self.line_count() {
            return self.char_len;
        }
        let line_start = self.rope.line_to_char(line);
        line_start + column.min(self.line_len_chars(line))
    }

    /// Returns a line as UTF-8 without trailing `\\r?\\n`.
    pub(crate) fn line_without_newline(&self, line: usize) -> String {
        if line >= self.line_count() {
            return String::new();
        }
        let line_slice = self.rope.line(line);
        let keep_chars = self.line_len_chars(line);
        line_slice.slice(..keep_chars).to_string()
    }

    /// Returns the character length of a line without trailing `\\r?\\n`.
    pub(crate) fn line_len_chars(&self, line: usize) -> usize {
        if line >= self.line_count() {
            return 0;
        }
        let line_slice = self.rope.line(line);
        let mut len = line_slice.len_chars();
        if len == 0 {
            return 0;
        }
        let last_char = line_slice.char(len - 1);
        if last_char == '\n' {
            len = len.saturating_sub(1);
            if len > 0 && line_slice.char(len - 1) == '\r' {
                len = len.saturating_sub(1);
            }
        } else if last_char == '\r' {
            len = len.saturating_sub(1);
        }
        len
    }

    /// Returns a UTF-8 snapshot for the given char range.
    pub(crate) fn slice_chars(&self, range: Range<usize>) -> String {
        let start = range.start.min(self.char_len);
        let end = range.end.min(self.char_len);
        if start >= end {
            return String::new();
        }
        self.rope.slice(start..end).to_string()
    }

    /// Insert text at the given char position.
    pub(crate) fn insert_text(
        &mut self,
        char_index: usize,
        text: &str,
    ) -> Option<VirtualEditDelta> {
        if text.is_empty() {
            return None;
        }
        let start = char_index.min(self.char_len);
        let start_line = line_for_char(&self.rope, start);
        let inserted = text.chars().count();
        self.rope.insert(start, text);
        self.char_len = self.char_len.saturating_add(inserted);
        self.revision = self.revision.wrapping_add(1);
        let new_end_line = line_for_char(&self.rope, start.saturating_add(inserted));
        Some(VirtualEditDelta {
            start_line,
            old_end_line: start_line,
            new_end_line,
            char_delta: inserted as isize,
        })
    }

    /// Delete a char range.
    pub(crate) fn delete_char_range(&mut self, range: Range<usize>) -> Option<VirtualEditDelta> {
        let start = range.start.min(self.char_len);
        let end = range.end.min(self.char_len);
        if start >= end {
            return None;
        }
        let removed = end.saturating_sub(start);
        let start_line = line_for_char(&self.rope, start);
        let old_end_line = line_for_char(&self.rope, end);
        self.rope.remove(start..end);
        self.char_len = self.char_len.saturating_sub(removed);
        self.revision = self.revision.wrapping_add(1);
        let new_end_line = line_for_char(&self.rope, start);
        Some(VirtualEditDelta {
            start_line,
            old_end_line,
            new_end_line,
            char_delta: -(removed as isize),
        })
    }

    /// Replace a char range with new text.
    pub(crate) fn replace_char_range(
        &mut self,
        range: Range<usize>,
        text: &str,
    ) -> Option<VirtualEditDelta> {
        let start = range.start.min(self.char_len);
        let end = range.end.min(self.char_len);
        if start > end {
            return None;
        }
        if start == end && text.is_empty() {
            return None;
        }
        let old_end_line = line_for_char(&self.rope, end);
        let start_line = line_for_char(&self.rope, start);
        let removed = end.saturating_sub(start) as isize;
        let inserted = text.chars().count() as isize;
        if start < end {
            self.rope.remove(start..end);
        }
        if !text.is_empty() {
            self.rope.insert(start, text);
        }
        self.char_len = (self.char_len as isize + inserted - removed).max(0) as usize;
        self.revision = self.revision.wrapping_add(1);
        let new_end_line =
            line_for_char(&self.rope, start.saturating_add(inserted.max(0) as usize));
        Some(VirtualEditDelta {
            start_line,
            old_end_line,
            new_end_line,
            char_delta: inserted - removed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_char_conversions_roundtrip_utf8() {
        let buf = RopeBuffer::new("aé\nb");
        let idx = buf.line_col_to_char(0, 2);
        assert_eq!(idx, 2);
        let (line, col) = buf.char_to_line_col(idx);
        assert_eq!((line, col), (0, 2));
    }

    #[test]
    fn replace_range_returns_delta() {
        let mut buf = RopeBuffer::new("one\ntwo\nthree");
        let delta = buf.replace_char_range(4..7, "dos\nzwei").expect("delta");
        assert_eq!(delta.start_line, 1);
        assert_eq!(delta.old_end_line, 1);
        assert!(delta.new_end_line >= 1);
        assert_eq!(buf.line_without_newline(1), "dos");
        assert_eq!(buf.line_without_newline(2), "zwei");
    }
}
