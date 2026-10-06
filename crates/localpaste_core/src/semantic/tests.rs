//! Semantic classification and retrieval regressions.

use super::{derive, extract_definition_handle_from_line, PasteKind};

#[test]
fn explicit_documents_override_code_and_log_signals() {
    for language in ["markdown", "md", "rst", "restructuredtext", "latex", "tex"] {
        assert_eq!(
            derive(
                "# Notes\n```rust\nfn main() {}\n```\nerror: panic caused by: bad input",
                Some(language)
            )
            .kind,
            PasteKind::Document,
            "{language}"
        );
    }
    assert_eq!(
        derive("A short prose note to keep for later.", None).kind,
        PasteKind::Document
    );
    assert_eq!(derive("", None).kind, PasteKind::Other);
    assert_eq!(derive("fn main() {}", Some("rust")).kind, PasteKind::Code);
    for (content, expected) in [
        ("```python\nprint('hello')\n```", PasteKind::Code),
        ("```json\n{\"name\":\"Ada\"}\n```", PasteKind::Config),
        ("\n  ~~~sh\necho hello world\n  ~~~~\n", PasteKind::Code),
        ("```\necho hello world\n```", PasteKind::Code),
        ("```text\nINFO Starting the server\n```", PasteKind::Log),
        (
            "```text\nA short prose note to keep.\n```",
            PasteKind::Document,
        ),
        ("```\nA short prose note to keep.\n```", PasteKind::Document),
        ("```\nhello\n```", PasteKind::Other),
        (
            "```\nfn main() { println!(\"hello\"); }\n```",
            PasteKind::Code,
        ),
        ("```\n{\"a\":1}\n{\"a\":2}\n```", PasteKind::Config),
        (
            "```\nERROR\tcount\tname\n404\t2\twidget\n```",
            PasteKind::Other,
        ),
        (
            "```\ndeadbeefcafebabe0123456789abcdef\n```",
            PasteKind::Other,
        ),
        ("```markdown\n# Read me\n```", PasteKind::Document),
        (
            "Notes before the example\n```python\nprint('hello')\n```",
            PasteKind::Document,
        ),
        (
            "```python\nprint('hello')\n```\nNotes after the example",
            PasteKind::Document,
        ),
        ("```python\nprint('hello')", PasteKind::Document),
        ("```python\nprint('hello')\n~~~", PasteKind::Document),
    ] {
        assert_eq!(
            derive(content, Some("markdown")).kind,
            expected,
            "{content}"
        );
    }
    let large_fence = format!("```python\n{}\n```", "print('hello')\n".repeat(5_000));
    assert_eq!(derive(&large_fence, Some("markdown")).kind, PasteKind::Code);
}

