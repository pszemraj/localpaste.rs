//! Literal search ranges shared by picker excerpts and editor Find.

use std::{borrow::Cow, ops::Range};

/// Iterate nonoverlapping source byte ranges using canonical body-search case policy.
///
/// ASCII queries use ASCII folding; other insensitive queries lowercase the
/// complete strings, including contextual casing and expanding source scalars.
/// Source characters are scanned once to map normalized matches back to UTF-8.
///
/// # Arguments
/// - `content`: Original body text.
/// - `query`: Literal query; an empty query yields no ranges.
/// - `case_sensitive`: Whether to preserve case when matching.
///
/// # Returns
/// Matching ranges at source character boundaries, in document order. A partial
/// match within a lowercase expansion selects its whole source character.
///
/// # Panics
/// Slices use UTF-8 boundaries returned by substring searches and character indices.
pub(crate) fn search_match_ranges<'a>(
    content: &'a str,
    query: &str,
    case_sensitive: bool,
) -> impl Iterator<Item = Range<usize>> + 'a {
    let query = if case_sensitive {
        query.to_owned()
    } else {
        query.to_lowercase()
    };
    let unicode_fold = !case_sensitive && !query.is_ascii();
    let normalized = if unicode_fold {
        Cow::Owned(content.to_lowercase())
    } else {
        Cow::Borrowed(content)
    };
    let mut search_offset = 0;
    let mut source_chars = content.char_indices();
    let mut source_normalized_end = 0;
    std::iter::from_fn(move || loop {
        if query.is_empty() {
            return None;
        }
        let remaining = &normalized[search_offset..];
        let relative = if !case_sensitive && !unicode_fold {
            localpaste_core::text::find_ascii_case_insensitive_range(remaining, &query)?
        } else {
            let start = remaining.find(&query)?;
            start..start + query.len()
        };
        let range = search_offset + relative.start..search_offset + relative.end;
        search_offset = range.end;
        if !unicode_fold {
            return Some(range);
        }
        // Two normalized hits can belong to one expanding source character.
        // Emit that character once, preserving nonoverlapping editor selections.
        if range.start < source_normalized_end {
            continue;
        }
        let mut source_start = None;
        for (offset, ch) in source_chars.by_ref() {
            source_normalized_end += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
            if source_start.is_none() && range.start < source_normalized_end {
                source_start = Some(offset);
            }
            if range.end <= source_normalized_end {
                return Some(source_start.unwrap_or(offset)..offset + ch.len_utf8());
            }
        }
        return None;
    })
}
