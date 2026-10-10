//! Command-line recognition shared by language detection and semantic kinds.

use crate::text::{complete_line_prefix_by_bytes, TEXT_SAMPLE_MAX_BYTES};

/// Listed executable names shared by detection and semantic command handles.
pub(crate) const COMMANDS: &[&str] = &[
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
        && !has_unquoted_prose_copula(words.get(1..).unwrap_or_default(), false)
        && !words.iter().any(|word| sentence_markers.contains(word))
        && (words.iter().skip(1).any(|word| is_shell_option(word))
            || words.get(1).is_some_and(|word| is_shell_path(word)))
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
        let trimmed = strip_shell_prompt(line.trim_matches('`'));

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let first = *parts.first()?;
        let cmd = first.to_ascii_lowercase();
        let is_regular_command = COMMANDS.iter().any(|known| *known == cmd);
        let is_setup_command = SETUP_COMMANDS.iter().any(|known| *known == cmd);
        if first != cmd || (!is_regular_command && !is_setup_command) {
            return None;
        }

        let arguments = parts.get(1..).unwrap_or_default();
        let has_command_syntax = command_has_shell_syntax(trimmed, arguments);
        let explicit_arguments = command_has_explicit_arguments(&cmd, trimmed, arguments);
        let comment_evidence = command_has_comment_evidence(&cmd, trimmed, arguments);
        if (line.starts_with("> ") && !explicit_arguments)
            || (leading_comment && !comment_evidence && !looks_like_shell_command_sequence(sample))
        {
            return None;
        }

        if is_setup_command {
            let next_command = looks_like_shell_command_sequence(sample)
                || lines
                    .find(|line| !line.starts_with('#'))
                    .is_some_and(starts_with_command_word);
            if !setup_command_is_valid(&cmd, arguments, next_command) {
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
            command_has_shell_syntax(line, arguments.as_slice()),
        );
    }
    if !SETUP_COMMANDS.contains(&command) {
        return false;
    }
    let arguments: Vec<&str> = parts.collect();
    setup_command_is_valid(command, &arguments, false)
}

