//! Editor buffer, line index, and mode helpers for the native GUI.

use eframe::egui;
use ropey::Rope;
use std::any::TypeId;
use std::fmt;
use tracing::warn;

use super::text_coords::line_for_char;

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

fn trim_line_endings(mut line: &str) -> &str {
    if let Some(trimmed) = line.strip_suffix('\n') {
        line = trimmed;
    }
    if let Some(trimmed) = line.strip_suffix('\r') {
        line = trimmed;
    }
    line
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
    /// Creates a new editor buffer from owned text.
    ///
    /// # Returns
    /// A buffer with rope mirror, zero revision, and cached char length.
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

    /// Replaces buffer contents and resets revision/delta tracking.
    pub(super) fn reset(&mut self, text: String) {
        self.rope = Rope::from_str(text.as_str());
        self.text = text;
        self.last_delta = None;
        self.revision = 0;
        self.char_len = self.text.chars().count();
    }

    /// Returns the current buffer size in bytes.
    ///
    /// # Returns
    /// UTF-8 byte length of the underlying text.
    pub(super) fn len(&self) -> usize {
        self.text.len()
    }

    /// Returns the current monotonic edit revision counter.
    ///
    /// # Returns
    /// Revision value incremented on mutating text operations.
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the current buffer size in Unicode scalar values.
    ///
    /// # Returns
    /// Character count cached alongside the text buffer.
    pub(super) fn chars_len(&self) -> usize {
        self.char_len
    }

    #[cfg(test)]
    /// Returns the rope mirror used by editor-mode text operations.
    ///
    /// # Returns
    /// Immutable reference to the internal [`Rope`] buffer.
    pub(super) fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Returns the current text buffer as `&str`.
    ///
    /// # Returns
    /// Borrowed UTF-8 view of the editor text.
    pub(super) fn as_str(&self) -> &str {
        self.text.as_str()
    }

    /// Takes and clears the most recent edit delta.
    ///
    /// # Returns
    /// Last tracked [`EditDelta`] when one is pending.
    pub(super) fn take_edit_delta(&mut self) -> Option<EditDelta> {
        self.last_delta.take()
    }

    /// Computes the line start/end char indices that contain `char_index`.
    ///
    /// # Arguments
    /// - `char_index`: Global character index to map into a line range.
    ///
    /// # Returns
    /// `(start, end)` char bounds for the containing line.
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
    char_len: usize,
}

impl EditorLineIndex {
    /// Clears cached line offsets and revision metadata.
    pub(super) fn reset(&mut self) {
        self.revision = 0;
        self.text_len = 0;
        self.lines.clear();
    }

    /// Rebuilds the line index when revision/length no longer match.
    ///
    /// # Arguments
    /// - `revision`: Buffer revision for cache identity.
    /// - `text`: Source text to index.
    pub(super) fn ensure_for(&mut self, revision: u64, text: &str) {
        if !self.lines.is_empty() && self.revision == revision && self.text_len == text.len() {
            return;
        }
        self.rebuild(revision, text);
    }

