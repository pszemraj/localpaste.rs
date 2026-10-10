//! Command-versus-prose regressions for notes that start with a tool name.

use super::commands::{extract_command_handle, looks_like_shell_command_sequence};
use super::{derive, PasteKind};
use crate::models::paste::{Paste, PasteMeta};

fn with_crlf(content: &str) -> [String; 2] {
    [content.to_owned(), content.replace('\n', "\r\n")]
}

#[test]
fn sentences_led_by_tool_names_stay_documents() {
    for content in [
        "git history doesn't lie about who broke the build",
        "git history from 2019 explains why the parser is so odd",
        "git push got rejected, someone force pushed main",
        "git blame says it was me, but I swear it wasn't",
        "echo chamber is what twitter's become",
        "echo what Sam said, great work everyone!",
        "echo hello there this is fine",
        "curl up with a good book tonight",
        "mkdir fails on the shared drive again today",
        "conda keeps hanging when I install pytorch, any tips?",
        "cargo build takes forever on my laptop, any ideas?",
        "npm install keeps failing behind the corporate proxy",
        "ssh keys need rotating every 90 days per the security policy",
        "brew a fresh pot of coffee for the 9am standup",
        "sudo make me a sandwich please",
        "make sure it's plugged in before you call IT",
        "cd and/or vinyl, whichever is cheaper",
        "git status shows what changed\ngit add stages files\ngit commit records them",
        "make retries configurable\n\nAdds RETRY_LIMIT to the environment so ops can tune it.",
        "cd repo\ngit pull\nThen tell the team the release notes are ready.",
    ] {
        for content in with_crlf(content) {
            assert_eq!(extract_command_handle(&content), None, "{content}");
            assert!(!looks_like_shell_command_sequence(&content), "{content}");
            for language in [None, Some("text")] {
                assert_eq!(
                    derive(&content, language).kind,
                    PasteKind::Document,
                    "{content}: {language:?}"
                );
            }
            let paste = Paste::new(content.clone(), "note".into());
            assert_eq!(
                PasteMeta::from(&paste).derived.kind,
                PasteKind::Document,
                "{content}: {:?}",
                paste.language
            );
        }
    }
}

#[test]
fn git_subcommands_quoted_operands_and_transcripts_stay_commands() {
    for (content, handle) in [
        ("git lfs pull", "git lfs"),
        ("git config user.name Peter", "git config"),
        ("git reflog", "git reflog"),
        ("git am patches.mbox", "git am"),
        ("git rev-parse HEAD", "git rev-parse"),
        ("git blame README.md", "git blame"),
        ("cd repo\ngit submodule update", "cd repo"),
        ("git commit -m \"fix the parser\"", "git commit"),
        ("echo 'this is a test'", "echo this"),
        ("npm i", "npm i"),
        ("docker compose up -d", "docker compose"),
        ("set /p prompt=Enter your full name", "set p"),
        (
            "$ git status\nOn branch main\nnothing to commit, working tree clean",
            "git status",
        ),
    ] {
        for content in with_crlf(content) {
            assert_eq!(
                extract_command_handle(&content).as_deref(),
                Some(handle),
                "{content}"
            );
            for language in [None, Some("text")] {
                assert_eq!(
                    derive(&content, language).kind,
                    PasteKind::Code,
                    "{content}"
                );
            }
        }
    }
}

#[test]
fn makefile_recipes_may_echo_sentences() {
    let makefile = "all:\n\t@echo Building the app for you\n\tcargo build --release\n";
    for body in with_crlf(makefile) {
        assert_eq!(super::makefile_body_language(&body), Some("makefile"));
        assert!(!super::makefile_note_body(&body), "{body}");
    }
}
