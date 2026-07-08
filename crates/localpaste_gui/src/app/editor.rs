//! Editor line index helpers for the native GUI.

fn trim_line_endings(mut line: &str) -> &str {
    if let Some(trimmed) = line.strip_suffix('\n') {
        line = trimmed;
    }
    if let Some(trimmed) = line.strip_suffix('\r') {
        line = trimmed;
    }
    line
}

/// Holds byte offsets for each line in the buffer to enable fast line lookups.
#[derive(Default, Debug, Clone)]
pub(super) struct EditorLineIndex {
    lines: Vec<LineEntry>,
}

#[derive(Clone, Copy, Debug)]
struct LineEntry {
    start: usize,
    len: usize,
}

impl EditorLineIndex {
    /// Clears cached line offsets.
    pub(super) fn reset(&mut self) {
        self.lines.clear();
    }

    /// Rebuilds cached byte/char offsets for each logical line.
    ///
    /// # Arguments
    /// - `_revision`: Buffer revision kept at call sites for cache-owner clarity.
    /// - `text`: Source text to index.
    ///
    /// # Panics
    /// Panics if computed byte spans are not valid UTF-8 boundaries.
    pub(super) fn rebuild(&mut self, _revision: u64, text: &str) {
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
            let len = text.len().saturating_sub(start);
            self.lines.push(LineEntry { start, len });
        }
        if self.lines.is_empty() {
            self.lines.push(LineEntry { start: 0, len: 0 });
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_line_index_caches_char_lengths_without_newline_suffixes() {
        let text = "ab\néç\r\n🦀";
        let mut index = EditorLineIndex::default();
        index.rebuild(7, text);

        assert_eq!(index.line_count(), 3);
        assert_eq!(index.line_without_newline(text, 1), "éç");
    }
}
