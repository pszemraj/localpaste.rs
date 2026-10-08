//! Command-shaped semantic handles and their prose disambiguation.

const COMMANDS: &[&str] = &[
    "brew",
    "cargo",
    "git",
    "docker",
    "kubectl",
    "python",
    "pytest",
    "uv",
    "pip",
    "npm",
    "pnpm",
    "yarn",
    "make",
    "just",
    "curl",
    "wget",
    "ssh",
    "torchrun",
    "ls",
    "sudo",
    "echo",
    "printf",
    "systemctl",
    "conda",
    "mkdir",
    "rustup",
];
const SETUP_COMMANDS: &[&str] = &["cd", "export", "source", "set"];

/// Command-shape evidence for tools outside the executable handle list.
///
/// # Returns
/// Whether the line has prompt, assignment, flag, or path structure rather than prose.
pub(super) fn has_unlisted_command_syntax(line: &str) -> bool {
    let line = line.trim();
    let line = line.strip_prefix("> ").unwrap_or(line);
    let words: Vec<_> = line.split_whitespace().collect();
    if (matches!(words.first().copied(), Some("$" | "%"))
        && words.get(1).is_some_and(|word| {
            word.chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        }))
        || words.first().is_some_and(|word| {
            word.contains('=')
                && word.split('=').next().is_some_and(|name| {
                    !name.is_empty()
                        && name
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                })
        })
    {
        return true;
    }
    let sentence_markers = [
        "a", "an", "the", "for", "to", "about", "please", "should", "could", "would", "we", "you",
        "our", "your", "in", "at", "by",
    ];
    let leading_executable = words.first().is_some_and(|word| {
        !word.is_empty()
            && word
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || matches!(ch, '_' | '-'))
    });
    leading_executable
        && !crate::detection::has_unquoted_prose_copula(words.get(1..).unwrap_or_default(), false)
        && !words.iter().any(|word| sentence_markers.contains(word))
        && (words
            .iter()
            .skip(1)
            .any(|word| crate::detection::is_shell_option(word))
            || words
                .get(1)
                .is_some_and(|word| crate::detection::is_shell_path(word)))
}

/// Extract a compact handle from a leading command-shaped line.
///
/// # Returns
/// A command and optional subcommand handle when the leading line has enough
/// command structure and is not grammatical prose.
pub(super) fn extract_command_handle(sample: &str) -> Option<String> {
    // Later command-shaped lines may be explanations or quoted email content.
    // A command must lead the paste and use an executable's case-sensitive name.
    let mut lines = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .peekable();
    let leading_comment = lines.peek().is_some_and(|line| line.starts_with('#'));
    while lines.peek().is_some_and(|line| line.starts_with('#')) {
        let _ = lines.next();
    }
    if let Some(line) = lines.next() {
        let trimmed = crate::detection::strip_shell_prompt(line.trim_matches('`'));

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let first = *parts.first()?;
        let cmd = first.to_ascii_lowercase();
        let is_regular_command = COMMANDS.iter().any(|known| *known == cmd);
        let is_setup_command = SETUP_COMMANDS.iter().any(|known| *known == cmd);
        if first != cmd || (!is_regular_command && !is_setup_command) {
            return None;
        }

        let arguments = parts.get(1..).unwrap_or_default();
        let has_command_syntax = crate::detection::command_has_shell_syntax(trimmed, arguments);
        let explicit_arguments =
            crate::detection::command_has_explicit_arguments(&cmd, trimmed, arguments);
        if (line.starts_with("> ") && !explicit_arguments)
            || (leading_comment
                && !explicit_arguments
                && !crate::detection::looks_like_shell_command_sequence(sample))
        {
            return None;
        }

        if is_setup_command {
            let next_command = crate::detection::looks_like_shell_command_sequence(sample)
                || lines
                    .find(|line| !line.starts_with('#'))
                    .is_some_and(starts_with_command_word);
            if !crate::detection::setup_command_is_valid(&cmd, arguments, next_command) {
                return None;
            }
            if !has_command_syntax && !next_command {
                return None;
            }
        }

        if is_regular_command
            && !regular_command_is_valid(cmd.as_str(), arguments, has_command_syntax)
        {
            return None;
        }

        let sub = parts
            .get(1)
            .copied()
            .map(super::clean_atom)
            .filter(|value| !value.is_empty() && !value.starts_with('-'));

        return Some(match sub {
            Some(sub) => format!("{} {}", cmd, sub),
            None => cmd,
        });
    }

    None
}

fn starts_with_command_word(line: &str) -> bool {
    let line = line.trim().trim_matches('`');
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return false;
    };
    if command != command.to_ascii_lowercase() {
        return false;
    }
    if COMMANDS.contains(&command) {
        let arguments: Vec<&str> = parts.collect();
        return regular_command_is_valid(
            command,
            arguments.as_slice(),
            crate::detection::command_has_shell_syntax(line, arguments.as_slice()),
        );
    }
    if !SETUP_COMMANDS.contains(&command) {
        return false;
    }
    let arguments: Vec<&str> = parts.collect();
    crate::detection::setup_command_is_valid(command, &arguments, false)
}

fn regular_command_is_valid(command: &str, arguments: &[&str], has_shell_syntax: bool) -> bool {
    if command == "git"
        && !has_shell_syntax
        && !arguments
            .first()
            .is_some_and(|subcommand| crate::detection::SHELL_GIT_SUBCOMMANDS.contains(subcommand))
    {
        return false;
    }
    if matches!(command, "make" | "just")
        && !has_shell_syntax
        && ambiguous_recipe_is_prose(arguments)
    {
        return false;
    }
    // Unquoted copulas near the verb are prose evidence (`echo chamber is`,
    // `sudo is required`). Quoting, options, and shell syntax supply command
    // evidence even when the argument text contains those words.
    !crate::detection::has_unquoted_prose_copula(arguments, has_shell_syntax)
}

fn ambiguous_recipe_is_prose(arguments: &[&str]) -> bool {
    let words: Vec<String> = arguments
        .iter()
        .map(|word| {
            word.trim_matches(|ch: char| !ch.is_ascii_alphabetic())
                .to_ascii_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect();
    let Some(predicate) = words.first() else {
        return false;
    };
    let grammatical_marker = |word: &str| {
        [
            "a", "an", "at", "for", "i", "in", "me", "my", "of", "on", "our", "please", "that",
            "the", "this", "to", "us", "we", "you", "your", "yourself",
        ]
        .contains(&word)
    };
    if grammatical_marker(predicate) {
        return true;
    }
    let later_markers = words
        .iter()
        .skip(1)
        .filter(|word| grammatical_marker(word))
        .count();
    let predicate_has_sentence_shape = predicate.ends_with("ed")
        || predicate.ends_with("ing")
        || matches!(predicate.as_str(), "remember" | "sure");
    later_markers >= 2 || (predicate_has_sentence_shape && later_markers >= 1)
}
