//! Semantic classification and retrieval regressions.

use super::{derive, extract_definition_handle_from_line, PasteKind};

#[test]
fn review_regression_prose_flags_paths_and_currency_stay_documents() {
    for content in [
        "check logs in /var/log/app",
        "buy milk - 2 cartons",
        "$ 5 for a coffee is too much honestly",
        "% 5 is a small percentage of the budget",
        "inspect logs at C:\\projects\\app",
        "write notes - remember tomorrow",
        "check logs -- remember tomorrow",
        "inspect report/example for errors",
    ] {
        for language in [None, Some("text")] {
            assert_eq!(
                derive(content, language).kind,
                PasteKind::Document,
                "{content}"
            );
        }
        let paste = crate::models::paste::Paste::new(content.into(), "ordinary note".into());
        assert_eq!(
            crate::models::paste::PasteMeta::from(&paste).derived.kind,
            PasteKind::Document,
            "{content}: {:?}",
            paste.language
        );
    }
    for content in [
        "apt install -y git curl",
        "rsync ./source ./dest",
        "$ apt install git",
        "% apt install git",
        "$ python script",
        "> python script.py",
    ] {
        assert_ne!(derive(content, None).kind, PasteKind::Document, "{content}");
    }
}

#[test]
fn weak_markdown_labels_do_not_hide_whole_technical_bodies() {
    for (content, expected) in [
        ("# install\npip install foo", PasteKind::Code),
        ("# run it\ndocker run -it --rm ubuntu bash", PasteKind::Code),
        ("# update\nrustup update stable", PasteKind::Code),
        ("# deps\nnpm i", PasteKind::Code),
        ("# run tests\npytest", PasteKind::Code),
        (
            "# fetch the page\ncurl https://example.com",
            PasteKind::Code,
        ),
        ("# connect\nssh user@host", PasteKind::Code),
        ("# containers\ndocker ps", PasteKind::Code),
        ("# build\nall:\n\tcargo build", PasteKind::Code),
        (
            "npm ERR! code 1\nnpm ERR! command failed\n> demo@1.0.0 test\n> pytest",
            PasteKind::Code,
        ),
        (
            "> Traceback (most recent call last):\n>   File \"demo.py\", line 1, in <module>\n> ValueError: bad value",
            PasteKind::Log,
        ),
        ("import X", PasteKind::Code),
        ("# install\napt install -y git curl", PasteKind::Other),
        ("# build\nFOO=bar cargo build --release", PasteKind::Other),
        ("# install\n> apt install -y git curl", PasteKind::Other),
        ("# build\n> FOO=bar cargo build --release", PasteKind::Other),
        ("# install\n$ pip install foo bar", PasteKind::Code),
        ("# install\nprintf '[label](url)'", PasteKind::Code),
        ("stderr:\n> error: failed\nexit code 1", PasteKind::Log),
        (
            "stderr:\n> error: [label](url)\nexit code 1",
            PasteKind::Log,
        ),
        (
            "# Compute things\nimport numpy as np\nprint('[label](url)')",
            PasteKind::Code,
        ),
        (
            "# Compute things\nimport numpy as np\nvalues = np.array([1,2,3])\nprint(values.sum())",
            PasteKind::Code,
        ),
    ] {
        for content in [content.to_owned(), content.replace('\n', "\r\n")] {
            assert_eq!(
                derive(&content, Some("markdown")).kind,
                expected,
                "{content}"
            );
            for language in ["rst", "latex"] {
                assert_eq!(derive(&content, Some(language)).kind, PasteKind::Document);
            }
        }
    }
    for content in [
        "# Installation\n\nRun the command below.\n\npip install foo",
        "> This is a quoted note for tomorrow.",
        "# Notes\n> This is quoted prose about /usr/bin tools.",
        "# Logs\n\nHere is the output:\n\n```text\nstderr:\n> error: failed\nexit code 1\n```",
        "# Python notes\n\nHere is how the code works.\n\nimport numpy as np\nprint(np.arange(10))",
        "# Python notes\nimport numpy as np\nThis paragraph explains the example.",
        "# Notes\nimport numpy as np\nrelease details",
        "# Notes\nrelease details",
        "# Notes\n[label](url)",
        "> This is a quoted explanation.\n> The example uses `pip install foo`.",
    ] {
        assert_eq!(
            derive(content, Some("markdown")).kind,
            PasteKind::Document,
            "{content}"
        );
    }
}

