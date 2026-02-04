//! Editor buffer, line index, and mode helpers for the native GUI.

use eframe::egui;
use std::any::TypeId;

/// Tracks the current editor buffer text and simple revision counters.
#[derive(Default)]
pub(super) struct EditorBuffer {
    text: String,
    revision: u64,
    char_len: usize,
}

impl EditorBuffer {
    pub(super) fn new(text: String) -> Self {
        let char_len = text.chars().count();
        Self {
            text,
            revision: 0,
            char_len,
        }
    }

    pub(super) fn reset(&mut self, text: String) {
        self.text = text;
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

    pub(super) fn as_str(&self) -> &str {
        self.text.as_str()
    }

    pub(super) fn to_string(&self) -> String {
        self.text.clone()
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
        let inserted = <String as egui::TextBuffer>::insert_text(&mut self.text, text, char_index);
        if inserted > 0 {
            self.revision = self.revision.wrapping_add(1);
            self.char_len = self.char_len.saturating_add(inserted);
        }
        inserted
    }

    fn delete_char_range(&mut self, char_range: std::ops::Range<usize>) {
        if char_range.start == char_range.end {
            return;
        }
        let removed = char_range.end.saturating_sub(char_range.start);
        <String as egui::TextBuffer>::delete_char_range(&mut self.text, char_range);
        self.revision = self.revision.wrapping_add(1);
        self.char_len = self.char_len.saturating_sub(removed);
    }

    fn clear(&mut self) {
        if self.text.is_empty() {
            return;
        }
        self.text.clear();
        self.revision = self.revision.wrapping_add(1);
        self.char_len = 0;
    }

    fn replace_with(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        self.text.clear();
        self.text.push_str(text);
        self.revision = self.revision.wrapping_add(1);
        self.char_len = text.chars().count();
    }

    fn take(&mut self) -> String {
        self.revision = self.revision.wrapping_add(1);
        self.char_len = 0;
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
}

impl EditorMode {
    pub(super) fn from_env() -> Self {
        match std::env::var("LOCALPASTE_VIRTUAL_PREVIEW") {
            Ok(value) => {
                let lowered = value.trim().to_ascii_lowercase();
                if lowered.is_empty() || lowered == "0" || lowered == "false" {
                    Self::TextEdit
                } else {
                    Self::VirtualPreview
                }
            }
            Err(_) => Self::TextEdit,
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
