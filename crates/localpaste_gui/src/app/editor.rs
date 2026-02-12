//! Editor buffer, line index, and mode helpers for the native GUI.

use super::util::env_value_enabled;
use eframe::egui;
use ropey::Rope;
use std::any::TypeId;
use std::fmt;

/// Delta summary for the most recent text mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EditDelta {
    /// Line where the mutation started in the pre-edit buffer.
    pub(super) start_line: usize,
    /// Last impacted line index in the pre-edit buffer.
    pub(super) old_end_line: usize,
    /// Last impacted line index in the post-edit buffer.
    pub(super) new_end_line: usize,
    /// Character delta (`new_chars - old_chars`) from the mutation.
    pub(super) char_delta: isize,
}

fn line_for_char(rope: &Rope, char_index: usize) -> usize {
    rope.char_to_line(char_index.min(rope.len_chars()))
}

/// Tracks the current editor buffer text and simple revision counters.
#[derive(Default)]
pub(super) struct EditorBuffer {
    text: String,
    rope: Rope,
    last_delta: Option<EditDelta>,
    revision: u64,
    char_len: usize,
}

impl EditorBuffer {
    pub(super) fn new(text: String) -> Self {
        let char_len = text.chars().count();
        Self {
            rope: Rope::from_str(text.as_str()),
            text,
            last_delta: None,
            revision: 0,
            char_len,
        }
    }

    pub(super) fn reset(&mut self, text: String) {
        self.rope = Rope::from_str(text.as_str());
        self.text = text;
        self.last_delta = None;
        self.revision = 0;
        self.char_len = self.text.chars().count();
    }

    pub(super) fn len(&self) -> usize {
        self.text.len()
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn chars_len(&self) -> usize {
        self.char_len
    }

    #[cfg(test)]
    pub(super) fn rope(&self) -> &Rope {
        &self.rope
    }

    pub(super) fn as_str(&self) -> &str {
        self.text.as_str()
    }

    pub(super) fn take_edit_delta(&mut self) -> Option<EditDelta> {
        self.last_delta.take()
    }

    pub(super) fn line_range_chars(&self, char_index: usize) -> (usize, usize) {
        let idx = char_index.min(self.char_len);
        let line = line_for_char(&self.rope, idx);
        let start = self.rope.line_to_char(line);
        let end = if line + 1 < self.rope.len_lines() {
            self.rope.line_to_char(line + 1)
        } else {
            self.rope.len_chars()
        };
        (start, end)
    }
}

impl fmt::Display for EditorBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text.as_str())
    }
}

/// Holds byte offsets for each line in the buffer to enable fast line lookups.
#[derive(Default)]
pub(super) struct EditorLineIndex {
    revision: u64,
    text_len: usize,
    lines: Vec<LineEntry>,
}

#[derive(Clone, Copy)]
struct LineEntry {
    start: usize,
    len: usize,
}

impl EditorLineIndex {
    pub(super) fn reset(&mut self) {
        self.revision = 0;
        self.text_len = 0;
        self.lines.clear();
    }

    pub(super) fn ensure_for(&mut self, revision: u64, text: &str) {
        if !self.lines.is_empty() && self.revision == revision && self.text_len == text.len() {
            return;
        }
        self.rebuild(revision, text);
    }