#[test]
fn unlisted_or_prefixed_commands_do_not_become_prose_documents() {
    for content in [
        "$ pip install foo bar",
        "apt install -y git curl",
        "rustup update stable",
        "FOO=bar cargo build --release",
    ] {
        for language in [None, Some("text")] {
            assert_ne!(
                derive(content, language).kind,
                PasteKind::Document,
                "{content}"
            );
        }
    }
    for content in [
        "remember to use --release when building the app",
        "notes about /usr/bin tools and their behavior",
        "The --release option optimizes the build.",
    ] {
        assert_eq!(derive(content, None).kind, PasteKind::Document, "{content}");
    }
    let long_prose = "This is a long ordinary prose note about the release. ".repeat(2_000);
    assert_eq!(derive(&long_prose, None).kind, PasteKind::Document);
}

#[test]
fn complete_record_sampling_keeps_delimited_and_log_kinds_stable() {
    for ending in ["\n", "\r\n"] {
        let csv_row = format!(
            "\"{}\",\"{}\"{ending}",
            "alpha beta ".repeat(30),
            "gamma delta ".repeat(30)
        );
        let tsv_row = format!("404\t{}\twidget{ending}", "alpha beta ".repeat(50));
        let log_row = format!("INFO Starting worker {}{ending}", "alpha beta ".repeat(50));
        for offset in 0..40 {
            let csv = format!(
                "\"{}{}\",\"{}\"{ending}{}",
                "alpha beta ".repeat(30),
                "x".repeat(offset),
                "gamma delta ".repeat(30),
                csv_row.repeat(140)
            );
            let tsv = format!(
                "ERROR\tcount\tname{ending}4{}04\talpha beta\twidget{ending}{}",
                "0".repeat(offset),
                tsv_row.repeat(140)
            );
            let log = format!(
                "INFO Starting {}worker{ending}{}",
                "X".repeat(offset),
                log_row.repeat(140)
            );
            for language in [None, Some("text")] {
                assert_eq!(
                    derive(&csv, language).kind,
                    PasteKind::Other,
                    "CSV offset {offset} {ending:?}"
                );
                assert_eq!(
                    derive(&tsv, language).kind,
                    PasteKind::Other,
                    "TSV offset {offset} {ending:?}"
                );
                assert_eq!(
                    derive(&log, language).kind,
                    PasteKind::Log,
                    "log offset {offset} {ending:?}"
                );
            }
        }
    }
}

#[test]
fn explicit_setup_queries_chains_and_adjacent_commands_remain_code() {
    for content in [
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
        "source \"my env\" && cargo check",
        "source .env && cargo check",
        "cd Program Files\ngit status",
        "cd Program Files2\ngit status",
        "cd \"Program Files\" && git status",
        "cd repo && git status",
        "cd repo; git status",
    ] {
        for content in [
            format!("{content}\n"),
            format!("{content}\n").replace('\n', "\r\n"),
        ] {
            for language in [None, Some("text"), Some("batch")] {
                assert_eq!(
                    derive(&content, language).kind,
                    PasteKind::Code,
                    "{content}: {language:?}"
                );
            }
            #[cfg(feature = "magika")]
            {
                let paste =
                    crate::models::paste::Paste::new(content.clone(), "setup boundary".into());
                assert_eq!(
                    crate::models::paste::PasteMeta::from(&paste).derived.kind,
                    PasteKind::Code,
                    "{content}"
                );
            }
        }
    }
    for content in ["export PATH2", "source env2"] {
        for language in [None, Some("text"), Some("batch")] {
            assert_eq!(
                derive(content, language).kind,
                PasteKind::Other,
                "{content}"
            );
        }
    }
}

