//! Whole-body Makefile evidence and the note shapes mistaken for recipes.

use super::{commands, sample_prefix};

/// Recognize complete Makefile bodies without treating lowercase words as commands.
///
/// # Arguments
/// - `content`: Paste body sampled for complete Makefile structure.
///
/// # Returns
/// `makefile` for distinctive directives or complete target/recipe bodies whose
/// every recipe is command-shaped, or `None` for prose, documentary labels,
/// incomplete definitions, or syntax this recognizer cannot prove.
pub(crate) fn makefile_body_language(content: &str) -> Option<&'static str> {
    let sample = sample_prefix(content);
    let note_body = makefile_note_body(sample);
    let mut saw_target = false;
    let mut saw_recipe = false;
    let mut saw_directive = false;
    let mut documentary_target = false;
    let mut markdown_heading = false;
    let mut capitalized_targets = true;
    let mut in_definition = false;
    let mut continuation = false;
    for line in sample.lines().filter(|line| !line.trim().is_empty()) {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            markdown_heading |= crate::models::paste::is_markdown_heading_line(trimmed);
            continue;
        }
        if continuation {
            continuation = trimmed.ends_with('\\');
            continue;
        }
        if in_definition {
            in_definition = trimmed != "endef";
            continue;
        }
        if line.starts_with('\t') {
            // Prose, logs, assembly, and other tabbed code reach this point with
            // lines that are neither commands nor invocation-shaped.
            if !saw_target
                || !(recipe_has_command_evidence(trimmed)
                    || (!note_body && recipe_is_invocation(trimmed)))
            {
                return None;
            }
            saw_recipe = true;
        } else if directive_with_argument(trimmed, "define") {
            in_definition = true;
            saw_directive = true;
        } else if looks_like_assignment(trimmed)
            || [
                "include", "-include", "sinclude", "ifeq", "ifneq", "ifdef", "ifndef", "vpath",
            ]
            .iter()
            .any(|directive| directive_with_argument(trimmed, directive))
            || matches!(trimmed, "export" | "else" | "endif" | "vpath")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("else ")
            || (trimmed.starts_with("$(") && trimmed.ends_with(')'))
        {
            saw_directive |= directive_with_argument(trimmed, "vpath")
                || (trimmed.starts_with("$(") && trimmed.ends_with(')'))
                || ["include", "-include", "sinclude"].iter().any(|directive| {
                    directive_with_argument(trimmed, directive)
                        && trimmed
                            .split_whitespace()
                            .skip(1)
                            .all(|path| path.ends_with(".mk"))
                });
        } else if let Some(target) = trimmed
            .split_once(':')
            .map(|(target, _)| target.trim())
            .filter(|target| {
                !(target.is_empty() || (target.starts_with('[') && target.ends_with(']')))
            })
        {
            documentary_target = !saw_target
                && trimmed.strip_suffix(':').is_some_and(|target| {
                    target.eq_ignore_ascii_case("example") || target.eq_ignore_ascii_case("steps")
                });
            capitalized_targets &= target.starts_with(char::is_uppercase);
            saw_target = true;
        } else {
            return None;
        }
        continuation = trimmed.ends_with('\\');
    }
    // Capitalized labels under a Markdown heading are README sections, whose
    // command examples keep their document classification.
    let documentary = documentary_target || (markdown_heading && capitalized_targets);
    if in_definition || continuation {
        None
    } else if !saw_directive && crate::detection::looks_like_python_source(sample) {
        // Source anchors resolve ambiguous recipes, while a closed Make define
        // or another distinctive directive may itself contain Python source.
        None
    } else if saw_target && saw_recipe && (!documentary || saw_directive) {
        Some("makefile")
    } else {
        saw_directive.then_some("makefile")
    }
}

