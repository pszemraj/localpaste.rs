//! Runtime-output recognition: level headers, timestamps, panics, and traces.
//!
//! Logs are recognized by line structure, never by vocabulary, so prose that
//! mentions a panic, a stack trace, or an error budget stays prose.

/// Level words accepted at the start of a log line.
const LEVELS: &[&str] = &[
    "trace", "debug", "info", "warn", "warning", "error", "fatal", "success", "panic",
];

/// npm's own log prefixes: `npm ERR!`, `npm WARN`, `npm notice`, and the lowercase
/// levels npm 10 prints.
const NPM_LEVELS: &[&str] = &[
    "ERR!", "WARN", "notice", "verb", "http", "timing", "sill", "error", "warn",
];

/// Cargo's right-aligned status verbs, which precede a panic in `cargo run` output.
const CARGO_STATUS_VERBS: &[&str] = &[
    "Adding",
    "Blocking",
    "Checking",
    "Compiling",
    "Downloaded",
    "Downloading",
    "Fetching",
    "Finished",
    "Fresh",
    "Locking",
    "Running",
    "Updating",
];

const MONTHS: &[&str] = &[
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Recognize a body that is runtime output by its line structure.
///
/// # Arguments
/// - `content`: Paste body; only its bounded semantic sample is inspected.
///
/// # Returns
/// Whether the sample opens with a run of level-prefixed lines, consists mostly
/// of timestamped lines, or is a Rust panic, crash output (a Python traceback
/// or a fatal signal or exit report), Java stack trace, or rustc diagnostic.
pub(crate) fn looks_like_runtime_log(content: &str) -> bool {
    let sample = super::sample_prefix(content);
    looks_like_multiline_log(sample)
        || looks_like_timestamped_log(sample)
        || looks_like_rust_panic(sample)
        || looks_like_crash_output(sample)
        || looks_like_java_stack_trace(sample)
        || looks_like_rustc_diagnostic(sample)
}

/// Parse a leading log header while excluding record, TSV, and list shapes.
///
/// Spaced levels must be uppercase or bracketed. Lowercase words with spaces
/// introduce ordinary prose; colon-delimited lowercase levels remain supported
/// unless several of them open the body without output evidence (see
/// [`ParsedLogHeader::output_evidence`]), which makes them a glossary, to-do
/// list, or mapping rather than output.
///
/// # Arguments
/// - `sample`: Bounded semantic sample whose first non-empty line is parsed.
/// - `language`: Canonical stored language; a `tsv` label keeps tabbed rows as data.
///
/// # Returns
/// The first line's header facts, or `None` when the body is not log-led.
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
    let run = leading_header_run(sample);
    if !header.strong_single_line && run.headers >= 2 && !run.corroborated {
        return None;
    }
    Some(header)
}

/// Structural facts about one level-prefixed log line.
#[derive(Clone, Copy)]
pub(super) struct ParsedLogHeader {
    /// Bracketed or uppercase spaced levels, which identify a log on their own.
    pub(super) strong_single_line: bool,
    /// An uppercase level, or a message shaped like tool output (a progress
    /// ellipsis, or after a colon a nested `label: ` or backtick code span),
    /// which neither a note written in lowercase level words nor a mapping
    /// keyed by them has.
    output_evidence: bool,
    contextual: bool,
}