#[test]
fn incidental_operators_in_setup_prose_do_not_establish_commands() {
    for content in [
        "source code is at https://example.com?a=1&b=2",
        "set timer for 10 minutes; remember tea",
    ] {
        assert_eq!(
            super::commands::extract_command_handle(content),
            None,
            "{content}"
        );
        for language in [None, Some("text"), Some("batch")] {
            assert_ne!(derive(content, language).kind, PasteKind::Code, "{content}");
        }
    }
}

#[test]
fn setup_options_and_later_assignments_keep_code_kind() {
    for content in [
        "export -n PATH",
        "export -p",
        "export -f build",
        "export -nf build",
        "export PATH MODE=dev",
        "cd -P repo",
        "cd -L repo",
        "cd -- repo",
    ] {
        for ending in ["\n", "\r\n"] {
            let content = format!("{content}{ending}");
            for language in [None, Some("text"), Some("batch")] {
                assert_eq!(
                    derive(&content, language).kind,
                    PasteKind::Code,
                    "{content}: {language:?}"
                );
            }
            #[cfg(feature = "magika")]
            {
                let paste =
                    crate::models::paste::Paste::new(content.clone(), "setup command".into());
                assert_eq!(
                    crate::models::paste::PasteMeta::from(&paste).derived.kind,
                    PasteKind::Code,
                    "{content}"
                );
            }
        }
    }
}

#[test]
fn quoted_source_paths_with_spaces_keep_code_kind() {
    for content in [
        "source \"my project.env\"\n",
        "source \"project setup.env\"\ncargo check\n",
        "source 'project setup.env' arg\n",
    ] {
        for content in [content.to_owned(), content.replace('\n', "\r\n")] {
            for language in [None, Some("text"), Some("batch")] {
                let derived = derive(&content, language);
                assert_eq!(derived.kind, PasteKind::Code, "{content}");
                assert!(
                    derived
                        .handle
                        .as_deref()
                        .is_some_and(|handle| handle.starts_with("source ")),
                    "{content}"
                );
            }
            #[cfg(feature = "magika")]
            {
                let paste =
                    crate::models::paste::Paste::new(content.clone(), "source command".into());
                assert_eq!(
                    crate::models::paste::PasteMeta::from(&paste).derived.kind,
                    PasteKind::Code,
                    "{content}"
                );
            }
        }
    }
}

#[test]
fn cmd_variable_assignments_with_spaces_keep_code_kind() {
    for content in [
        "set \"long variable name=value\"",
        "set \"my variable=hello world\"",
        "set long variable name=value",
        "set my variable=hello world",
    ] {
        for ending in ["", "\n", "\r\n"] {
            let content = format!("{content}{ending}");
            for language in [None, Some("text"), Some("batch")] {
                let derived = derive(&content, language);
                assert_eq!(derived.kind, PasteKind::Code, "{content}: {language:?}");
                assert!(
                    derived
                        .handle
                        .as_deref()
                        .is_some_and(|handle| handle.starts_with("set ")),
                    "{content}"
                );
            }
        }
    }
}