    pub(super) fn rebuild(&mut self, revision: u64, text: &str) {
        self.lines.clear();
        let mut start = 0usize;
        for (idx, byte) in text.as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                let len = idx + 1 - start;
                self.lines.push(LineEntry { start, len });
                start = idx + 1;
            }
        }
        if start <= text.len() {
            self.lines.push(LineEntry {
                start,
                len: text.len().saturating_sub(start),
            });
        }
        if self.lines.is_empty() {
            self.lines.push(LineEntry { start: 0, len: 0 });
        }
        self.revision = revision;
        self.text_len = text.len();
    }

    pub(super) fn line_count(&self) -> usize {
        self.lines.len().max(1)
    }

    pub(super) fn line_slice<'a>(&self, text: &'a str, index: usize) -> &'a str {
        let Some(line) = self.lines.get(index) else {
            return "";
        };
        let end = line.start.saturating_add(line.len).min(text.len());
        &text[line.start..end]
    }

    pub(super) fn line_without_newline<'a>(&self, text: &'a str, index: usize) -> &'a str {
        let mut line = self.line_slice(text, index);
        if let Some(trimmed) = line.strip_suffix('\n') {
            line = trimmed;
        }
        if let Some(trimmed) = line.strip_suffix('\r') {
            line = trimmed;
        }
        line
    }
}

impl egui::TextBuffer for EditorBuffer {
    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.text.as_str()
    }

    fn insert_text(&mut self, text: &str, char_index: usize) -> usize {
        let start_char = char_index.min(self.char_len);
        let start_line = line_for_char(&self.rope, start_char);
        let inserted = <String as egui::TextBuffer>::insert_text(&mut self.text, text, start_char);
        if inserted > 0 {
            self.rope.insert(start_char, text);
            let end_line = line_for_char(&self.rope, start_char.saturating_add(inserted));
            self.last_delta = Some(EditDelta {
                start_line,
                old_end_line: start_line,
                new_end_line: end_line,
                char_delta: inserted as isize,
            });
            self.revision = self.revision.wrapping_add(1);
            self.char_len = self.char_len.saturating_add(inserted);
        }
        inserted
    }

    fn delete_char_range(&mut self, char_range: std::ops::Range<usize>) {
        if char_range.start == char_range.end {
            return;
        }
        let start_char = char_range.start.min(self.char_len);
        let end_char = char_range.end.min(self.char_len);
        if start_char >= end_char {
            return;
        }
        let removed = end_char.saturating_sub(start_char);
        let start_line = line_for_char(&self.rope, start_char);
        let old_end_line = line_for_char(&self.rope, end_char);
        <String as egui::TextBuffer>::delete_char_range(&mut self.text, start_char..end_char);
        self.rope.remove(start_char..end_char);
        let new_end_line = line_for_char(&self.rope, start_char);
        self.last_delta = Some(EditDelta {
            start_line,
            old_end_line,
            new_end_line,
            char_delta: -(removed as isize),
        });
        self.revision = self.revision.wrapping_add(1);
        self.char_len = self.char_len.saturating_sub(removed);
    }

    fn clear(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let old_end_line = self.rope.len_lines().saturating_sub(1);
        self.text.clear();
        self.rope = Rope::new();
        self.last_delta = Some(EditDelta {
            start_line: 0,
            old_end_line,
            new_end_line: 0,
            char_delta: -(self.char_len as isize),
        });
        self.revision = self.revision.wrapping_add(1);
        self.char_len = 0;
    }

    fn replace_with(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        let old_end_line = self.rope.len_lines().saturating_sub(1);
        let old_chars = self.char_len as isize;
        self.text.clear();
        self.text.push_str(text);
        self.rope = Rope::from_str(text);
        let new_chars = text.chars().count() as isize;
        let new_end_line = self.rope.len_lines().saturating_sub(1);
        self.last_delta = Some(EditDelta {
            start_line: 0,
            old_end_line,
            new_end_line,
            char_delta: new_chars - old_chars,
        });
        self.revision = self.revision.wrapping_add(1);
        self.char_len = new_chars as usize;
    }

    fn take(&mut self) -> String {
        let old_end_line = self.rope.len_lines().saturating_sub(1);
        let old_chars = self.char_len as isize;
        self.revision = self.revision.wrapping_add(1);
        self.char_len = 0;
        self.rope = Rope::new();
        self.last_delta = Some(EditDelta {
            start_line: 0,
            old_end_line,
            new_end_line: 0,
            char_delta: -old_chars,
        });
        std::mem::take(&mut self.text)
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

/// Selects the editor rendering mode based on environment configuration.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum EditorMode {
    TextEdit,
    VirtualPreview,
    VirtualEditor,
}

