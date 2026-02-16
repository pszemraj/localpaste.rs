//! Shared text-coordinate helpers for editor buffers.

use ropey::Rope;

/// Clamp a global char index and return its containing line index.
pub(crate) fn line_for_char(rope: &Rope, char_index: usize) -> usize {
    rope.char_to_line(char_index.min(rope.len_chars()))
}