#[test]
fn batch_setup_prose_is_document_without_broad_language_override() {
    for content in [
        "set timer for 10 minutes\r\n",
        "source code is at https://example.com\r\n",
        "inspect logs at C:\\projects\\app",
    ] {
        assert_eq!(
            derive(content, Some("batch")).kind,
            PasteKind::Document,
            "{content}"
        );
        assert_eq!(derive(content, Some("python")).kind, PasteKind::Code);
        for language in ["csv", "tsv"] {
            assert_eq!(derive(content, Some(language)).kind, PasteKind::Other);
        }
    }
    for (content, expected) in [
        ("set MODE=dev", PasteKind::Code),
        ("set -e", PasteKind::Code),
        ("export MODE=dev", PasteKind::Code),
        ("source .env", PasteKind::Other),
        ("source .env arg", PasteKind::Other),
        ("source .env\ncargo check", PasteKind::Code),
        ("source ./.env", PasteKind::Code),
        ("cd repo\ncargo check", PasteKind::Code),
        (
            "@echo off\r\nset MODE=dev\r\necho %MODE%\r\n",
            PasteKind::Other,
        ),
        ("rem keep this batch script comment", PasteKind::Other),
        ("rem inspect logs at C:\\projects\\app", PasteKind::Other),
        ("copy source destination", PasteKind::Other),
        ("dir logs in C:\\projects\\app", PasteKind::Other),
        (
            "type notes at C:\\projects\\app\\file.txt",
            PasteKind::Other,
        ),
        ("copy source in C:\\projects\\app", PasteKind::Other),
        ("move source at C:\\projects\\app", PasteKind::Other),
        ("call inspect logs at C:\\projects\\app", PasteKind::Other),
        ("if exist file echo found", PasteKind::Other),
        ("for %%f in C:\\input\\* do echo %%f", PasteKind::Other),
        ("set /p prompt=Enter your full name", PasteKind::Code),
        ("set /a count=10", PasteKind::Code),
        ("cd /d C:\\Users\\project", PasteKind::Code),
        ("cd Program Files", PasteKind::Other),
        ("cd C:\\Program Files", PasteKind::Code),
        ("cd /d Program Files", PasteKind::Code),
        ("set /?", PasteKind::Code),
        ("set /? additional words", PasteKind::Code),
        ("set PATH", PasteKind::Other),
        ("set PATH\necho hello world", PasteKind::Code),
        ("source\tcode\tlocation\nmain\trust\tpath", PasteKind::Other),
        ("source,code,location\nmain,rust,path", PasteKind::Other),
        ("export function renderPanel() {}", PasteKind::Other),
    ] {
        assert_eq!(derive(content, Some("batch")).kind, expected, "{content}");
    }
    assert_eq!(
        derive("export function renderPanel() {}", Some("javascript")).kind,
        PasteKind::Code
    );
}

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
fn setup_words_in_prose_do_not_acquire_command_handles() {
    for content in [
        "set timer for 10 minutes",
        "set the table for 6 people before dinner",
        "export markets are down 5% this quarter",
        "source code is at https://example.com",
        "set oven to 180C",
        "set default timeout to 30s",
        "export last year's notes",
        "cd albums are on the shelf/desk",
        "set timer for 10 minutes\nThis is a meeting note.\ncargo check",
        "cd repo\nThis is a meeting note.\ncargo check",
    ] {
        for content in std::iter::once(content.to_owned()).chain(
            content
                .contains('\n')
                .then(|| content.replace('\n', "\r\n")),
        ) {
            assert_eq!(
                super::commands::extract_command_handle(&content),
                None,
                "{content}"
            );
            for language in [None, Some("text")] {
                let derived = derive(&content, language);
                assert_eq!(derived.kind, PasteKind::Document, "{content}");
            }
        }
    }
}

#[test]
fn common_git_commands_keep_code_kind_and_handles() {
    for (content, handle) in [
        ("git rev-parse HEAD", "git rev-parse"),
        ("git blame README.md", "git blame"),
        ("cd repo\ngit submodule update", "cd repo"),
    ] {
        for content in std::iter::once(content.to_owned()).chain(
            content
                .contains('\n')
                .then(|| content.replace('\n', "\r\n")),
        ) {
            for language in [None, Some("text")] {
                let derived = derive(&content, language);
                assert_eq!(derived.kind, PasteKind::Code, "{content}");
                assert_eq!(derived.handle.as_deref(), Some(handle), "{content}");
            }
        }
    }
}

#[test]
fn derive_matrix_covers_code_config_log_link_and_other() {
    for content in ["", " \t\r\n"] {
        for language in [
            None,
            Some("text"),
            Some("python"),
            Some("markdown"),
            Some("md"),
            Some("rst"),
            Some("latex"),
        ] {
            let derived = derive(content, language);
            assert_eq!(
                derived.kind,
                if super::is_document_language(language) {
                    PasteKind::Document
                } else {
                    PasteKind::Other
                }
            );
            assert!(derived.handle.is_none());
            assert!(derived.terms.is_empty());
        }
    }
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
        ("[INFO] Server started", "markdown", PasteKind::Log),
        (
            "# Log notes\n\nThe server reported:\n\n[INFO] Server started",
            "markdown",
            PasteKind::Document,
        ),
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