impl EditorMode {
    pub(super) fn from_env() -> Self {
        let preview_mode = || match std::env::var("LOCALPASTE_VIRTUAL_PREVIEW") {
            Ok(value) if env_value_enabled(&value) => Self::VirtualPreview,
            _ => Self::TextEdit,
        };

        if let Ok(value) = std::env::var("LOCALPASTE_VIRTUAL_EDITOR") {
            if env_value_enabled(&value) {
                return Self::VirtualEditor;
            }
            return preview_mode();
        }

        match std::env::var("LOCALPASTE_VIRTUAL_PREVIEW") {
            Ok(value) if env_value_enabled(&value) => Self::VirtualPreview,
            _ => Self::VirtualEditor,
        }
    }
}

/// Returns the internal revision of an EditorBuffer-backed TextBuffer.
pub(super) fn editor_buffer_revision(text: &dyn egui::TextBuffer) -> Option<u64> {
    if text.type_id() == TypeId::of::<EditorBuffer>() {
        let ptr = text as *const dyn egui::TextBuffer as *const EditorBuffer;
        // Safety: we only cast when the type id matches.
        let buffer = unsafe { &*ptr };
        Some(buffer.revision)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::TextBuffer;

    #[test]
    fn editor_buffer_maintains_rope_after_edits() {
        let mut buffer = EditorBuffer::new("ab\ncd".to_string());
        buffer.insert_text("X", 1);
        buffer.delete_char_range(0..1);
        buffer.replace_with("hello\nworld");
        assert_eq!(buffer.as_str(), "hello\nworld");
        assert_eq!(buffer.rope().to_string(), "hello\nworld");
        assert_eq!(buffer.chars_len(), "hello\nworld".chars().count());
    }

    #[test]
    fn editor_mode_env_matrix() {
        let preview_key = "LOCALPASTE_VIRTUAL_PREVIEW";
        let editor_key = "LOCALPASTE_VIRTUAL_EDITOR";
        let old_preview = std::env::var(preview_key).ok();
        let old_editor = std::env::var(editor_key).ok();
        std::env::remove_var(preview_key);
        std::env::remove_var(editor_key);

        // Default mode is the editable virtual editor.
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualEditor);

        // Read-only preview can be forced for diagnostics.
        std::env::set_var(preview_key, "1");
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualPreview);

        // Explicit virtual editor flag wins over preview.
        std::env::set_var(editor_key, "1");
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualEditor);

        // Explicitly disabling virtual editor falls back to preview/textedit.
        std::env::set_var(editor_key, "0");
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualPreview);

        std::env::set_var(preview_key, "0");
        assert_eq!(EditorMode::from_env(), EditorMode::TextEdit);

        if let Some(value) = old_preview {
            std::env::set_var(preview_key, value);
        } else {
            std::env::remove_var(preview_key);
        }
        if let Some(value) = old_editor {
            std::env::set_var(editor_key, value);
        } else {
            std::env::remove_var(editor_key);
        }
    }

    #[test]
    fn line_range_chars_includes_newline_for_non_terminal_lines() {
        let buffer = EditorBuffer::new("one\ntwo\nthree".to_string());
        let (start, end) = buffer.line_range_chars(5);
        let selected: String = buffer
            .as_str()
            .chars()
            .skip(start)
            .take(end - start)
            .collect();
        assert_eq!(selected, "two\n");
    }

    #[test]
    fn line_range_chars_excludes_missing_newline_for_last_line() {
        let buffer = EditorBuffer::new("one\ntwo".to_string());
        let (start, end) = buffer.line_range_chars(5);
        let selected: String = buffer
            .as_str()
            .chars()
            .skip(start)
            .take(end - start)
            .collect();
        assert_eq!(selected, "two");
    }
}
