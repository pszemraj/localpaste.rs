//! Command-line recognition shared by language detection and semantic kinds.
//!
//! A line is a command when it leads with a listed executable and none of its
//! unquoted words read as English grammar (see [`has_prose_grammar`]). Options,
//! paths, quotes, and operators establish command structure, but they never
//! outweigh prose evidence: sentences mention flags, paths, and numbers too.

use crate::text::{complete_line_prefix_by_bytes, TEXT_SAMPLE_MAX_BYTES};

/// Shell control blocks inside command sequences.
mod control;
use control::shell_control_lines;

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
    "cp",
    "mv",
    "rm",
    "grep",
    "rg",
    "tar",
    "rsync",
    "scp",
    "chmod",
    "chown",
    "sed",
    "awk",
    "xargs",
    "ffmpeg",
    "jq",
    "gh",
    "helm",
    "terraform",
];

/// Shell builtins that prepare the session for the commands that follow.
const SETUP_COMMANDS: &[&str] = &["cd", "export", "source", "set"];

/// Git's own subcommands, which a bare `git` line must name to be a command.
pub(crate) const GIT_SUBCOMMANDS: &[&str] = &[
    "add",
    "am",
    "annotate",
    "apply",
    "archive",
    "bisect",
    "blame",
    "branch",
    "bundle",
    "cat-file",
    "check-attr",
    "check-ignore",
    "checkout",
    "cherry",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "config",
    "count-objects",
    "credential",
    "describe",
    "diff",
    "difftool",
    "fetch",
    "filter-branch",
    "format-patch",
    "fsck",
    "gc",
    "grep",
    "hash-object",
    "help",
    "init",
    "lfs",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "maintenance",
    "merge",
    "merge-base",
    "mergetool",
    "mv",
    "notes",
    "prune",
    "pull",
    "push",
    "range-diff",
    "rebase",
    "reflog",
    "remote",
    "repack",
    "replace",
    "rerere",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "shortlog",
    "show",
    "show-ref",
    "sparse-checkout",
    "stash",
    "status",
    "submodule",
    "switch",
    "symbolic-ref",
    "tag",
    "update-index",
    "update-ref",
    "verify-commit",
    "verify-tag",
    "version",
    "whatchanged",
    "worktree",
];

/// Closed-class English words: articles, pronouns, prepositions, auxiliaries,
/// conjunctions, and question words. None of them is an operand of the listed
/// tools, so one unquoted occurrence marks a sentence that merely starts with a
/// tool name, such as `cargo build takes forever on my laptop`. The pronoun `I`
/// matches only in uppercase (`npm i` installs) and `am` is absent (`git am`).
const FUNCTION_WORDS: &[&str] = &[
    "a",
    "an",
    "the",
    "this",
    "that",
    "these",
    "those",
    "some",
    "any",
    "every",
    "my",
    "your",
    "our",
    "their",
    "his",
    "her",
    "its",
    "me",
    "you",
    "we",
    "us",
    "they",
    "them",
    "he",
    "him",
    "she",
    "it",
    "someone",
    "anyone",
    "everyone",
    "nothing",
    "something",
    "everything",
    "myself",
    "yourself",
    "itself",
    "about",
    "after",
    "at",
    "before",
    "behind",
    "between",
    "by",
    "during",
    "for",
    "from",
    "in",
    "into",
    "of",
    "on",
    "onto",
    "over",
    "per",
    "since",
    "through",
    "to",
    "under",
    "until",
    "upon",
    "with",
    "within",
    "without",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "has",
    "have",
    "had",
    "do",
    "does",
    "did",
    "can",
    "could",
    "will",
    "would",
    "shall",
    "should",
    "may",
    "might",
    "must",
    "not",
    "and",
    "or",
    "but",
    "because",
    "so",
    "if",
    "when",
    "while",
    "than",
    "then",
    "what",
    "why",
    "how",
    "who",
    "which",
    "where",
    "please",
];

