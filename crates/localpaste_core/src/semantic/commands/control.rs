//! Shell control structure (`for`, `while`, `until`, `if`, and `case` blocks)
//! inside command sequences.

use super::{has_prose_grammar, line_is_command, strip_shell_prompt};

/// Shell keywords that open, continue, or close a control block.
const CONTROL_KEYWORDS: &[&str] = &[
    "for", "while", "until", "if", "elif", "then", "else", "do", "done", "fi", "case", "esac",
];

/// Classify each line as shell control syntax or an ordinary line.
///
/// A control line starts with a keyword such as `for`, `if`, `do`, or `done`,
/// is a `case` item such as `start) systemctl start app ;;`, or ends a block
/// after a command (`ssh $host uptime; done`). Headers need their shape
/// (`for NAME in WORDS`, a condition without English grammar), blocks must
/// open and close in order with `do` and `then` where they belong, and a
/// command on a control line must itself be a command line. Prose such as
/// `if you see errors, call me` or `for the record` fails these checks.
///
/// # Arguments
/// - `lines`: Trimmed non-comment lines, optionally prompt-prefixed.
/// - `script`: Whether the lines form a script, so `echo` prints any words.
///
/// # Returns
/// For each line, whether it is control syntax, or `None` when a control line
/// is malformed or a block is left open.
pub(super) fn shell_control_lines(lines: &[&str], script: bool) -> Option<Vec<bool>> {
    // Each open block: its closing keyword and the keyword its header awaits.
    let mut blocks: Vec<(&'static str, Option<&'static str>)> = Vec::new();
    let mut kinds = Vec::with_capacity(lines.len());
    for line in lines {
        let line = strip_shell_prompt(line.trim().trim_matches('`'));
        let statements = statements(line);
        let first = line.split_whitespace().next().unwrap_or_default();
        let case_item =
            blocks.last().is_some_and(|(closer, _)| *closer == "esac") && first.ends_with(')');
        let closes_block = statements.len() > 1
            && statements.last().is_some_and(|last| {
                ["done", "fi", "esac"].contains(&last.split_whitespace().next().unwrap_or_default())
            });
        if !CONTROL_KEYWORDS.contains(&first) && !case_item && !closes_block {
            // A command may not stand between a header and its `do` or `then`.
            if blocks.last().is_some_and(|(_, awaits)| awaits.is_some()) {
                return None;
            }
            kinds.push(false);
            continue;
        }
        for statement in statements {
            if !statement.is_empty() && !control_statement(statement, &mut blocks, script) {
                return None;
            }
        }
        kinds.push(true);
    }
    blocks.is_empty().then_some(kinds)
}

/// Split a control line at each `;` outside quotes and parentheses, so an
/// arithmetic header `for ((i = 0; i < 3; i++))` and `echo "a; b"` stay whole.
fn statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote = None;
    let mut start = 0;
    for (index, ch) in line.char_indices() {
        match (quote, ch) {
            (Some(open), _) if ch == open => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(ch),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ';') if depth <= 0 => {
                out.push(line[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    out.push(line[start..].trim());
    out
}

/// Apply one `;`-separated statement of a control line to the open blocks.
fn control_statement(
    statement: &str,
    blocks: &mut Vec<(&'static str, Option<&'static str>)>,
    script: bool,
) -> bool {
    let (keyword, rest) = statement
        .split_once(char::is_whitespace)
        .map_or((statement, ""), |(keyword, rest)| (keyword, rest.trim()));
    let condition = !rest.is_empty() && !has_prose_grammar(rest);
    let command = |text: &str| text.is_empty() || line_is_command(text, true, script);
    let top = blocks.last().copied();
    match keyword {
        "for" => {
            blocks.push(("done", Some("do")));
            for_header(rest)
        }
        "while" | "until" => {
            blocks.push(("done", Some("do")));
            condition
        }
        "if" => {
            blocks.push(("fi", Some("then")));
            condition
        }
        "case" => {
            blocks.push(("esac", None));
            rest.ends_with(" in") && rest.split_whitespace().count() == 2
        }
        "do" | "then" => match blocks.last_mut() {
            Some((_, awaits)) if *awaits == Some(keyword) => {
                *awaits = None;
                command(rest)
            }
            _ => false,
        },
        "elif" => match blocks.last_mut() {
            Some(("fi", awaits)) if awaits.is_none() => {
                *awaits = Some("then");
                condition
            }
            _ => false,
        },
        "else" => top == Some(("fi", None)) && command(rest),
        "done" | "fi" | "esac" => {
            // A loop may feed or redirect its output: `done < hosts.txt`.
            top == Some((keyword, None))
                && blocks.pop().is_some()
                && (rest.is_empty() || rest.starts_with(['<', '>', '|', '&']))
        }
        item if top == Some(("esac", None)) && item.ends_with(')') => command(rest),
        _ => top.is_none_or(|(_, awaits)| awaits.is_none()) && command(statement),
    }
}

/// Recognize `NAME in WORDS`, a bare `NAME`, or an arithmetic `((...))` loop header.
fn for_header(header: &str) -> bool {
    if header.starts_with("((") && header.ends_with("))") {
        return true;
    }
    let (name, words) = header
        .split_once(" in ")
        .map_or((header, None), |(name, words)| (name, Some(words.trim())));
    name.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_')
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && words.is_none_or(|words| !words.is_empty() && !has_prose_grammar(words))
}
