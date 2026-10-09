//! Whole-sample compound Python evidence for ambiguous Makefile-shaped bodies.

use crate::text::{utf8_prefix_by_bytes, TEXT_SAMPLE_MAX_BYTES};

/// Recognize compound source only while every sampled line remains source-shaped.
///
/// # Arguments
/// - `sample`: Paste body, inspected within the shared text sample limit.
///
/// # Returns
/// Whether a compound header has an indented Python body without unrelated
/// targets, Make directives, command recipes, or prose elsewhere in the sample.
///
/// # Panics
/// Panics only if the lexical scanner's character-boundary invariant is violated.
pub(super) fn compound_body(sample: &str) -> bool {
    let sample = utf8_prefix_by_bytes(sample, TEXT_SAMPLE_MAX_BYTES);
    let mut compound = false;
    let mut body = false;
    let mut nesting = 0isize;
    let mut continued = false;
    let mut pending_header = false;
    let mut string_block: Option<&str> = None;
    for raw_line in sample.lines().take(512) {
        if let Some(quote) = string_block {
            if raw_line.matches(quote).count() % 2 == 1 {
                string_block = None;
            }
            continue;
        }
        let comment = syntax_chars(raw_line).find(|(_, ch)| *ch == '#');
        let line = raw_line[..comment.map_or(raw_line.len(), |(idx, _)| idx)].trim();
        if line.is_empty() {
            continue;
        }
        if syntax_chars(line).any(|(_, ch)| ch == '$') {
            return false;
        }
        let mut header = compound_header(line);
        let starts_header = !header
            && nesting == 0
            && matches!(
                line.split([' ', '\t', '(', '[', '{']).next(),
                Some("if" | "elif" | "while" | "for" | "with" | "match" | "case")
            )
            && syntax_chars(line).any(|(_, ch)| matches!(ch, '(' | '[' | '{'));
        if nesting == 0 && !continued && !header && !starts_header && !statement(line) {
            return false;
        }
        pending_header |= starts_header;
        for (_, ch) in syntax_chars(line) {
            nesting += match ch {
                '(' | '[' | '{' => 1,
                ')' | ']' | '}' => -1,
                _ => 0,
            };
        }
        if pending_header && nesting == 0 {
            if !line.ends_with(':') {
                return false;
            }
            header = true;
            pending_header = false;
        }
        compound |= header;
        body |= compound && raw_line.starts_with(char::is_whitespace) && !header && !pending_header;
        continued = syntax_chars(line).last().is_some_and(|(_, ch)| ch == '\\');
        if let Some(quote) = ["\"\"\"", "'''"]
            .into_iter()
            .find(|quote| line.contains(*quote))
        {
            if line.matches(quote).count() % 2 == 1 {
                string_block = Some(quote);
            }
        }
    }
    compound && body
}

fn compound_header(line: &str) -> bool {
    line.strip_suffix(':').is_some_and(|header| {
        matches!(header, "try" | "except" | "else" | "finally")
            || [
                "if ",
                "elif ",
                "while ",
                "for ",
                "async for ",
                "with ",
                "async with ",
                "except ",
                "match ",
                "case ",
                "def ",
                "async def ",
                "class ",
            ]
            .iter()
            .any(|prefix| {
                header
                    .strip_prefix(prefix)
                    .is_some_and(|rest| !rest.trim().is_empty())
            })
    })
}

fn statement(line: &str) -> bool {
    let assignment = top_level_delimiter(line, '=').is_some_and(|idx| {
        let target = line[..idx]
            .trim_end()
            .trim_end_matches(['+', '-', '*', '/', '%', '|', '&', '^', '<', '>', '@']);
        let value = line[idx + 1..].trim();
        assignment_target(target) && !value.is_empty() && !value.starts_with('=')
    });
    let annotation = top_level_delimiter(line, ':')
        .is_some_and(|idx| assignment_target(&line[..idx]) && !line[idx + 1..].trim().is_empty());
    let call = line
        .split_once('(')
        .is_some_and(|(name, _)| !name.is_empty() && name.trim().split('.').all(identifier));
    assignment
        || annotation
        || call
        || matches!(line, "pass" | "break" | "continue" | "return" | "raise")
        || [
            "return ",
            "raise ",
            "yield ",
            "await ",
            "assert ",
            "del ",
            "global ",
            "nonlocal ",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

fn assignment_target(target: &str) -> bool {
    let mut pending = vec![target];
    while let Some(target) = pending.pop() {
        let mut target = target.trim();
        if let Some(idx) = top_level_delimiter(target, ',') {
            pending.push(&target[..idx]);
            let tail = target[idx + 1..].trim();
            if !tail.is_empty() {
                pending.push(tail);
            }
            continue;
        }
        if target.starts_with(['(', '[']) {
            let Some(idx) = closing_bracket(target).filter(|idx| idx + 1 == target.len()) else {
                return false;
            };
            pending.push(&target[1..idx]);
            continue;
        }
        if let Some(idx) = top_level_delimiter(target, ':') {
            if target[idx + 1..].trim().is_empty() {
                return false;
            }
            target = target[..idx].trim_end();
        }
        target = target.strip_prefix('*').unwrap_or(target).trim_start();
        let Some(end) = identifier_end(target) else {
            return false;
        };
        let mut rest = target[end..].trim_start();
        while !rest.is_empty() {
            if let Some(attribute) = rest.strip_prefix('.') {
                let attribute = attribute.trim_start();
                let Some(end) = identifier_end(attribute) else {
                    return false;
                };
                rest = attribute[end..].trim_start();
            } else if rest.starts_with('[') {
                let Some(end) = closing_bracket(rest) else {
                    return false;
                };
                if rest[1..end].trim().is_empty() {
                    return false;
                }
                rest = rest[end + 1..].trim_start();
            } else {
                return false;
            }
        }
    }
    true
}

fn identifier(value: &str) -> bool {
    identifier_end(value) == Some(value.len())
}

fn identifier_end(value: &str) -> Option<usize> {
    let first = value.chars().next()?;
    if !(first == '_' || first.is_alphabetic()) {
        return None;
    }
    Some(
        value
            .char_indices()
            .find_map(|(idx, ch)| (!(ch == '_' || ch.is_alphanumeric())).then_some(idx))
            .unwrap_or(value.len()),
    )
}

fn closing_bracket(value: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in syntax_chars(value) {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn top_level_delimiter(value: &str, delimiter: char) -> Option<usize> {
    let mut depth = 0isize;
    for (idx, ch) in syntax_chars(value) {
        if depth == 0 && ch == delimiter {
            return Some(idx);
        }
        depth += match ch {
            '(' | '[' | '{' => 1,
            ')' | ']' | '}' => -1,
            _ => 0,
        };
    }
    None
}

fn syntax_chars(value: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut quote = None;
    let mut escaped = false;
    value.char_indices().filter(move |(_, ch)| {
        if escaped {
            escaped = false;
        } else if *ch == '\\' && quote.is_some() {
            escaped = true;
        } else if quote == Some(*ch) {
            quote = None;
        } else if quote.is_none() {
            if matches!(*ch, '\'' | '"') {
                quote = Some(*ch);
            } else {
                return true;
            }
        }
        false
    })
}