/// Require a complete note body before replacing an inferred Makefile label.
///
/// Unknown Make syntax alone is not document evidence. A target-shaped note
/// needs indented sentences or multiple one-word list items and no commands.
/// Explicit note labels such as `Agenda:` also support short items such as an
/// agenda entry. A `#` comment with such a word only lets a sentence-shaped line
/// read as prose; an invocation-shaped line stays a recipe.
///
/// # Arguments
/// - `content`: Paste body to inspect for a complete indented note.
///
/// # Returns
/// Whether the body supplies positive prose/list evidence rather than a recipe.
pub(crate) fn makefile_note_body(content: &str) -> bool {
    let mut saw_heading = false;
    let mut saw_prose = false;
    let mut word_items = 0;
    let mut note_label = false;
    let mut note_comment = false;
    let has_note_words = |heading: &str| {
        heading.split_whitespace().any(|word| {
            ["agenda", "notes", "list", "todo"]
                .iter()
                .any(|note| word.eq_ignore_ascii_case(note))
        })
    };
    for line in sample_prefix(content)
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            note_comment |= has_note_words(trimmed.trim_start_matches('#'));
            continue;
        }
        if line.starts_with('\t') && saw_heading {
            if recipe_has_command_evidence(trimmed) {
                return false;
            }
            let arguments: Vec<_> = trimmed.split_whitespace().skip(1).collect();
            saw_prose |= crate::detection::has_unquoted_prose_copula(&arguments, false);
            saw_prose |= super::looks_like_prose(trimmed)
                && (note_label
                    || (note_comment && !recipe_is_invocation(trimmed))
                    || arguments.iter().any(|word| {
                        [
                            "a", "an", "the", "for", "to", "about", "please", "we", "you", "our",
                            "your",
                        ]
                        .contains(word)
                    }));
            let words: Vec<_> = trimmed.split_whitespace().collect();
            let plain_words = words
                .iter()
                .all(|word| word.chars().all(char::is_alphabetic));
            word_items += usize::from(plain_words && words.len() == 1);
            saw_prose |= note_label && plain_words && words.len() <= 2;
        } else if trimmed.strip_suffix(':').is_some_and(|heading| {
            !heading.is_empty() && heading.chars().all(|ch| ch.is_alphabetic() || ch == ' ')
        }) {
            if saw_heading {
                return false;
            }
            saw_heading = true;
            note_label |= has_note_words(trimmed.trim_end_matches(':'));
        } else {
            return false;
        }
    }
    saw_heading && (saw_prose || word_items >= 2)
}

/// Recognize a bare tool invocation such as `zig build` or `poetry run pytest`.
///
/// An unlisted tool carries no flag, path, or variable, so its structure is the
/// evidence: a lowercase executable name followed by plain operands. Quotes,
/// brackets, separators, a capitalized or numeric lead, or a numeric first operand
/// mark prose, data, logs, or other languages' statements instead.
fn recipe_is_invocation(line: &str) -> bool {
    let mut words = line.trim_start_matches(['@', '+']).split_whitespace();
    let plain = |word: &str| {
        word.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'))
    };
    let tool = words.next().unwrap_or_default();
    let operands: Vec<_> = words.collect();
    tool.starts_with(|ch: char| ch.is_ascii_lowercase())
        && tool
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
        && operands
            .first()
            .is_some_and(|first| !first.chars().all(|ch| ch.is_ascii_digit()))
        && operands.iter().all(|word| plain(word))
}

fn recipe_has_command_evidence(line: &str) -> bool {
    let recipe = line.trim_start_matches(['@', '+', '-']);
    let command = recipe.split_whitespace().next().unwrap_or_default();
    commands::extract_command_handle(recipe).is_some()
        || commands::has_unlisted_command_syntax(recipe)
        || matches!(
            command,
            "cc" | "gcc"
                | "clang"
                | "c++"
                | "g++"
                | "clang++"
                | "rm"
                | "cp"
                | "mv"
                | "touch"
                | "install"
                | "true"
                | "false"
        )
        || crate::detection::is_shell_path(command)
        || recipe.split('$').skip(1).any(|rest| {
            rest.starts_with(['(', '{', '@', '<', '^', '?', '*', '+', '|', '%'])
                || rest
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        })
        || recipe.contains(['|', '>', '<', ';', '&', '='])
        || (recipe.starts_with("[ ") && recipe.contains(']'))
}

fn directive_with_argument(line: &str, directive: &str) -> bool {
    line.strip_prefix(directive)
        .is_some_and(|rest| rest.starts_with(char::is_whitespace) && !rest.trim().is_empty())
}

fn looks_like_assignment(mut line: &str) -> bool {
    while let Some(rest) = ["export ", "override ", "private ", "unexport "]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))
    {
        line = rest.trim_start();
    }
    ["::=", "?=", ":=", "+=", "!=", "="].iter().any(|operator| {
        line.split_once(operator).is_some_and(|(name, _)| {
            let name = name.trim();
            !name.is_empty() && !name.ends_with(':') && !name.contains(char::is_whitespace)
        })
    })
}