/// Recognize English grammar among the unquoted words of a would-be command.
///
/// Prose evidence is closed-class: a function word, a contraction or other
/// mid-word apostrophe, a typographic quote, or sentence-ending punctuation
/// attached to a word (`ideas?`, `ERR!`, `tomorrow.`). Commas are not evidence
/// on their own because operand lists use them too. Quoted text and a trailing
/// `#` comment are skipped, so `echo 'this is a test'` stays a command.
///
/// # Arguments
/// - `text`: Command arguments, or a whole line, to inspect.
///
/// # Returns
/// Whether any unquoted word reads as part of a sentence.
pub(crate) fn has_prose_grammar(text: &str) -> bool {
    let mut chars = text.chars();
    let mut word = String::new();
    let mut quoted = false;
    loop {
        let ch = chars.next();
        match ch {
            None | Some(' ' | '\t' | '\r' | '\n') => {
                if !quoted && is_prose_word(&word) {
                    return true;
                }
                word.clear();
                quoted = false;
                if ch.is_none() {
                    return false;
                }
            }
            Some('#') if word.is_empty() && !quoted => return false,
            Some('’' | '‘' | '“' | '”') => return true,
            Some('\\') => {
                let _ = chars.next();
                word.push('_');
            }
            Some(quote @ ('\'' | '"')) => {
                // A shell never opens a quote mid-word in `it's` or `twitter's`;
                // only prose puts an apostrophe after letters.
                if quote == '\'' && !word.is_empty() && word.chars().all(char::is_alphabetic) {
                    return true;
                }
                quoted = true;
                loop {
                    match chars.next() {
                        // A quote continuing past this text is a multi-line string.
                        None => return false,
                        Some(close) if close == quote => break,
                        Some('\\') if quote == '"' => {
                            let _ = chars.next();
                        }
                        Some(_) => {}
                    }
                }
            }
            Some(other) => word.push(other),
        }
    }
}

fn is_prose_word(word: &str) -> bool {
    let bare = word.trim_end_matches([',', ';', ':']);
    let stem = bare.trim_end_matches(['?', '!', '.']);
    // Sentences end in one dot or an ellipsis; two dots are a Git range
    // such as `main..`.
    let punctuated = stem.len() < bare.len() && &bare[stem.len()..] != "..";
    !stem.is_empty()
        && (stem == "I"
            || FUNCTION_WORDS.contains(&stem.to_lowercase().as_str())
            || (punctuated && stem.chars().all(char::is_alphabetic)))
}

/// Apply the prose check to a command's operands.
///
/// CMD's `set name=value` takes the rest of the line as the value, so an
/// assignment on a `set` line is structure regardless of the words around it.
/// An `echo` or `printf` message is printed text, so its words are not prose
/// evidence when it opens as a message of its own (see [`message_stands_alone`]).
fn operands_read_as_prose(command: &str, arguments: &[&str]) -> bool {
    let text = arguments.join(" ");
    let assignment = command == "set" && text.contains('=');
    let message = matches!(command, "echo" | "printf") && message_stands_alone(arguments);
    !(assignment || message) && has_prose_grammar(&text)
}

/// Recognize an `echo` or `printf` message that starts a sentence of its own.
///
/// A printed message opens with a capital letter (`echo Done deploying the
/// app`, `echo It's done`), a quote, or a `$` expansion. When the word after
/// `echo` continues in lowercase (`echo chamber is ...`, `echo what Sam
/// said`), or is the pronoun `I`, `echo` is part of an English sentence.
fn message_stands_alone(arguments: &[&str]) -> bool {
    arguments
        .iter()
        .find(|argument| !argument.starts_with('-'))
        .is_some_and(|first| {
            first.starts_with(['\'', '"', '$'])
                || (first.starts_with(char::is_uppercase)
                    && first.split(['\'', '’']).next() != Some("I"))
        })
}

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
    let leading_executable = words.first().is_some_and(|word| {
        !word.is_empty()
            && word
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || matches!(ch, '_' | '-'))
    });
    leading_executable
        && !has_prose_grammar(&words.get(1..).unwrap_or_default().join(" "))
        && (words.iter().skip(1).any(|word| is_shell_option(word))
            || words.get(1).is_some_and(|word| is_shell_path(word)))
}