#[test]
fn untyped_structural_content_does_not_default_to_document() {
    let cases = [
        ("name,age\nAda,37", PasteKind::Other),
        ("name,age,city", PasteKind::Other),
        ("deadbeefcafebabe0123456789abcdef", PasteKind::Other),
        ("550e8400-e29b-41d4-a716-446655440000", PasteKind::Other),
        ("VGhpcyBpcyBhIHNlY3JldCB0b2tlbg==", PasteKind::Other),
        ("aLongAlphabeticTokenWithoutSpaces", PasteKind::Other),
        ("ls -la /var/log", PasteKind::Code),
        ("brew install localpaste", PasteKind::Code),
        ("cd app\nnpm install\nnpm run dev", PasteKind::Code),
        ("conda activate misc\npip install requests", PasteKind::Code),
        ("export MODE=dev\ncargo build", PasteKind::Code),
        ("source .env\ncargo build", PasteKind::Code),
        ("set -e\ncargo build", PasteKind::Code),
        ("mkdir -p out\ncargo build", PasteKind::Code),
        ("cd app\ngit status", PasteKind::Code),
        ("cd app\necho ready", PasteKind::Code),
        ("just build", PasteKind::Code),
        ("just deploy prod", PasteKind::Code),
        ("just clean build test release", PasteKind::Code),
        ("just deploy staging production cluster", PasteKind::Code),
        ("make test", PasteKind::Code),
        ("make CC=clang all", PasteKind::Code),
        ("make clean install package docs", PasteKind::Code),
        ("make build test lint release", PasteKind::Code),
        ("git checkout main README Cargo docs", PasteKind::Code),
        ("git add README Cargo docs notes", PasteKind::Code),
        ("sudo systemctl restart nginx", PasteKind::Code),
        ("echo hello world", PasteKind::Code),
        ("echo hello there this is fine", PasteKind::Code),
        ("docker run ubuntu echo hello world", PasteKind::Code),
        ("printf hello world", PasteKind::Code),
        ("INFO Starting the server", PasteKind::Log),
        ("[INFO] Server started successfully", PasteKind::Log),
        ("info Starting the server", PasteKind::Document),
        ("info about the offsite agenda", PasteKind::Document),
        ("error in the invoice needs correcting", PasteKind::Document),
        ("echo chamber is a common expression", PasteKind::Document),
        ("sudo is required for this command", PasteKind::Document),
        (
            "Hello team,\nsudo is required for installation.",
            PasteKind::Document,
        ),
        ("echo 'this is a test'", PasteKind::Code),
        ("[WARN] Retry in thirty seconds", PasteKind::Log),
        ("thread 'main' panicked at src/main.rs:12:5", PasteKind::Log),
        ("error: unable to open database", PasteKind::Log),
        ("WARNING: database connection unavailable", PasteKind::Log),
        ("debug: true\nname: foo", PasteKind::Config),
        (
            "Warning: do not touch the deployment settings.",
            PasteKind::Document,
        ),
        ("Hello, Bob\nSee you soon, Alice", PasteKind::Document),
        (
            "First, check the plan, then confirm it.\nNext, send it.",
            PasteKind::Document,
        ),
        ("Just a reminder to save your work.", PasteKind::Document),
        ("just a reminder to save your work", PasteKind::Document),
        (
            "just remember to save your work before leaving",
            PasteKind::Document,
        ),
        (
            "just wanted to say thanks for your help",
            PasteKind::Document,
        ),
        ("just hoped we could talk today", PasteKind::Document),
        ("just remember this later", PasteKind::Document),
        (
            "make sure to save your work before leaving",
            PasteKind::Document,
        ),
        ("make sure to save", PasteKind::Document),
        ("make sure you bring your badge", PasteKind::Document),
        ("make yourself at home", PasteKind::Document),
        ("just checking in", PasteKind::Document),
        ("just a quick reminder for tomorrow", PasteKind::Document),
        ("git history helps explain this change", PasteKind::Document),
        ("git history helps explain", PasteKind::Document),
        ("cd app\ngit history helps explain", PasteKind::Document),
        (
            "cd app\nmake sure you bring your badge",
            PasteKind::Document,
        ),
        ("cd app\njust checking in", PasteKind::Document),
        (
            "cd app\necho chamber is a common expression",
            PasteKind::Document,
        ),
        ("Make a note of the deployment window.", PasteKind::Document),
        (
            "Python 3.12 is now installed on the workstation.",
            PasteKind::Document,
        ),
        (
            "Git is down for scheduled maintenance.",
            PasteKind::Document,
        ),
        (
            "Brew is ready for the next deployment.",
            PasteKind::Document,
        ),
        ("Ls lists the files in this folder.", PasteKind::Document),
        ("first name,age\nAda Lovelace,37", PasteKind::Other),
        ("name;age\nAda;37", PasteKind::Other),
        ("name\tage\nAda Lovelace\t37", PasteKind::Other),
        (
            "Bonjour à tous, à demain pour la réunion.",
            PasteKind::Document,
        ),
        (
            "info about the schedule\nerror in the report needs correction",
            PasteKind::Document,
        ),
        (
            "export controls are discussed here\nsource material follows",
            PasteKind::Document,
        ),
    ];

    for (content, expected) in cases {
        for language in [None, Some("text")] {
            assert_eq!(
                derive(content, language).kind,
                expected,
                "{content}: {language:?}"
            );
        }
    }
    assert_eq!(
        derive("A short prose note to keep for later.", Some("text")).kind,
        PasteKind::Document
    );
}

#[test]
fn git_subcommands_stay_consistent_across_detection_and_semantics() {
    for subcommand in crate::detection::SHELL_GIT_SUBCOMMANDS {
        let content = format!("git {subcommand} fixture\ncargo check\n");
        assert!(
            crate::detection::looks_like_shell_command_sequence(&content),
            "{subcommand}"
        );
        let derived = derive(&content, None);
        assert_eq!(derived.kind, PasteKind::Code, "{subcommand}");
        assert_eq!(
            derived.handle.as_deref(),
            Some(format!("git {subcommand}").as_str()),
            "{subcommand}"
        );
    }
}