/// Reject prose uses of a listed executable while retaining command syntax.
///
/// # Arguments
/// - `command`: Listed executable name.
/// - `arguments`: Whitespace-delimited arguments following the executable.
/// - `has_shell_syntax`: Whether options, quoting, or operators supply command evidence.
///
/// # Returns
/// Whether the command passes its Git, recipe, and prose-boundary rules.
pub(crate) fn regular_command_is_valid(
    command: &str,
    arguments: &[&str],
    has_shell_syntax: bool,
) -> bool {
    if command == "git"
        && !has_shell_syntax
        && !arguments
            .first()
            .is_some_and(|subcommand| SHELL_GIT_SUBCOMMANDS.contains(subcommand))
    {
        return false;
    }
    if matches!(command, "make" | "just") && ambiguous_recipe_is_prose(arguments) {
        // A quoted operand may follow other targets. An apostrophe inside
        // `it's` is prose punctuation, not an opening shell quote.
        let quoted_operand = arguments.iter().enumerate().any(|(index, word)| {
            ['\'', '"'].iter().any(|quote| {
                word.starts_with(*quote)
                    && arguments
                        .iter()
                        .skip(index)
                        .any(|part| part.ends_with(*quote))
            })
        });
        if !quoted_operand
            && !command_has_explicit_arguments(command, &arguments.join(" "), arguments)
        {
            return false;
        }
    }
    // Unquoted copulas near the verb are prose evidence (`echo chamber is`,
    // `sudo is required`). Quoting, options, and shell syntax supply command
    // evidence even when the argument text contains those words.
    !has_unquoted_prose_copula(arguments, has_shell_syntax)
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
            "the", "this", "to", "us", "we", "you", "your", "yourself", "before", "after",
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

/// Git subcommands accepted as command structure without extra shell syntax.
pub(crate) const SHELL_GIT_SUBCOMMANDS: &[&str] = &[
    "add",
    "bisect",
    "blame",
    "branch",
    "checkout",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "diff",
    "fetch",
    "grep",
    "init",
    "log",
    "merge",
    "mv",
    "pull",
    "push",
    "rebase",
    "remote",
    "reset",
    "restore",
    "revert",
    "rev-parse",
    "rm",
    "show",
    "stash",
    "status",
    "submodule",
    "switch",
    "tag",
    "worktree",
];

/// Recognize argument structure specific to shell setup commands.
///
/// # Arguments
/// - `command`: Leading executable or shell builtin word.
/// - `arguments`: Whitespace-separated arguments following that word.
/// - `next_command`: Whether the immediately following substantive line is a command.
///
/// # Returns
/// Whether arguments form an assignment, shell option, or setup path rather
/// than prose that merely contains punctuation or numbers.
pub(crate) fn setup_command_is_valid(
    command: &str,
    arguments: &[&str],
    next_command: bool,
) -> bool {
    let quoted_argument = arguments.first().is_some_and(|first| {
        ['\'', '"'].iter().any(|quote| {
            first.starts_with(*quote) && arguments.iter().any(|part| part.ends_with(*quote))
        })
    });
    let compact_arguments = |arguments: &[&str]| {
        !arguments.is_empty()
            && !has_unquoted_prose_copula(arguments, false)
            && !arguments
                .iter()
                .any(|part| matches!(*part, "for" | "to" | "the"))
            && arguments.iter().all(|part| {
                part.chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
            })
    };
    let control_operator = arguments
        .iter()
        .enumerate()
        .find(|(_, part)| part.contains(['&', '|', ';']))
        .is_some_and(|(index, part)| {
            let mut prefix: Vec<&str> = arguments.iter().take(index).copied().collect();
            let head = part.split(['&', '|', ';']).next().unwrap_or_default();
            if !head.is_empty() {
                prefix.push(head);
            }
            compact_arguments(&prefix)
        });
    let structured = match command {
        "export" => {
            arguments
                .first()
                .is_some_and(|argument| argument.starts_with('-'))
                || arguments.iter().any(|argument| argument.contains('='))
        }
        "set" => {
            arguments.len() == 1
                || arguments
                    .first()
                    .is_some_and(|argument| argument.starts_with(['-', '+', '/']))
                || arguments.iter().any(|argument| argument.contains('='))
        }
        "cd" => {
            arguments.len() == 1
                || arguments
                    .first()
                    .is_some_and(|path| path.starts_with('-') || path.contains(['/', '\\']))
        }
        "source" => arguments
            .first()
            .is_some_and(|path| path.contains(['.', '/', '\\']) || path.starts_with('~')),
        _ => false,
    };
    structured
        || quoted_argument
        || control_operator
        || (next_command && compact_arguments(arguments))
}

/// Recognize a prose copula near a command-shaped leading word.
///
/// # Arguments
/// - `arguments`: Command arguments following the executable-shaped word.
/// - `has_shell_syntax`: Whether quoting, options, paths, or operators already
///   establish command structure.
///
/// # Returns
/// `true` when the first three arguments contain an unquoted copula without
/// other shell syntax establishing an actual command.
pub(crate) fn has_unquoted_prose_copula(arguments: &[&str], has_shell_syntax: bool) -> bool {
    !has_shell_syntax
        && arguments
            .iter()
            .take(3)
            .any(|part| matches!(*part, "is" | "are" | "was" | "were"))
}

/// Remove an optional leading shell prompt from a command line.
///
/// # Arguments
/// - `line`: Command line after whitespace and backtick trimming.
///
/// # Returns
/// The line without a leading `$ `, `% `, or `> ` prompt.
pub(crate) fn strip_shell_prompt(line: &str) -> &str {
    line.strip_prefix("$ ")
        .or_else(|| line.strip_prefix("% "))
        .or_else(|| line.strip_prefix("> "))
        .unwrap_or(line)
}

/// Recognize punctuation, options, or digits establishing shell command structure.
///
/// # Arguments
/// - `line`: Command line without a leading shell prompt.
/// - `arguments`: Whitespace-separated arguments following the command word.
///
/// # Returns
/// Whether shell syntax supplies command evidence beyond the leading word.
pub(crate) fn command_has_shell_syntax(line: &str, arguments: &[&str]) -> bool {
    line.contains(['\'', '"', '$', '|', '>', '<', '=', '/', '\\', ';', '&'])
        || arguments
            .iter()
            .any(|part| part.starts_with('-') || part.chars().any(|ch| ch.is_ascii_digit()))
}

/// Recognize an option token rather than a separated prose dash.
///
/// # Returns
/// Whether a dash-prefixed token contains an option name or number.
pub(crate) fn is_shell_option(value: &str) -> bool {
    value
        .strip_prefix('-')
        .map(|rest| rest.trim_start_matches('-'))
        .and_then(|rest| rest.chars().next())
        .is_some_and(|ch| ch.is_ascii_alphanumeric())
}

/// Recognize a compact path operand, excluding URLs.
///
/// # Returns
/// Whether the argument names a path rather than a prose URL.
pub(crate) fn is_shell_path(value: &str) -> bool {
    !value.contains("://")
        && (value.contains(['/', '\\'])
            || matches!(value, "." | "..")
            || value
                .strip_prefix('.')
                .and_then(|rest| rest.chars().next())
                .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_'))
}

/// Require executable-specific arguments before treating a quote/comment as a command.
///
/// # Arguments
/// - `command`: Executable name.
/// - `line`: Command line inspected for shell operators and quoting.
/// - `arguments`: Whitespace-delimited operands after the executable.
///
/// # Returns
/// Whether the command has shell structure or a recognized tool action.
pub(crate) fn command_has_explicit_arguments(
    command: &str,
    line: &str,
    arguments: &[&str],
) -> bool {
    let Some(first) = arguments.first() else {
        return false;
    };
    if arguments.iter().any(|argument| is_shell_option(argument))
        || is_shell_path(first)
        || first.contains('$')
        || line.contains(['|', '>', '<', '=', ';', '&'])
        || ['\'', '"'].iter().any(|quote| {
            first.starts_with(*quote) && arguments.iter().any(|part| part.ends_with(*quote))
        })
    {
        return true;
    }
    match command {
        "git" => SHELL_GIT_SUBCOMMANDS.contains(first),
        "echo" | "printf" => true,
        "python" | "pytest" | "torchrun" => first.ends_with(".py"),
        "curl" => first.starts_with("https://") || first.starts_with("http://"),
        "ssh" => first
            .split_once('@')
            .is_some_and(|(user, host)| !user.is_empty() && !host.is_empty()),
        "docker" if *first == "ps" => true,
        "npm" if *first == "i" => true,
        "brew" | "cargo" | "conda" | "docker" | "kubectl" | "npm" | "pip" | "pnpm" | "rustup"
        | "systemctl" | "uv" | "yarn" => [
            "activate",
            "add",
            "build",
            "check",
            "ci",
            "clippy",
            "exec",
            "fmt",
            "get",
            "install",
            "list",
            "remove",
            "restart",
            "run",
            "show",
            "start",
            "status",
            "stop",
            "test",
            "toolchain",
            "uninstall",
            "update",
        ]
        .contains(first),
        _ => false,
    }
}

/// Accept listed executables after comments without enumerating every tool action.
///
/// Quoted lines intentionally continue to require [`command_has_explicit_arguments`]
/// so ordinary blockquotes do not become shell snippets.
///
/// # Arguments
/// - `command`: Executable name.
/// - `line`: Command line without a leading prompt.
/// - `arguments`: Whitespace-delimited operands after the executable.
///
/// # Returns
/// Whether a leading comment plus this command supplies enough technical evidence.
pub(crate) fn command_has_comment_evidence(command: &str, line: &str, arguments: &[&str]) -> bool {
    command_has_explicit_arguments(command, line, arguments)
        || (command != "ls"
            && !has_unquoted_prose_copula(arguments, false)
            && !arguments.iter().any(|word| {
                [
                    "a", "an", "the", "for", "to", "about", "please", "should", "could", "would",
                    "we", "you", "our", "your", "in", "at", "by",
                ]
                .contains(word)
            }))
}

/// Recognize a compact sequence of shell setup and executable command lines.
///
/// # Returns
/// `true` when every non-comment line is a recognized command and at least two
/// command lines are present.
pub(crate) fn looks_like_shell_command_sequence(content: &str) -> bool {
    let sample = complete_line_prefix_by_bytes(content.trim(), TEXT_SAMPLE_MAX_BYTES);
    if looks_like_shell_for_loop(sample) {
        return true;
    }
    let has_leading_comment = sample.trim_start().starts_with('#');
    let has_leading_prompt = sample.trim_start().starts_with(['$', '%', '>']);
    let mut commands = 0usize;
    let mut next_command = false;
    let mut explicit_single_command = false;
    for line in sample
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if line.starts_with('#') {
            continue;
        }
        let line = line.trim_matches('`');
        let explicit_prompt = line.starts_with("$ ") || line.starts_with("% ");
        let quoted = line.starts_with("> ");
        let line = strip_shell_prompt(line);
        let mut parts = line.split_whitespace();
        let Some(command) = parts.next() else {
            return false;
        };
        if command != command.to_ascii_lowercase() {
            return false;
        }
        let arguments: Vec<&str> = parts.collect();
        let has_shell_syntax = command_has_shell_syntax(line, &arguments);
        let explicit_arguments = command_has_explicit_arguments(command, line, &arguments);
        let comment_evidence = command_has_comment_evidence(command, line, &arguments);
        if quoted && !explicit_arguments {
            return false;
        }
        let command_line = if COMMANDS.contains(&command) {
            regular_command_is_valid(command, &arguments, has_shell_syntax)
        } else if SETUP_COMMANDS.contains(&command) {
            setup_command_is_valid(command, &arguments, next_command)
        } else {
            false
        };
        if !command_line {
            return false;
        }
        commands = commands.saturating_add(1);
        explicit_single_command = explicit_prompt || explicit_arguments || comment_evidence;
        next_command = command_line;
    }
    commands >= 2
        || (commands == 1 && (has_leading_comment || has_leading_prompt) && explicit_single_command)
}

fn looks_like_shell_for_loop(sample: &str) -> bool {
    let mut lines = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let Some(first) = lines.next().and_then(|line| line.strip_prefix("for ")) else {
        return false;
    };
    let Some((name, values)) = first.split_once(" in ") else {
        return false;
    };
    let name = name.trim();
    if !name
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        || values.trim().is_empty()
    {
        return false;
    }
    let mut has_do = values.contains("; do");
    let mut last = first;
    for line in lines {
        has_do |= line == "do" || line.starts_with("do ");
        last = line;
    }
    has_do && (last == "done" || last.ends_with("; done"))
}