/// Parse one log line's level prefix.
///
/// # Arguments
/// - `line`: Candidate line.
/// - `allow_lowercase_spaced`: Whether `info message` counts; such lines need
///   corroboration from neighbouring machine output.
///
/// # Returns
/// The header's structural facts, or `None` when the line has no level prefix.
pub(super) fn parse_log_header(
    line: &str,
    allow_lowercase_spaced: bool,
) -> Option<ParsedLogHeader> {
    let line = line.trim_start();
    if let Some((level, _)) = line
        .strip_prefix("npm ")
        .map(|rest| rest.split_once(' ').unwrap_or((rest, "")))
    {
        return NPM_LEVELS.contains(&level).then_some(ParsedLogHeader {
            strong_single_line: true,
            output_evidence: true,
            contextual: false,
        });
    }
    let (level, mut message, bracketed, colon) =
        if let Some(bracketed_line) = line.strip_prefix('[') {
            let (level, message) = bracketed_line.split_once(']')?;
            (level, message, true, false)
        } else {
            let token = line.split([':', ' ', '\t']).next().unwrap_or("");
            let rest = line.get(token.len()..)?;
            let colon = rest.starts_with(':');
            let message = if colon { rest.get(1..)? } else { rest };
            // rustc prints a diagnostic code after the level: `error[E0308]:`.
            let level = token
                .split_once('[')
                .filter(|(_, code)| {
                    code.strip_suffix(']')
                        .is_some_and(|code| code.chars().all(|ch| ch.is_ascii_alphanumeric()))
                })
                .map_or(token, |(level, _)| level);
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

    // `error: /var/log/error.log` or `info: "Saved."` is a mapping entry whose
    // value is one scalar, so nothing inside it is evidence of output.
    let scalar_value = colon
        && (!message.contains(char::is_whitespace)
            || (message.len() > 1 && message.starts_with('"') && message.ends_with('"')));
    // To-do items and mapping values name paths, versions, and files as often
    // as output does. Neither prints a progress ellipsis, and a mapping value
    // cannot hold a second `label: ` or, in practice, a backtick code span,
    // both of which compiler and VCS diagnostics put after `error:`.
    let output_shape =
        message.contains("...") || (colon && (message.contains(": ") || message.contains('`')));
    Some(ParsedLogHeader {
        strong_single_line: bracketed || (!colon && level == level.to_ascii_uppercase()),
        output_evidence: level == level.to_ascii_uppercase() || (!scalar_value && output_shape),
        contextual,
    })
}

/// Recognize a token people rarely type in notes: a path, a quote or backtick,
/// an `@` or `=` binding, an ellipsis, or a dotted name such as `package.json`.
fn has_machine_token(text: &str) -> bool {
    text.contains(['/', '\\', '"', '`', '@', '='])
        || text.contains("...")
        || text.split_whitespace().any(|word| {
            let word = word.trim_end_matches(|ch: char| !ch.is_ascii_alphanumeric());
            word.char_indices().any(|(index, ch)| {
                ch == '.'
                    && word[..index].ends_with(|prev: char| prev.is_ascii_alphanumeric())
                    && word[index + 1..].starts_with(|next: char| next.is_ascii_alphanumeric())
            })
        })
}

/// The run of level-prefixed lines that opens a sample.
struct HeaderRun {
    headers: usize,
    strong: bool,
    corroborated: bool,
}

fn leading_header_run(sample: &str) -> HeaderRun {
    let mut run = HeaderRun {
        headers: 0,
        strong: false,
        corroborated: false,
    };
    for line in sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        // Yarn prints a version banner and numbered steps around its levels.
        if (run.headers == 0 && line.starts_with("yarn ")) || is_progress_step(line) {
            run.corroborated = true;
            continue;
        }
        let Some(header) = parse_log_header(line, true) else {
            break;
        };
        run.headers = run.headers.saturating_add(1);
        run.strong |= header.strong_single_line;
        run.corroborated |= header.output_evidence;
    }
    run
}

/// Recognize a run of at least two level-prefixed lines that is output.
///
/// Lowercase levels are also ordinary words (`debug the crash in login.rs`)
/// and mapping keys (`info: version 1.2`), so a run of only lowercase levels
/// needs a yarn banner or progress step, or output evidence in some message:
/// a progress ellipsis, or after a colon a nested `label: ` or a backtick code
/// span. Paths, versions, and file names appear in notes and mappings too.
///
/// # Returns
/// Whether the leading header run has two lines and is strong or corroborated.
fn looks_like_multiline_log(sample: &str) -> bool {
    let run = leading_header_run(sample);
    run.headers >= 2 && (run.strong || run.corroborated)
}

fn is_progress_step(line: &str) -> bool {
    line.strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .and_then(|(step, _)| step.split_once('/'))
        .is_some_and(|(done, total)| {
            !done.is_empty()
                && !total.is_empty()
                && done
                    .chars()
                    .chain(total.chars())
                    .all(|ch| ch.is_ascii_digit())
        })
}