/// Extract a compact handle from a body led by a command.
///
/// The command must lead the paste (after optional `#` comments) and use an
/// executable's case-sensitive name. A body whose later lines include a prose
/// sentence is a note that quotes a command, so it gets no command handle,
/// unless a `$ ` or `% ` prompt marks the paste as a terminal transcript.
///
/// # Returns
/// A command and optional subcommand handle when the leading line has enough
/// command structure and no line reads as grammatical prose.
pub(super) fn extract_command_handle(sample: &str) -> Option<String> {
    let mut lines = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .peekable();
    let leading_comment = lines.peek().is_some_and(|line| line.starts_with('#'));
    while lines.peek().is_some_and(|line| line.starts_with('#')) {
        let _ = lines.next();
    }
    let line = lines.next()?;
    let trimmed = strip_shell_prompt(line.trim_matches('`'));
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let command = *parts.first()?;
    let is_regular_command = COMMANDS.contains(&command);
    let is_setup_command = SETUP_COMMANDS.contains(&command);
    if !is_regular_command && !is_setup_command {
        return None;
    }

    let arguments = parts.get(1..).unwrap_or_default();
    let has_command_syntax = command_has_shell_syntax(trimmed, arguments);
    let explicit_arguments = command_has_explicit_arguments(command, trimmed, arguments);
    if (line.starts_with("> ") && !explicit_arguments)
        || (leading_comment
            && !command_has_comment_evidence(command, explicit_arguments)
            && !looks_like_shell_command_sequence(sample))
    {
        return None;
    }

    let rest: Vec<&str> = lines.filter(|line| !line.starts_with('#')).collect();
    let script = script_anchor(std::iter::once(trimmed).chain(rest.iter().copied()));
    let valid = if is_setup_command {
        let next_command = looks_like_shell_command_sequence(sample)
            || rest
                .first()
                .is_some_and(|line| line_is_command(line, false, script));
        setup_command_is_valid(command, arguments, next_command)
            && (has_command_syntax || next_command)
    } else {
        regular_command_is_valid(command, arguments, has_command_syntax, script)
    };
    // A prompt marks a terminal transcript, whose later lines are output.
    // Otherwise a later prose sentence marks a note that quotes the command;
    // command lines and shell control lines are not prose, whatever their words.
    let transcript = line.starts_with("$ ") || line.starts_with("% ");
    let control = shell_control_lines(&rest, script);
    if !valid
        || (!transcript
            && rest.iter().enumerate().any(|(index, line)| {
                control.as_ref().and_then(|kinds| kinds.get(index)) != Some(&true)
                    && !line_is_command(line, true, script)
                    && has_prose_grammar(line)
                    && super::looks_like_prose(line)
            }))
    {
        return None;
    }

    let sub = parts
        .get(1)
        .copied()
        .map(super::clean_atom)
        .filter(|value| !value.is_empty() && !value.starts_with('-'));
    Some(match sub {
        Some(sub) => format!("{command} {sub}"),
        None => command.to_string(),
    })
}

/// Validate one line as a listed or setup command.
///
/// # Arguments
/// - `line`: Candidate line, optionally prompt-prefixed or backtick-wrapped.
/// - `next_command`: Whether the following substantive line is a command, which
///   lets plain setup operands such as `cd repo` count.
/// - `script`: Whether the surrounding lines form a script (see [`script_anchor`]).
fn line_is_command(line: &str, next_command: bool, script: bool) -> bool {
    let line = strip_shell_prompt(line.trim().trim_matches('`'));
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return false;
    };
    let arguments: Vec<&str> = parts.collect();
    if COMMANDS.contains(&command) {
        regular_command_is_valid(
            command,
            &arguments,
            command_has_shell_syntax(line, &arguments),
            script,
        )
    } else if SETUP_COMMANDS.contains(&command) {
        setup_command_is_valid(command, &arguments, next_command)
    } else {
        false
    }
}