    /// Rebuilds cached byte/char offsets for each logical line.
    ///
    /// # Arguments
    /// - `revision`: Buffer revision for cache identity.
    /// - `text`: Source text to index.
    ///
    /// # Panics
    /// Panics if computed byte spans are not valid UTF-8 boundaries.
    pub(super) fn rebuild(&mut self, revision: u64, text: &str) {
        self.lines.clear();
        let mut start = 0usize;
        for (idx, byte) in text.as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                let len = idx + 1 - start;
                let line = trim_line_endings(&text[start..(start + len)]);
                self.lines.push(LineEntry {
                    start,
                    len,
                    char_len: line.chars().count(),
                });
                start = idx + 1;
            }
        }
        if start <= text.len() {
            let len = text.len().saturating_sub(start);
            let line = trim_line_endings(&text[start..]);
            self.lines.push(LineEntry {
                start,
                len,
                char_len: line.chars().count(),
            });
        }
        if self.lines.is_empty() {
            self.lines.push(LineEntry {
                start: 0,
                len: 0,
                char_len: 0,
            });
        }
        self.revision = revision;
        self.text_len = text.len();
    }

    /// Returns the number of indexed lines.
    ///
    /// # Returns
    /// At least `1`, including an empty trailing line entry for empty buffers.
    pub(super) fn line_count(&self) -> usize {
        self.lines.len().max(1)
    }

    /// Returns a raw line slice (including newline suffix when present).
    ///
    /// # Arguments
    /// - `text`: Source text corresponding to this index.
    /// - `index`: Logical line index.
    ///
    /// # Returns
    /// Borrowed line slice or empty string when out of bounds.
    ///
    /// # Panics
    /// Panics if stored line byte spans are not valid UTF-8 boundaries.
    pub(super) fn line_slice<'a>(&self, text: &'a str, index: usize) -> &'a str {
        let Some(line) = self.lines.get(index) else {
            return "";
        };
        let end = line.start.saturating_add(line.len).min(text.len());
        &text[line.start..end]
    }

    /// Returns a line slice with trailing CR/LF removed.
    ///
    /// # Arguments
    /// - `text`: Source text corresponding to this index.
    /// - `index`: Logical line index.
    ///
    /// # Returns
    /// Borrowed line text without newline terminators.
    pub(super) fn line_without_newline<'a>(&self, text: &'a str, index: usize) -> &'a str {
        trim_line_endings(self.line_slice(text, index))
    }

    /// Returns cached character length for a line (without newline suffix).
    ///
    /// # Returns
    /// Character length of indexed line, or `0` when out of bounds.
    pub(super) fn line_len_chars(&self, index: usize) -> usize {
        self.lines.get(index).map(|line| line.char_len).unwrap_or(0)
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
    fn parse_flag(name: &str) -> Option<bool> {
        let value = std::env::var(name).ok()?;
        match localpaste_core::config::parse_env_flag(&value) {
            Some(enabled) => Some(enabled),
            None => {
                warn!(
                    "Unrecognized value for {}='{}'; expected 1/0/true/false/yes/no/on/off. Using defaults.",
                    name, value
                );
                None
            }
        }
    }

    /// Resolves editor mode from environment feature flags.
    ///
    /// # Returns
    /// `VirtualPreview` when forced, `TextEdit` when virtual mode is disabled,
    /// otherwise `VirtualEditor`.
    pub(super) fn from_env() -> Self {
        // Preview is a force-on diagnostic mode only. Falsy preview values are
        // treated as "not forcing preview" so they cannot silently disable
        // the default virtual editor path.
        if Self::parse_flag("LOCALPASTE_VIRTUAL_PREVIEW").unwrap_or(false) {
            return Self::VirtualPreview;
        }

        match Self::parse_flag("LOCALPASTE_VIRTUAL_EDITOR") {
            Some(false) => Self::TextEdit,
            Some(true) | None => Self::VirtualEditor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::TextBuffer;
    use localpaste_core::env::{env_lock, remove_env_var, set_env_var, EnvGuard};

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
        let _lock = env_lock().lock().expect("env lock");
        let preview_key = "LOCALPASTE_VIRTUAL_PREVIEW";
        let editor_key = "LOCALPASTE_VIRTUAL_EDITOR";
        let _preview_restore = EnvGuard::remove(preview_key);
        let _editor_restore = EnvGuard::remove(editor_key);

        // Default mode is the editable virtual editor.
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualEditor);

        // Read-only preview can be forced for diagnostics.
        set_env_var(preview_key, "1");
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualPreview);

        // Preview force-mode takes precedence over editor mode flag.
        set_env_var(editor_key, "1");
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualPreview);

        // Preview=true keeps preview regardless of editor kill-switch.
        set_env_var(editor_key, "0");
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualPreview);

        // Invalid editor flag does not disable preview=true force mode.
        set_env_var(preview_key, "1");
        set_env_var(editor_key, "enabled");
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualPreview);

        // Falsy preview does not force TextEdit; defaults stay virtual editor.
        set_env_var(editor_key, "enabled");
        set_env_var(preview_key, "0");
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualEditor);

        // Explicit kill-switch still works when preview is not truthy.
        set_env_var(editor_key, "0");
        set_env_var(preview_key, "0");
        assert_eq!(EditorMode::from_env(), EditorMode::TextEdit);

        // Empty preview value is falsy and should not change default mode.
        remove_env_var(editor_key);
        set_env_var(preview_key, "");
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualEditor);

        // With preview unset and editor unset, default remains virtual editor.
        remove_env_var(preview_key);
        remove_env_var(editor_key);
        assert_eq!(EditorMode::from_env(), EditorMode::VirtualEditor);
    }

    #[test]
    fn line_range_chars_includes_newline_for_non_terminal_lines() {
        assert_line_range_chars_selection("one\ntwo\nthree", 5, "two\n");
    }

    #[test]
    fn line_range_chars_excludes_missing_newline_for_last_line() {
        assert_line_range_chars_selection("one\ntwo", 5, "two");
    }

    #[test]
    fn editor_line_index_caches_char_lengths_without_newline_suffixes() {
        let text = "ab\néç\r\n🦀";
        let mut index = EditorLineIndex::default();
        index.rebuild(7, text);

        assert_eq!(index.line_count(), 3);
        assert_eq!(index.line_len_chars(0), 2);
        assert_eq!(index.line_len_chars(1), 2);
        assert_eq!(index.line_len_chars(2), 1);
        assert_eq!(index.line_without_newline(text, 1), "éç");
    }

    fn assert_line_range_chars_selection(text: &str, char_index: usize, expected: &str) {
        let buffer = EditorBuffer::new(text.to_string());
        let (start, end) = buffer.line_range_chars(char_index);
        let selected: String = buffer
            .as_str()
            .chars()
            .skip(start)
            .take(end - start)
            .collect();
        assert_eq!(selected, expected);
    }
}