/// Recognize logs whose lines start with a timestamp that includes seconds.
///
/// Most unindented lines must carry the timestamp, so a note that mentions a
/// date, a dated diary, or a CSV whose first column is a time does not qualify;
/// indented continuation lines such as stack frames are not counted. A single
/// stamped line needs a level after the timestamp.
fn looks_like_timestamped_log(sample: &str) -> bool {
    let mut rows = 0usize;
    let mut stamped = 0usize;
    let mut leveled = false;
    for line in sample.lines().filter(|line| !line.trim().is_empty()) {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        rows += 1;
        if let Some(end) = timestamp_prefix_len(line) {
            stamped += 1;
            leveled |= parse_log_header(&line[end..], false)
                .is_some_and(|header| header.strong_single_line);
        }
    }
    stamped > 0 && stamped * 2 >= rows && (stamped >= 2 || leveled)
}

/// Byte length of a leading `YYYY-MM-DD[T ]HH:MM:SS` or syslog `Mon DD HH:MM:SS`
/// timestamp, including fractions, a zone, and an enclosing `[`/`]`.
///
/// A comma, tab, or other delimiter right after the timestamp marks a record
/// field rather than a log message, so delimited data never matches. The one
/// exception is the comma that introduces Python's and log4j's milliseconds
/// (`06:00:01,423 INFO`), recognized by its three digits and the message after it.
fn timestamp_prefix_len(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let start = usize::from(bytes.first() == Some(&b'['));
    let mut end = iso_timestamp_end(bytes, start).or_else(|| syslog_timestamp_end(bytes, start))?;
    if start == 1 && bytes.get(end) == Some(&b']') {
        end += 1;
    }
    matches!(bytes.get(end), None | Some(b' ')).then_some(end)
}

fn digits(bytes: &[u8], at: usize, count: usize) -> Option<usize> {
    let run = bytes.get(at..at + count)?;
    run.iter().all(u8::is_ascii_digit).then_some(at + count)
}

fn byte(bytes: &[u8], at: usize, expected: &[u8]) -> Option<usize> {
    bytes
        .get(at)
        .filter(|found| expected.contains(found))
        .map(|_| at + 1)
}

fn clock_end(bytes: &[u8], at: usize) -> Option<usize> {
    let at = digits(bytes, at, 2)?;
    let at = byte(bytes, at, b":")?;
    let at = digits(bytes, at, 2)?;
    let at = byte(bytes, at, b":")?;
    digits(bytes, at, 2)
}

fn iso_timestamp_end(bytes: &[u8], at: usize) -> Option<usize> {
    let at = digits(bytes, at, 4)?;
    let separator = *bytes.get(at)?;
    let at = byte(bytes, at, b"-/")?;
    let at = digits(bytes, at, 2)?;
    let at = byte(bytes, at, &[separator])?;
    let at = digits(bytes, at, 2)?;
    let at = byte(bytes, at, b"T ")?;
    let mut at = clock_end(bytes, at)?;
    if bytes.get(at) == Some(&b'.') && bytes.get(at + 1).is_some_and(u8::is_ascii_digit) {
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    } else if let Some(end) = byte(bytes, at, b",").and_then(|at| digits(bytes, at, 3)) {
        // Python logging and log4j print milliseconds after a comma: exactly
        // three digits, then the message or a closing bracket. Any other
        // comma, including `,42` or `,423` ending a line, is a record field.
        if matches!(bytes.get(end), Some(b' ' | b']')) {
            return Some(end);
        }
    }
    if bytes.get(at) == Some(&b'Z') {
        return Some(at + 1);
    }
    if let Some(zone) = byte(bytes, at, b"+-").and_then(|zone| digits(bytes, zone, 2)) {
        let zone = byte(bytes, zone, b":").unwrap_or(zone);
        if let Some(end) = digits(bytes, zone, 2) {
            return Some(end);
        }
    }
    Some(at)
}

fn syslog_timestamp_end(bytes: &[u8], at: usize) -> Option<usize> {
    let month = bytes.get(at..at + 3)?;
    if !MONTHS.iter().any(|name| name.as_bytes() == month) {
        return None;
    }
    let at = byte(bytes, at + 3, b" ")?;
    let at = if bytes.get(at) == Some(&b' ') {
        digits(bytes, at + 1, 1)?
    } else {
        digits(bytes, at, 2).or_else(|| digits(bytes, at, 1))?
    };
    let at = byte(bytes, at, b" ")?;
    clock_end(bytes, at)
}

