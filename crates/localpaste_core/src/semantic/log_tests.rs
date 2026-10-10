//! Structure-based log recognition regressions.

use super::{derive, PasteKind};
use crate::detection::detect_language;
use crate::models::paste::{Paste, PasteMeta};

fn with_crlf(content: &str) -> [String; 2] {
    [content.to_owned(), content.replace('\n', "\r\n")]
}

fn stored_kind(content: &str) -> PasteKind {
    PasteMeta::from(&Paste::new(content.to_owned(), "fixture".into()))
        .derived
        .kind
}

#[test]
fn timestamp_led_lines_are_logs() {
    for content in [
        "2026-10-10T06:00:01Z INFO worker-3 request id=000001 path=/api/paste/1 took 12ms\n2026-10-10T06:00:02Z WARN worker-2 request id=000002 path=/api/paste/2 took 812ms",
        "2026-10-10T06:00:01+02:00 INFO starting scheduler\n2026-10-10T06:00:03-05:00 WARN job 7 is late",
        "2026-10-10 06:00:01.123 INFO starting scheduler\n2026-10-10 06:00:02.456 ERROR scheduler crashed",
        "2026-10-10 06:00:01,123 - app - INFO - Starting\n2026-10-10 06:00:02,456 - app - ERROR - Failed to connect",
        "2026-10-10 06:00:01 starting scheduler\n2026-10-10 06:00:02 loaded 12 jobs",
        // Python logging and log4j print milliseconds after a comma.
        "2026-10-10 06:00:01,423 INFO msg",
        "[2026-10-10 06:00:01,423] INFO starting scheduler\n[2026-10-10 06:00:02,117] ERROR scheduler crashed",
        "[2026-10-10 06:00:01] INFO starting scheduler\n[2026-10-10 06:00:02] ERROR scheduler crashed",
        "2026-10-10 06:00:01 [INFO] starting scheduler\n2026-10-10 06:00:02 [ERROR] scheduler crashed",
        "2026-10-10 06:00:01  INFO  starting scheduler\n2026-10-10 06:00:02 ERROR  scheduler crashed",
        "[2026-10-06T12:00:01Z INFO  localpaste] starting\n[2026-10-06T12:00:02Z ERROR localpaste] failed to bind",
        "Oct 10 06:00:01 web01 nginx[1234]: started\nOct  9 23:59:59 web01 CRON[4321]: (root) CMD (run-parts /etc/cron.hourly)",
        "2026-10-10 06:00:01 ERROR request failed\nTraceback (most recent call last):\n  File \"app.py\", line 3, in <module>\nValueError: boom\n2026-10-10 06:00:02 INFO retrying",
        "=== run 42 ===\n2026-10-10 06:00:01 INFO starting\n2026-10-10 06:00:02 INFO step 1 done\n2026-10-10 06:00:03 ERROR step 2 failed",
        "2026-10-10T06:00:01Z INFO worker-3 request id=000001 path=/api/paste/1 took 12ms",
    ] {
        for content in with_crlf(content) {
            assert_eq!(derive(&content, None).kind, PasteKind::Log, "{content}");
            assert_eq!(detect_language(&content).as_deref(), Some("log"), "{content}");
            assert_eq!(stored_kind(&content), PasteKind::Log, "{content}");
            let fence = format!("```text\n{content}\n```");
            assert_eq!(derive(&fence, Some("markdown")).kind, PasteKind::Log, "{fence}");
        }
    }
    let large: String = (0..6_000)
        .map(|i| {
            format!(
                "2026-10-10T06:{:02}:{:02}Z INFO worker-{} request id={i:06} path=/api/paste/{i} took {}ms\n",
                (i / 60) % 60,
                i % 60,
                i % 4,
                i % 97
            )
        })
        .collect();
    assert_eq!(derive(&large, None).kind, PasteKind::Log);
    assert_eq!(stored_kind(&large), PasteKind::Log);
}

#[test]
fn delimited_records_and_dated_prose_are_not_timestamp_logs() {
    for content in [
        "2026-10-10T06:00:01Z,worker-3,12\n2026-10-10T06:00:02Z,worker-1,9",
        "timestamp,worker,latency_ms\n2026-10-10 06:00:01,worker-3,12\n2026-10-10 06:00:02,worker-1,9",
        "2026-10-10 06:00:01\tworker-3\t12\n2026-10-10 06:00:02\tworker-1\t9",
        // A numeric second column is a field, not comma milliseconds.
        "ts,count\n2026-10-10 06:00:01,42\n2026-10-10 06:00:02,17",
        "ts,count\n2026-10-10 06:00:01,423\n2026-10-10 06:00:02,117",
        "2026-10-10 06:00:01,42\n2026-10-10 06:00:02,17",
        "2026-10-10 06:00:01,423,ok\n2026-10-10 06:00:02,117,retry",
        "date,amount,description\n2026-10-01,12.50,coffee beans\n2026-10-02,40.00,groceries",
        "2026-10-10 06:00 meeting with Sam about the roadmap was moved to Friday.",
        "On 2026-10-10 at 06:00:00 the deploy started, and by 06:30 everything was green.",
        "2026-10-09\nLong day. Finally fixed the build and went for a run.\n\n2026-10-10\nSlept in.",
        "2026-10-10 buy milk\n2026-10-11 call the dentist",
        "2026-10-10 06:00:01 started the job\nThe job took forever and nobody noticed.",
    ] {
        for content in with_crlf(content) {
            assert_ne!(derive(&content, None).kind, PasteKind::Log, "{content}");
            assert_ne!(derive(&content, Some("csv")).kind, PasteKind::Log, "{content}");
            assert_ne!(detect_language(&content).as_deref(), Some("log"), "{content}");
            assert_ne!(stored_kind(&content), PasteKind::Log, "{content}");
        }
    }
    let table = "ts,count\n2026-10-10 06:00:01,42\n2026-10-10 06:00:02,17";
    for language in [None, Some("csv")] {
        assert_eq!(
            derive(table, language).kind,
            PasteKind::Other,
            "{language:?}"
        );
    }
}