#[test]
fn derive_matrix_covers_code_config_log_link_and_other() {
    for (content, language, expected) in [
        (
            "[INFO] Server started\nINFO Starting worker\nWARN Queue full",
            "dockerfile",
            PasteKind::Log,
        ),
        ("INFO Starting the server", "dockerfile", PasteKind::Log),
        (
            "thread 'main' panicked at src/main.rs:12:5",
            "html",
            PasteKind::Log,
        ),
        ("[INFO]\nname = \"worker\"", "toml", PasteKind::Config),
        ("INFO = \"Starting worker\"", "python", PasteKind::Code),
        ("info () { echo \"ready\"; }", "shell", PasteKind::Code),
        ("INFO () { echo \"ready\"; }", "shell", PasteKind::Code),
        ("error\tamount\nmissing\t12", "tsv", PasteKind::Other),
        ("INFO\tstatus\nready\t12", "tsv", PasteKind::Other),
        ("INFO\tstatus", "tsv", PasteKind::Other),
        ("INFO Starting the server", "tsv", PasteKind::Log),
        ("INFO\t(main)\tstarting service", "tsv", PasteKind::Log),
        ("ERROR\tcount\tname", "tsv", PasteKind::Other),
        ("[WARN] Retry in thirty seconds", "tsv", PasteKind::Log),
        ("INFO (main) Starting the server", "text", PasteKind::Log),
        (
            "info Resolving packages\nwarning Retrying request\nsuccess Saved lockfile",
            "text",
            PasteKind::Log,
        ),
        (
            "info Resolving packages\ninfo Fetching packages",
            "text",
            PasteKind::Log,
        ),
        (
            "INFO\tStarting worker\nERROR\tWorker stopped",
            "tsv",
            PasteKind::Log,
        ),
        (
            "fn main() {}\nthread 'main' panicked at src/main.rs:12:5",
            "rust",
            PasteKind::Code,
        ),
        ("debug: true\nname: foo", "yaml", PasteKind::Config),
        (
            "FROM ubuntu\nRUN echo hello world",
            "dockerfile",
            PasteKind::Config,
        ),
        ("[INFO] Server started", "markdown", PasteKind::Document),
    ] {
        assert_eq!(
            derive(content, Some(language)).kind,
            expected,
            "{language}: {content}"
        );
    }
    let code = derive("fn handle_request(input: &str) {}\n", Some("rust"));
    assert_eq!(code.kind, PasteKind::Code);
    assert_eq!(code.handle.as_deref(), Some("fn handle_request"));

    let config = derive("model: gpt-4\nbatch: 32\n", Some("yaml"));
    assert_eq!(config.kind, PasteKind::Config);
    assert_eq!(config.handle.as_deref(), Some("model gpt-4"));

    let log = derive(
        "panic: failed to bind\ncaused by: port already in use\n",
        Some("text"),
    );
    assert_eq!(log.kind, PasteKind::Log);
    assert!(log
        .handle
        .as_deref()
        .map(|handle| handle.starts_with("panic"))
        .unwrap_or(false));

    let link = derive("https://example.com/docs\n", Some("text"));
    assert_eq!(link.kind, PasteKind::Link);
    assert_eq!(link.handle.as_deref(), Some("example.com"));

    let short_text = derive("hi", Some("text"));
    assert_eq!(short_text.kind, PasteKind::Other);
    assert!(short_text.handle.is_none());
}

#[cfg(feature = "magika")]
#[test]
fn standalone_fence_kind_does_not_invoke_magika() {
    let before = crate::detection::magika_detection_call_count();
    assert_eq!(
        derive(
            "```\nfn main() { println!(\"hello\"); }\n```",
            Some("markdown")
        )
        .kind,
        PasteKind::Code
    );
    assert_eq!(crate::detection::magika_detection_call_count(), before);
}

#[test]
fn derive_terms_prefers_repeated_technical_tokens() {
    let derived = derive(
        "validation failed for fsdp2 after cublaslt retry\nfsdp2 validation repeated\n",
        Some("text"),
    );
    assert!(derived.terms.iter().any(|term| term == "fsdp2"));
    assert!(derived.terms.iter().any(|term| term == "validation"));
    assert!(derived.terms.iter().any(|term| term == "cublaslt"));
}

#[test]
fn definition_handle_extracts_exported_js_ts_declarations() {
    let cases = [
        (
            "export const renderPanel = () => {};",
            Some("typescript"),
            Some("export const renderPanel"),
        ),
        (
            "export class WorkspacePanel {}",
            Some("javascript"),
            Some("export class WorkspacePanel"),
        ),
    ];

    for (line, language, expected) in cases {
        assert_eq!(
            extract_definition_handle_from_line(line, language).as_deref(),
            expected,
            "line: {line}"
        );
    }
}
