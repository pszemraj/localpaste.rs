//! Runtime-output recognition: level headers, panics, and tracebacks.

/// Parse a leading log header while excluding record and TSV header shapes.
///
/// Spaced levels must be uppercase or bracketed. Lowercase words with spaces
/// introduce ordinary prose; colon-delimited lowercase levels remain supported.
pub(super) fn leading_log_header(sample: &str, language: &str) -> Option<ParsedLogHeader> {
    let first_line = sample
        .lines()
        .map(str::trim_start)
        .find(|line| !line.is_empty())?;
    let header = parse_log_header(first_line, false)?;
    if super::looks_like_delimited_records(sample) && !header.contextual {
        return None;
    }
    // A detector-provided TSV label plus a tab is sufficient to identify a
    // one-row header. Without this guard, an `INFO`/`ERROR` header is mistaken
    // for a spaced runtime level before stored-language precedence can apply.
    // An explicit parsed context such as `(main)` is log structure rather than
    // a typed table column and may pass the guard.
    if language == "tsv" && first_line.contains('\t') && !header.contextual {
        return None;
    }
    Some(header)
}

/// Structural facts about one level-prefixed log line.
#[derive(Clone, Copy)]
pub(super) struct ParsedLogHeader {
    /// Bracketed or uppercase spaced levels, which identify a log on their own.
    pub(super) strong_single_line: bool,
    machine_message: bool,
    contextual: bool,
}

pub(super) fn parse_log_header(
    line: &str,
    allow_lowercase_spaced: bool,
) -> Option<ParsedLogHeader> {
    const LEVELS: &[&str] = &[
        "trace", "debug", "info", "warn", "warning", "error", "fatal", "success",
    ];

    let line = line.trim_start();
    let (level, mut message, bracketed, colon) =
        if let Some(bracketed_line) = line.strip_prefix('[') {
            let (level, message) = bracketed_line.split_once(']')?;
            (level, message, true, false)
        } else {
            let level = line.split([':', ' ', '\t']).next().unwrap_or("");
            let rest = line.get(level.len()..)?;
            let colon = rest.starts_with(':');
            let message = if colon { rest.get(1..)? } else { rest };
            (level, message, false, colon)
        };
    let lower = level.to_ascii_lowercase();
    if !LEVELS.contains(&lower.as_str())
        || (level != lower && level != level.to_ascii_uppercase())
        || (!allow_lowercase_spaced && !bracketed && !colon && level != level.to_ascii_uppercase())
    {
        return None;
    }

    message = message.trim_start();
    let contextual = message.starts_with('(');
    if let Some(context) = message.strip_prefix('(') {
        let (name, rest) = context.split_once(')')?;
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        {
            return None;
        }
        message = rest.trim_start();
    }
    if message.is_empty() || message.starts_with(['=', ':', '{']) {
        return None;
    }

    Some(ParsedLogHeader {
        strong_single_line: bracketed || (!colon && level == level.to_ascii_uppercase()),
        machine_message: message.chars().next().is_some_and(char::is_uppercase),
        contextual,
    })
}

pub(super) fn looks_like_multiline_log(sample: &str) -> bool {
    let mut headers = 0usize;
    let mut strong_header = false;
    let mut machine_messages = 0usize;
    let mut header_run_started = false;
    let mut yarn_preamble = false;
    for line in sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if !header_run_started && line.starts_with("yarn ") {
            yarn_preamble = true;
            continue;
        }
        if !header_run_started
            && yarn_preamble
            && line.starts_with('[')
            && line.contains('/')
            && line.contains(']')
        {
            continue;
        }
        if let Some(header) = parse_log_header(line, true) {
            header_run_started = true;
            headers = headers.saturating_add(1);
            strong_header |= header.strong_single_line;
            machine_messages = machine_messages.saturating_add(usize::from(header.machine_message));
        } else {
            break;
        }
    }
    headers >= 2 && (strong_header || yarn_preamble || machine_messages == headers)
}

/// Recognize Rust's runtime panic header, rather than prose mentioning a panic.
///
/// # Returns
/// Whether the runtime's thread and source-location prefix appears first or
/// follows only a recognized Cargo build/run preamble.
pub(crate) fn looks_like_rust_panic(content: &str) -> bool {
    for line in content.lines().take(64).map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if line.starts_with("thread '") && line.contains("' panicked at ") {
            return true;
        }
        if !is_cargo_run_preamble(line) {
            return false;
        }
    }
    false
}

/// Recognize a complete Python traceback with a frame and final exception row.
///
/// # Returns
/// Whether runtime structure occupies the sampled body, including quoted exception rows.
pub(crate) fn looks_like_python_traceback(content: &str) -> bool {
    let sample = crate::text::complete_line_prefix_by_bytes(
        content.trim(),
        crate::text::TEXT_SAMPLE_MAX_BYTES,
    );
    let mut lines = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_prefix("> ").unwrap_or(line).trim_start());
    let first = lines.next().unwrap_or_default();
    let first = if matches!(first, "stderr:" | "stdout:") {
        lines.next().unwrap_or_default()
    } else {
        first
    };
    if first != "Traceback (most recent call last):" {
        return false;
    }
    let mut frame = false;
    let mut last = "";
    for line in lines {
        frame |= line.starts_with("File \"") && line.contains("\", line ");
        last = line;
    }
    let name = last.split_once(':').map_or(last, |(name, _)| name);
    frame
        && name.rsplit('.').next().is_some_and(|class| {
            class
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
        })
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.'))
}

/// Accept Cargo's own build/run status and an entered `cargo run` command before
/// a panic header. Any source, Markdown, or prose line ends the runtime preamble.
fn is_cargo_run_preamble(line: &str) -> bool {
    if line.strip_prefix("Compiling ").is_some_and(|status| {
        status
            .split_whitespace()
            .any(|part| part.starts_with('v') && part.get(1..).is_some_and(|v| v.contains('.')))
    }) || line
        .strip_prefix("Finished ")
        .is_some_and(|status| status.contains('`') && status.contains(" profile"))
        || line.strip_prefix("Running ").is_some_and(|status| {
            let status = status.trim();
            status.starts_with('`') && status.get(1..).is_some_and(|rest| rest.contains('`'))
        })
    {
        return true;
    }

    let command = line
        .strip_prefix('$')
        .or_else(|| line.strip_prefix('%'))
        .or_else(|| line.strip_prefix('>'))
        .map(str::trim_start)
        .unwrap_or(line);
    command == "cargo run" || command.starts_with("cargo run ")
}
