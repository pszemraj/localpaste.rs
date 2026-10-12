//! Batch (CMD) label refinement and the kinds of setup-word commands and notes.

#[cfg(feature = "magika")]
use super::detect_language;
use super::refine_magika_label;

#[cfg(feature = "magika")]
#[test]
fn automatic_cmd_variable_assignments_keep_code_kind() {
    let cases = [
        "set \"long variable name=value\"",
        "set \"my variable=hello world\"",
        "set long variable name=value",
        "set my variable=hello world",
    ];
    for content in cases {
        for ending in ["\n", "\r\n"] {
            let content = format!("{content}{ending}");
            let language = detect_language(&content);
            assert_eq!(
                refine_magika_label("batch", &content).as_deref(),
                Some("batch"),
                "{content}"
            );
            assert_eq!(
                crate::semantic::derive(&content, language.as_deref()).kind,
                crate::test_support::kind_unless_short(
                    &content,
                    language.as_deref(),
                    crate::semantic::PasteKind::Code
                ),
                "{content}"
            );
            if content.starts_with("set my variable=hello world") {
                assert_eq!(language.as_deref(), Some("batch"));
            }
        }
    }
}

#[cfg(feature = "magika")]
#[test]
fn automatic_setup_word_notes_remain_documents() {
    use crate::models::paste::{Paste, PasteMeta};
    use crate::semantic::PasteKind;

    let derived_kind = |content: &str| {
        let paste = Paste::new(content.into(), "review note".into());
        assert_eq!(paste.language, None, "{content}");
        assert!(!paste.language_is_manual);
        PasteMeta::from(&paste).derived.kind
    };
    // The second note of each pair clears the auto-detect minimum, so detection
    // and the body rules classify it instead of filing it under Short.
    for (short, classified) in [
        (
            "set timer for 10 minutes\r\n",
            "set timer for 10 minutes before the release review starts this afternoon\r\nThe meeting starts after lunch.\r\nPlease bring your review notes.\r\n",
        ),
        (
            "source code is at https://example.com\r\n",
            "source code is at https://example.com and the review notes are in the wiki\r\nThe meeting starts after lunch.\r\nPlease bring your review notes.\r\n",
        ),
    ] {
        assert_eq!(derived_kind(short), PasteKind::Short, "{short}");
        assert_eq!(
            derived_kind(classified),
            PasteKind::Document,
            "{classified}"
        );
    }
}

#[test]
fn batch_refinement_rejects_setup_prose_and_retains_script_structure() {
    for content in [
        "set timer for 10 minutes\r\n",
        "source code is at https://example.com\r\n",
        "inspect logs at C:\\projects\\app",
    ] {
        assert_eq!(refine_magika_label("batch", content), None, "{content}");
    }
    for content in [
        "@echo off\r\nset MODE=dev\r\necho %MODE%\r\n",
        "rem keep this batch script comment",
        "rem inspect logs at C:\\projects\\app",
        "copy source destination",
        "dir logs in C:\\projects\\app",
        "type notes at C:\\projects\\app\\file.txt",
        "copy source in C:\\projects\\app",
        "move source at C:\\projects\\app",
        "call inspect logs at C:\\projects\\app",
        "if exist file echo found",
        "for %%f in C:\\input\\* do echo %%f",
        "set MODE=dev",
        "set \"my variable\"",
        "set \"alpha beta\" gamma",
        "set PATH && echo done",
        "set +o errexit\ncargo check",
        "set alpha beta\ncargo check",
        "export \"PATH\"",
        "export PATH\ncargo check",
        "export PATH HOME\ncargo check",
        "export PATH && cargo check",
        "source env\ncargo check",
        "source activate myenv\ncargo check",
        "cd Program Files\ngit status",
        "cd Program Files2\ngit status",
        "cd \"Program Files\" && git status",
        "cd repo && git status",
        "cd repo; git status",
        "set /p prompt=Enter your full name",
        "set /A count=10",
        "cd /D C:\\Users\\project",
        "cd Program Files",
        "cd C:\\Program Files",
        "set /?",
        "set /? additional words",
        "source .env\ncargo check",
        "source .env arg",
        "source \"my project.env\"",
        "source \"project setup.env\"\r\ncargo check",
        "source 'project setup.env' arg",
        "export MODE=dev",
        "export -n PATH",
        "export -p",
        "export -f build",
        "export -nf build",
        "export PATH MODE=dev",
        "cd -P repo",
        "cd -L repo",
        "cd -- repo",
        "source\tcode\tlocation\nmain\trust\tpath",
        "export function renderPanel() {}",
    ] {
        assert_eq!(
            refine_magika_label("batch", content).as_deref(),
            Some("batch"),
            "{content}"
        );
    }
}