/// Recognize Rust's runtime panic header, rather than prose mentioning a panic.
///
/// # Returns
/// Whether the runtime's thread and source-location prefix appears first or
/// follows only a recognized Cargo build/run preamble.
fn looks_like_rust_panic(content: &str) -> bool {
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

/// Recognize output that ends with a process failing: a Python traceback block
/// or a line reporting a fatal signal or exit status, with no commentary or
/// source around it.
///
/// The traceback may start anywhere, after training output, a `$ ` command, or
/// other program lines, and exit or signal reports may follow it. Every other
/// line must be output rather than a note about it (see [`is_commentary`]), so
/// a bug report that quotes a traceback stays a document, and must not be a
/// Python definition or import line
/// ([`crate::detection::looks_like_python_source`]), so a function whose
/// docstring holds a doctest traceback stays source. Frame lines inside the
/// block, such as `import torch`, are part of the traceback.
///
/// # Returns
/// Whether the sample holds a crash anchor and no commentary or source, including quoted rows.
pub(crate) fn looks_like_crash_output(content: &str) -> bool {
    let sample = crate::text::complete_line_prefix_by_bytes(
        content.trim(),
        crate::text::TEXT_SAMPLE_MAX_BYTES,
    );
    let lines: Vec<&str> = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_prefix("> ").unwrap_or(line).trim_start())
        .collect();
    let traceback = traceback_block(&lines);
    if traceback.is_none() && !lines.iter().any(|line| is_termination_line(line)) {
        return false;
    }
    lines.iter().enumerate().all(|(index, line)| {
        traceback
            .as_ref()
            .is_some_and(|block| block.contains(&index))
            || is_termination_line(line)
            || !(is_commentary(line) || crate::detection::looks_like_python_source(line))
    })
}

/// Line span from the first `Traceback (most recent call last):` header to the
/// last exception line, such as `ValueError: boom`, that follows a
/// `File "...", line N` frame. Chained tracebacks and the sentences Python
/// prints between them fall inside the span.
fn traceback_block(lines: &[&str]) -> Option<std::ops::RangeInclusive<usize>> {
    let header = lines
        .iter()
        .position(|line| *line == "Traceback (most recent call last):")?;
    let mut frame = false;
    let mut end = None;
    for (index, line) in lines.iter().enumerate().skip(header + 1) {
        if line.starts_with("File \"") && line.contains("\", line ") {
            frame = true;
        } else if frame && is_exception_line(line) {
            end = Some(index);
        }
    }
    end.map(|end| header..=end)
}

/// Recognize `ValueError`, `ValueError: boom`, or `requests.HTTPError: ...`.
fn is_exception_line(line: &str) -> bool {
    let name = line.split_once(':').map_or(line, |(name, _)| name);
    name.rsplit('.')
        .next()
        .is_some_and(|class| class.starts_with(|ch: char| ch.is_ascii_uppercase()))
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.'))
}

/// Signal descriptions a shell prints when a process dies.
const FATAL_SIGNALS: &[&str] = &[
    "Segmentation fault",
    "Bus error",
    "Illegal instruction",
    "Floating point exception",
    "Aborted",
    "Killed",
];

/// Recognize the line that reports how a process ended.
///
/// A shell names a fatal signal alone or with `(core dumped)`
/// (`Segmentation fault (core dumped)`), macOS adds the signal number
/// (`Segmentation fault: 11`), zsh prefixes its name and appends the command
/// (`zsh: segmentation fault  ./a.out`), and Make quotes the report after the
/// target. Runners and IDEs print the status: `exit code 1`, `exit status 2`,
/// `Process finished with exit code 1`. A sentence that merely starts with one
/// of these words, such as `Killed the old worker`, does not match.
fn is_termination_line(line: &str) -> bool {
    if line.ends_with("(core dumped)") {
        return true;
    }
    if let Some(code) = [
        "Process finished with exit code ",
        "exit code ",
        "exit status ",
    ]
    .iter()
    .find_map(|prefix| line.strip_prefix(prefix))
    {
        return code.parse::<i32>().is_ok();
    }
    let (zsh, report) = line
        .strip_prefix("zsh: ")
        .map_or((false, line), |rest| (true, rest));
    FATAL_SIGNALS.iter().any(|signal| {
        let Some(rest) = report
            .get(..signal.len())
            .filter(|head| head.eq_ignore_ascii_case(signal) && (zsh || head == signal))
            .and_then(|_| report.get(signal.len()..))
        else {
            return false;
        };
        rest.is_empty()
            || rest
                .strip_prefix(": ")
                .is_some_and(|number| number.parse::<u8>().is_ok())
            || (zsh && rest.starts_with(char::is_whitespace))
    })
}

