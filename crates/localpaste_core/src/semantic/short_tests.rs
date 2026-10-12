//! Unlabeled pastes too short to classify file under the Short kind.

use super::{derive, PasteKind};

#[test]
fn short_unlabeled_content_is_short_rather_than_a_guessed_kind() {
    for content in [
        // Prose-like, code-like, and config-like bodies that a few words once
        // filed as Document, Other, or Config.
        "hello new paste",
        "if ready:\n\tresult = 1",
        "def main():\n\tpass",
        "debug: True\ntrace: False",
        "",
        " \n\t",
    ] {
        for content in [content.to_owned(), content.replace('\n', "\r\n")] {
            assert_eq!(derive(&content, None).kind, PasteKind::Short, "{content:?}");
            assert_eq!(
                derive(&content, Some(" ")).kind,
                PasteKind::Short,
                "{content:?}"
            );
        }
    }
    // Search terms and handles are still derived for metadata search.
    let derived = derive("debug: True\ntrace: False", None);
    assert_eq!(derived.terms, ["debug", "trace"]);
}

#[test]
fn a_short_url_or_strict_log_line_keeps_its_certain_kind() {
    let link = derive("https://example.com/some/page", None);
    assert_eq!(link.kind, PasteKind::Link);
    assert_eq!(link.handle.as_deref(), Some("example.com"));
    for content in [
        "2026-10-10 06:00:01 INFO starting scheduler",
        "[2026-10-10 06:00:01] ERROR scheduler crashed",
        "thread 'main' panicked at src/main.rs:12:5",
        "Segmentation fault (core dumped)",
    ] {
        assert_eq!(derive(content, None).kind, PasteKind::Log, "{content}");
    }
    // Unlabeled, a level line this short is not certain runtime output: only a
    // timestamped line passes the strict runtime-log rule. Labeled plain text,
    // the level rule still files it as a Log.
    for content in [
        "INFO Starting the server",
        "[INFO] Server started successfully",
        "[WARN] Retry in thirty seconds",
        "error: unable to open database",
        "WARNING: database connection unavailable",
    ] {
        assert_eq!(derive(content, None).kind, PasteKind::Short, "{content}");
        assert_eq!(
            derive(content, Some("text")).kind,
            PasteKind::Log,
            "{content}"
        );
    }
}

#[test]
fn a_short_paste_with_a_language_keeps_the_kind_its_language_gives() {
    for (content, language, kind) in [
        ("fn main() {}", "rust", PasteKind::Code),
        ("def main():\n\tpass", "python", PasteKind::Code),
        ("debug: True\ntrace: False", "yaml", PasteKind::Config),
        ("hello new paste", "markdown", PasteKind::Document),
        ("hello new paste", "text", PasteKind::Document),
        ("hi", "text", PasteKind::Other),
        ("", "rust", PasteKind::Other),
    ] {
        assert_eq!(
            derive(content, Some(language)).kind,
            kind,
            "{language}: {content:?}"
        );
    }
}

#[test]
fn short_uses_the_auto_detection_minimum() {
    // Prose padded to `len` characters, with no trailing whitespace.
    let prose = |len: usize| {
        "plain words "
            .repeat(8)
            .chars()
            .take(len)
            .collect::<String>()
    };
    for (content, short) in [
        (prose(63), true),
        (format!("\n  {}\t\n", prose(63)), true),
        (prose(64), true),
        (format!("{}\n\n  \ncall the bank", prose(64)), true),
        ("buy milk today\n\n  \ncall the bank".to_owned(), true),
        (
            "buy milk today\ncall the bank\nwalk the dog".to_owned(),
            true,
        ),
        (format!("{}\ncall the bank\nwalk the dog", prose(40)), false),
    ] {
        assert_eq!(
            crate::detection::too_short_to_classify(&content),
            short,
            "{content:?}"
        );
        let kind = derive(&content, None).kind;
        assert_eq!(kind == PasteKind::Short, short, "{content:?}: {kind:?}");
        if !short {
            assert_eq!(kind, PasteKind::Document, "{content:?}");
        }
    }
}