#[test]
fn prose_about_failures_is_not_a_log() {
    for content in [
        "The panic in the auth service was caused by a missing config key; the stack trace pointed at the token parser.",
        "Postmortem summary\n\nAt 02:14 the API began returning 500s. The panic in the auth service was caused by a missing config key. The stack trace pointed at the token parser.",
        "```text\nThe panic last night came from the stack trace we saw in staging.\n```",
        "```text\nWe burned through our ERROR budget this week, and the exception report says stdout was empty.\n```",
        "debug Safari crash on login\ntrace Network calls in staging",
        "info Desk opens at 9 on weekdays\nerror Rate dropped after the fix",
        "debug: Find and fix defects in code.\ntrace: Record of program execution.\nerror: A mistake in the program.",
    ] {
        for content in with_crlf(content) {
            for language in [None, Some("text"), Some("markdown")] {
                assert_eq!(
                    derive(&content, language).kind,
                    PasteKind::Document,
                    "{content}: {language:?}"
                );
            }
            assert_eq!(stored_kind(&content), PasteKind::Document, "{content}");
        }
    }
    let flags = "debug: True\ntrace: False";
    assert_eq!(detect_language(flags).as_deref(), Some("yaml"));
    assert_eq!(derive(flags, Some("yaml")).kind, PasteKind::Config);
    assert_eq!(stored_kind(flags), PasteKind::Config);
    assert_eq!(
        derive(
            "info: Service metadata\ndebug: False\nport: 8080",
            Some("yaml")
        )
        .kind,
        PasteKind::Config
    );
}

#[test]
fn level_keyed_mappings_stay_config() {
    for content in [
        "error: /var/log/app/error.log\ninfo: /var/log/app/info.log",
        "debug: \"true\"\ninfo: v1.2.3\nport: 8080",
        "error: \"Something went wrong, please try again.\"\ninfo: \"Your changes were saved.\"",
    ] {
        for content in with_crlf(content) {
            assert!(!super::logs::looks_like_runtime_log(&content), "{content}");
            assert_ne!(
                detect_language(&content).as_deref(),
                Some("log"),
                "{content}"
            );
            assert_eq!(
                derive(&content, Some("yaml")).kind,
                PasteKind::Config,
                "{content}"
            );
        }
    }
    let output =
        "error: unable to open database file: /home/me/.cache/x\nwarning: unused import `std::io`";
    assert_eq!(derive(output, Some("yaml")).kind, PasteKind::Log);
}

#[test]
fn tool_output_is_recognized_by_its_line_format() {
    for content in [
        "npm ERR! code ERESOLVE\nnpm ERR! ERESOLVE unable to resolve dependency tree\nnpm ERR! While resolving: demo@1.0.0",
        "npm WARN deprecated inflight@1.0.6: This module is not supported, and leaks memory.\nnpm WARN deprecated glob@7.2.3: Glob versions prior to v9 are no longer supported",
        "npm notice\nnpm notice New major version of npm available! 9.8.1 -> 10.2.0\nnpm notice Run npm install -g npm@10.2.0 to update!",
        "info No lockfile found.\nwarning package.json: No license field\ninfo Fetching packages...",
        "yarn install v1.22.19\ninfo No lockfile found.\n[1/4] Resolving packages...\n[2/4] Fetching packages...\nsuccess Saved lockfile.\nDone in 3.21s.",
        "INFO:root:Starting\nWARNING:root:Low disk",
        "    Updating crates.io index\n  Downloaded serde v1.0.200\n   Compiling demo v0.1.0 (/home/me/demo)\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.12s\n     Running `target/debug/demo`\nthread 'main' panicked at src/main.rs:4:5:\nboom",
        "error[E0308]: mismatched types\n --> src/main.rs:2:18\n  |\n2 |     let x: i32 = \"a\";\n  |                  ^^^ expected `i32`, found `&str`",
        "Exception in thread \"main\" java.lang.NullPointerException: s is null\n\tat com.example.App.run(App.java:14)\n\tat com.example.App.main(App.java:5)",
        "panic: runtime error: index out of range [5] with length 3\n\ngoroutine 1 [running]:\nmain.main()\n\t/tmp/main.go:8 +0x1d\nexit status 2",
    ] {
        for content in with_crlf(content) {
            assert_eq!(derive(&content, None).kind, PasteKind::Log, "{content}");
            assert_eq!(stored_kind(&content), PasteKind::Log, "{content}");
        }
    }
}