/// Recognize a line written about output rather than printed by it.
///
/// A `#` heading or comment is commentary, and so is a line with English
/// grammar ([`super::commands::has_prose_grammar`]) that asks a question,
/// speaks in the first or second person (`Getting this error when I run it`),
/// or is a whole sentence ending in `.`, `!`, or `:`. Program status lines read
/// as English too (`Loading data from /data/train`), but rarely do any of
/// these, and labeled runtime lines (`UserWarning: ...`, `wandb: ...`), prompt
/// lines, progress lines with an ellipsis, and Python's `Exception in thread`
/// header are never commentary.
fn is_commentary(line: &str) -> bool {
    if line.starts_with('#') {
        return true;
    }
    let labeled = line
        .split_whitespace()
        .next()
        .is_some_and(|word| word.len() > 1 && word.ends_with(':'));
    if labeled
        || line.starts_with("$ ")
        || line.starts_with("% ")
        || line.contains("...")
        || line.starts_with("Exception in thread ")
        || !super::commands::has_prose_grammar(line)
    {
        return false;
    }
    let personal = line.split_whitespace().any(|word| {
        let word = word
            .split(['\'', '’'])
            .next()
            .unwrap_or_default()
            .trim_matches(|ch: char| !ch.is_alphabetic());
        word == "I"
            || ["me", "my", "we", "us", "our", "you", "your"]
                .contains(&word.to_lowercase().as_str())
    });
    line.ends_with('?')
        || personal
        || (line.ends_with(['.', '!', ':']) && super::looks_like_prose(line))
}

/// Recognize a Java exception header followed by an `at Class.method(File.java:N)` frame.
fn looks_like_java_stack_trace(sample: &str) -> bool {
    let mut lines = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let header = lines.next().is_some_and(|line| {
        line.starts_with("Exception in thread \"")
            || line.split(':').next().is_some_and(|name| {
                name.contains('.')
                    && !name.contains(' ')
                    && ["Exception", "Error", "Throwable"]
                        .iter()
                        .any(|suffix| name.ends_with(suffix))
            })
    });
    header
        && lines.next().is_some_and(|frame| {
            frame
                .strip_prefix("at ")
                .is_some_and(|call| call.ends_with(')') && call.contains('('))
        })
}

/// Recognize a rustc diagnostic: an `error`/`warning` header and a `-->` location.
fn looks_like_rustc_diagnostic(sample: &str) -> bool {
    let mut lines = sample
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    lines.next().is_some_and(|line| {
        ["error", "warning"].iter().any(|level| {
            line.strip_prefix(level)
                .is_some_and(|rest| rest.starts_with(':') || rest.starts_with('['))
        })
    }) && lines
        .next()
        .and_then(|line| line.strip_prefix("--> "))
        .is_some_and(|location| location.contains(':'))
}

/// Accept Cargo's own status lines and an entered `cargo run` command before a
/// panic header. A status verb needs a machine token such as a version,
/// backticked profile, or path, so `Finished painting the wall` ends the preamble.
fn is_cargo_run_preamble(line: &str) -> bool {
    if let Some((verb, status)) = line.split_once(' ') {
        if CARGO_STATUS_VERBS.contains(&verb)
            && (has_machine_token(status) || status.starts_with(|ch: char| ch.is_ascii_digit()))
        {
            return true;
        }
    }
    let command = line
        .strip_prefix('$')
        .or_else(|| line.strip_prefix('%'))
        .or_else(|| line.strip_prefix('>'))
        .map(str::trim_start)
        .unwrap_or(line);
    command == "cargo run" || command.starts_with("cargo run ")
}