/// Reject prose uses of a listed executable and Git lines without a subcommand.
///
/// In a script, `echo` and `printf` print whatever words they are given, as
/// Makefile recipes do, so their operands face no prose check.
fn regular_command_is_valid(
    command: &str,
    arguments: &[&str],
    has_shell_syntax: bool,
    script: bool,
) -> bool {
    ((script && matches!(command, "echo" | "printf"))
        || !operands_read_as_prose(command, arguments))
        && (command != "git"
            || has_shell_syntax
            || arguments
                .first()
                .is_some_and(|subcommand| GIT_SUBCOMMANDS.contains(subcommand)))
}

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
pub(super) fn setup_command_is_valid(
    command: &str,
    arguments: &[&str],
    next_command: bool,
) -> bool {
    if operands_read_as_prose(command, arguments) {
        return false;
    }
    let quoted_argument = arguments.first().is_some_and(|first| {
        ['\'', '"'].iter().any(|quote| {
            first.starts_with(*quote) && arguments.iter().any(|part| part.ends_with(*quote))
        })
    });
    let compact_arguments = |arguments: &[&str]| {
        !arguments.is_empty()
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

/// Remove an optional leading shell prompt from a command line.
///
/// # Arguments
/// - `line`: Command line after whitespace and backtick trimming.
///
/// # Returns
/// The line without a leading `$ `, `% `, or `> ` prompt.
fn strip_shell_prompt(line: &str) -> &str {
    line.strip_prefix("$ ")
        .or_else(|| line.strip_prefix("% "))
        .or_else(|| line.strip_prefix("> "))
        .unwrap_or(line)
}

/// Recognize quoting, operators, paths, or options establishing shell structure.
///
/// # Arguments
/// - `line`: Command line without a leading shell prompt.
/// - `arguments`: Whitespace-separated arguments following the command word.
///
/// # Returns
/// Whether shell syntax supplies command evidence beyond the leading word.
fn command_has_shell_syntax(line: &str, arguments: &[&str]) -> bool {
    line.contains(['\'', '"', '$', '|', '>', '<', '=', '/', '\\', ';', '&'])
        || arguments.iter().any(|part| part.starts_with('-'))
}

/// Recognize an option token rather than a separated prose dash.
///
/// # Returns
/// Whether a dash-prefixed token contains an option name or number.
fn is_shell_option(value: &str) -> bool {
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
pub(super) fn is_shell_path(value: &str) -> bool {
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
pub(super) fn command_has_explicit_arguments(
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
        "git" => GIT_SUBCOMMANDS.contains(first),
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
/// The prose check has already run, so any listed command except a bare `ls`
/// (a common to-do word) is technical once a comment introduces it. Quoted lines
/// continue to require [`command_has_explicit_arguments`] so ordinary
/// blockquotes do not become shell snippets.
fn command_has_comment_evidence(command: &str, explicit_arguments: bool) -> bool {
    explicit_arguments || command != "ls"
}

/// Recognize a compact sequence of shell setup and executable command lines.
///
/// Shell control lines (`for host in a b; do`, `if [ -f x ]; then`, `done`)
/// count as command lines when their blocks open and close in order (see
/// [`shell_control_lines`]).
///
/// # Returns
/// `true` when every non-comment line is a recognized command or control line
/// and at least two such lines are present.
pub(crate) fn looks_like_shell_command_sequence(content: &str) -> bool {
    let sample = complete_line_prefix_by_bytes(content.trim(), TEXT_SAMPLE_MAX_BYTES);
    if looks_like_shell_for_loop(sample) {
        return true;
    }
    let has_leading_comment = sample.trim_start().starts_with('#');
    let has_leading_prompt = sample.trim_start().starts_with(['$', '%', '>']);
    let lines: Vec<&str> = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let script = script_anchor(lines.iter().copied());
    let Some(control) = shell_control_lines(&lines, script) else {
        return false;
    };
    let mut commands = 0usize;
    let mut next_command = false;
    let mut explicit_single_command = false;
    for (line, is_control) in lines.iter().zip(control).rev() {
        if is_control {
            commands = commands.saturating_add(1);
            next_command = true;
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
        let arguments: Vec<&str> = parts.collect();
        let explicit_arguments = command_has_explicit_arguments(command, line, &arguments);
        if (quoted && !explicit_arguments) || !line_is_command(line, next_command, script) {
            return false;
        }
        commands = commands.saturating_add(1);
        explicit_single_command =
            explicit_prompt || command_has_comment_evidence(command, explicit_arguments);
        next_command = true;
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

/// Recognize a script: lines among which a listed command other than `echo`
/// or `printf` stands on its own, as a Makefile target anchors its recipes.
/// Setup builtins do not count, so `cd app` followed by `echo chamber is a
/// common expression` stays a note.
fn script_anchor<'a>(lines: impl IntoIterator<Item = &'a str>) -> bool {
    lines.into_iter().any(|line| {
        let line = strip_shell_prompt(line.trim().trim_matches('`'));
        let command = line.split_whitespace().next().unwrap_or_default();
        COMMANDS.contains(&command)
            && !matches!(command, "echo" | "printf")
            && line_is_command(line, false, false)
    })
}
