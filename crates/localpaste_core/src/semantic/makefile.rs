//! Whole-body Makefile evidence and the note shapes mistaken for recipes.

use super::{commands, sample_prefix};

/// Recognize complete Makefile bodies without treating lowercase words as commands.
///
/// # Arguments
/// - `content`: Paste body sampled for complete Makefile structure.
///
/// # Returns
/// `makefile` for distinctive directives, `shell` for target/recipe bodies, or
/// `None` for prose, incomplete definitions, or syntax this recognizer cannot prove.
pub(crate) fn makefile_body_language(content: &str) -> Option<&'static str> {
    let sample = sample_prefix(content);
    let mut saw_target = false;
    let mut saw_recipe = false;
    let mut saw_directive = false;
    let mut in_definition = false;
    let mut continuation = false;
    for line in sample.lines().filter(|line| !line.trim().is_empty()) {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
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
            if !saw_target || !recipe_has_command_evidence(trimmed) {
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
        } else if trimmed.split_once(':').is_some_and(|(target, _)| {
            let target = target.trim();
            !(target.is_empty() || (target.starts_with('[') && target.ends_with(']')))
        }) {
            saw_target = true;
        } else {
            return None;
        }
        continuation = trimmed.ends_with('\\');
    }
    if in_definition || continuation {
        None
    } else if !saw_directive && crate::detection::looks_like_python_source(sample) {
        // Source anchors resolve ambiguous recipes, while a closed Make define
        // or another distinctive directive may itself contain Python source.
        None
    } else if saw_target && saw_recipe {
        Some("shell")
    } else {
        saw_directive.then_some("makefile")
    }
}

/// Require a complete note body before replacing an inferred Makefile label.
///
/// Unknown Make syntax alone is not document evidence. A target-shaped note
/// needs indented sentences or multiple plain-word list items and no commands.
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
    for line in sample_prefix(content)
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if line.starts_with('\t') && saw_heading {
            if recipe_has_command_evidence(trimmed) {
                return false;
            }
            saw_prose |= super::looks_like_prose(trimmed);
            let arguments: Vec<_> = trimmed.split_whitespace().skip(1).collect();
            saw_prose |= crate::detection::has_unquoted_prose_copula(&arguments, false);
            word_items += usize::from(trimmed.chars().all(char::is_alphabetic));
        } else if trimmed.strip_suffix(':').is_some_and(|heading| {
            !heading.is_empty() && heading.chars().all(|ch| ch.is_alphabetic() || ch == ' ')
        }) {
            saw_heading = true;
        } else {
            return false;
        }
    }
    saw_heading && (saw_prose || word_items >= 2)
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
