//! Structure-based log recognition regressions.

use super::{derive, PasteKind};
use crate::detection::detect_language;
use crate::models::paste::{Paste, PasteMeta};
use crate::test_support::{assert_derived_kinds, kind_unless_short};

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
        // To-do items name files and versions as often as output does.
        "debug crash in auth/login.rs\ndebug memory leak in parser.rs",
        "info bump the version in package.json\ninfo update docs for v1.2",
    ] {
        for content in with_crlf(content) {
            for language in [None, Some("text"), Some("markdown")] {
                assert_eq!(
                    derive(&content, language).kind,
                    kind_unless_short(&content, language, PasteKind::Document),
                    "{content}: {language:?}"
                );
            }
            assert_eq!(stored_kind(&content),
                kind_unless_short(&content, None, PasteKind::Document),
                "{content}"
            );
        }
    }
    let flags = "debug: True\ntrace: False";
    assert_eq!(detect_language(flags).as_deref(), Some("yaml"));
    assert_eq!(derive(flags, Some("yaml")).kind, PasteKind::Config);
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
        // Multi-word values with versions and paths are still mapping values.
        "info: version 1.2\nwarn: deprecated in 2.0",
        "info: listening on port 8080 by default\nwarn: deprecated since v2.0, see docs/migrate.md",
        "info: \"Loading...\"\nerror: \"Retrying...\"",
    ] {
        for content in with_crlf(content) {
            assert!(!super::logs::looks_like_runtime_log(&content), "{content}");
            assert_ne!(
                detect_language(&content).as_deref(),
                Some("log"),
                "{content}"
            );
            assert_ne!(stored_kind(&content), PasteKind::Log, "{content}");
            for language in ["yaml", "toml", "json"] {
                assert_eq!(
                    derive(&content, Some(language)).kind,
                    PasteKind::Config,
                    "{content}: {language}"
                );
            }
        }
    }
    // A nested `label: ` or a backtick code span cannot be a mapping value.
    for output in [
        "error: unable to open database file: /home/me/.cache/x\nwarning: unused import `std::io`",
        "error: could not compile `demo` due to 2 previous errors\nwarning: unused variable `x`",
        "fatal: not a git repository (or any of the parent directories): .git\nerror: pathspec did not match: src/",
    ] {
        for output in with_crlf(output) {
            assert!(super::logs::looks_like_runtime_log(&output), "{output}");
            assert_eq!(detect_language(&output).as_deref(), Some("log"), "{output}");
            for language in [None, Some("yaml")] {
                assert_eq!(derive(&output, language).kind, PasteKind::Log, "{output}");
            }
        }
    }
    // Lowercase levels after a timestamp are output, as Go and Node loggers print them.
    let stamped =
        "2026-10-10T06:00:01Z info: server listening on :8080\n2026-10-10T06:00:02Z warn: slow request took 812ms";
    assert_eq!(detect_language(stamped).as_deref(), Some("log"));
    assert_eq!(derive(stamped, Some("yaml")).kind, PasteKind::Log);
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

#[test]
fn crash_output_is_a_log_wherever_the_traceback_starts() {
    let crashes = [
        "Epoch 1/10\nloss: 0.53\nTraceback (most recent call last):\n  File \"train.py\", line 3, in <module>\n    main()\nRuntimeError: CUDA out of memory.",
        "Traceback (most recent call last):\n  File \"app.py\", line 3, in <module>\n    main()\nValueError: boom\n\nProcess finished with exit code 1",
        "$ python train.py\nTraceback (most recent call last):\n  File \"train.py\", line 3, in <module>\n    main()\nRuntimeError: CUDA out of memory.",
        "Loading data from /data/train\nUserWarning: You are using the default collate function.\nTraceback (most recent call last):\n  File \"train.py\", line 9, in <module>\n    fit()\nKeyError: 'label'\nwandb: Waiting for W&B process to finish... (failed 1)",
        "Exception in thread Thread-1:\nTraceback (most recent call last):\n  File \"worker.py\", line 4, in run\n    step()\nValueError: boom\n\nDuring handling of the above exception, another exception occurred:\n\nTraceback (most recent call last):\n  File \"worker.py\", line 6, in run\n    cleanup()\nOSError: disk full",
        "$ ./server --port 8080\nSegmentation fault (core dumped)",
        "Starting server on port 8080\nLoaded 12 plugins\nSegmentation fault (core dumped)",
        "Segmentation fault (core dumped)",
        "./bench\nSegmentation fault: 11",
        "zsh: segmentation fault  ./a.out",
        "make: *** [Makefile:5: run] Segmentation fault (core dumped)",
        "Epoch 3/10\nKilled",
        "$ cargo run\nexit status 101",
        // A frame's source line belongs to the traceback, not to source around it.
        "Traceback (most recent call last):\n  File \"train.py\", line 1, in <module>\n    import torch\nModuleNotFoundError: No module named 'torch'",
        // `>>> ` is the interpreter's prompt, like `$ `: a session that ends in a
        // traceback is crash output, and code typed at the prompt is not source.
        ">>> 1/0\nTraceback (most recent call last):\n  File \"<stdin>\", line 1, in <module>\nZeroDivisionError: division by zero",
        ">>> import json\n>>> json.loads(\"{\")\nTraceback (most recent call last):\n  File \"<stdin>\", line 1, in <module>\njson.decoder.JSONDecodeError: Expecting value: line 1 column 2 (char 1)",
    ];
    assert_derived_kinds(
        &crashes,
        &[None, Some("text"), Some("python"), Some("shell")],
        PasteKind::Log,
    );
    for content in crashes.iter().flat_map(|content| with_crlf(content)) {
        assert_eq!(
            detect_language(&content).as_deref(),
            Some("log"),
            "{content}"
        );
        assert_eq!(derive(&content, Some("log")).kind, PasteKind::Log);
    }
}

/// A service log whose last lines are a Rust panic and its backtrace.
pub(super) const PANIC_AFTER_SERVICE_LOG: &str =
    "2026-10-09T14:02:11.384Z  INFO checkout::api: POST /v1/orders 201 in 41ms
2026-10-09T14:02:11.902Z  WARN checkout::payments: provider latency 2.8s exceeds budget (1.5s)
2026-10-09T14:02:12.117Z ERROR checkout::payments: charge failed: connection reset by peer
thread 'tokio-runtime-worker' panicked at src/payments/client.rs:212:38:
called `Option::unwrap()` on a `None` value
stack backtrace:
   0: rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::panicking::panic
   3: core::option::unwrap_failed
   4: checkout::payments::client::Client::capture
             at ./src/payments/client.rs:212:38
   5: checkout::api::orders::create::{{closure}}
             at ./src/api/orders.rs:87:22
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.";

#[test]
fn rust_panic_output_is_a_log_wherever_the_panic_starts() {
    let outputs = [
        PANIC_AFTER_SERVICE_LOG,
        // Program output before the panic and Cargo's exit report after it.
        "Listening on 0.0.0.0:8080\nthread 'main' panicked at src/main.rs:12:5:\ncalled `Result::unwrap()` on an `Err` value: AddrInUse\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\nerror: process didn't exit successfully: `target/debug/server` (exit status: 101)",
        // A failing `cargo test` run.
        "running 1 test\ntest tests::adds ... FAILED\n\nfailures:\n\n---- tests::adds stdout ----\nthread 'tests::adds' panicked at src/lib.rs:10:9:\nassertion `left == right` failed\n  left: 4\n right: 5\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\n\nfailures:\n    tests::adds\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
        // Rust before 1.73 printed the message inside the header.
        "starting worker pool\nthread 'worker-1' panicked at 'queue closed', src/worker.rs:40:9\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
        // A panic that leads the paste is a log as before.
        "thread 'main' panicked at src/main.rs:4:5:\nconfig file missing\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
    ];
    assert_derived_kinds(
        &outputs,
        &[
            None,
            Some("text"),
            Some("yaml"),
            Some("markdown"),
            Some("rust"),
        ],
        PasteKind::Log,
    );
    for content in outputs.iter().flat_map(|content| with_crlf(content)) {
        assert!(super::logs::looks_like_runtime_log(&content), "{content}");
        assert_eq!(
            detect_language(&content).as_deref(),
            Some("log"),
            "{content}"
        );
        assert_eq!(stored_kind(&content), PasteKind::Log, "{content}");
    }
}

#[test]
fn notes_about_crashes_stay_documents() {
    let notes = [
        "The traceback showed a segmentation fault in the parser, so we rolled back.",
        "Note to self: check for a segmentation fault before the release.",
        "Killed the old worker and restarted the queue.",
        "Process finished with exit code 0 is what we want to see.",
        "Getting this error when I run train.py:\n\nTraceback (most recent call last):\n  File \"train.py\", line 3, in <module>\n    main()\nRuntimeError: CUDA out of memory.\n\nAny idea how to fix it?",
        "We hit a segfault again today.\nSegmentation fault (core dumped)\nAny ideas?",
        "Traceback (most recent call last):\n  File \"app.py\", line 3, in <module>\n    main()\nValueError: boom\nThis happens every time the batch size exceeds the limit.",
        "The worker crashed right after loading the config:\n\n    Traceback (most recent call last):\n      File \"worker.py\", line 12, in <module>\n        main()\n    KeyError: 'queue'\n\nAdding a default queue name fixed it.",
        "## Crash on startup\n\nThe worker crashed right after loading the config:\n\n    Traceback (most recent call last):\n      File \"worker.py\", line 12, in <module>\n        main()\n    KeyError: 'queue'\n\nAdding a default queue name fixed it.",
        "Our checkout service crashed in production last night:\n\nthread 'tokio-runtime-worker' panicked at src/payments/client.rs:212:38:\ncalled `Option::unwrap()` on a `None` value\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\nIt looks like the provider returned no charge id. Can we handle that case?",
    ];
    assert_derived_kinds(&notes, &[None, Some("text")], PasteKind::Document);
    assert!(notes
        .iter()
        .flat_map(|note| with_crlf(note))
        .all(|note| !super::logs::looks_like_runtime_log(&note)
            && detect_language(&note).as_deref() != Some("log")));
    for note in notes.iter().flat_map(|note| with_crlf(note)) {
        assert_eq!(
            stored_kind(&note),
            kind_unless_short(&note, None, PasteKind::Document),
            "{note}"
        );
    }
}

#[test]
fn source_around_a_traceback_stays_code() {
    let sources = [
        (
            "def div(a, b):\n    \"\"\"\n    >>> div(1, 0)\n    Traceback (most recent call last):\n      File \"<stdin>\", line 1, in <module>\n    ZeroDivisionError: division by zero\n    \"\"\"\n    return a / b",
            "def div",
        ),
        (
            "import textwrap\n\nEXPECTED = \"\"\"\\\nTraceback (most recent call last):\n  File \"app.py\", line 3, in <module>\n    main()\nValueError: boom\n\"\"\"\n\n\ndef test_crash_prints_traceback(capfd):\n    run_app()\n    assert capfd.readouterr().err == EXPECTED",
            "def test_crash_prints_traceback",
        ),
        (
            "def test_traceback_is_printed(run):\n    result = run(\"app.py\")\n    assert result.stderr == textwrap.dedent(\"\"\"\n        Traceback (most recent call last):\n          File \"app.py\", line 3, in <module>\n            main()\n        ValueError: boom\n    \"\"\").lstrip()",
            "def test_traceback_is_printed",
        ),
    ];
    let bodies: Vec<&str> = sources.iter().map(|(body, _)| *body).collect();
    assert_derived_kinds(&bodies, &[Some("python")], PasteKind::Code);
    for (body, handle) in sources {
        for body in with_crlf(body) {
            assert!(!super::logs::looks_like_runtime_log(&body), "{body}");
            for language in [None, Some("text")] {
                assert_ne!(derive(&body, language).kind, PasteKind::Log, "{body}");
            }
            let paste = Paste::new(body.clone(), "source".into());
            assert_eq!(paste.language.as_deref(), Some("python"), "{body}");
            let derived = PasteMeta::from(&paste).derived;
            assert_eq!(derived.kind, PasteKind::Code, "{body}");
            assert_eq!(derived.handle.as_deref(), Some(handle), "{body}");
        }
    }
    // Rust source around a whole panic block: a program followed by the
    // output it prints, and a test that compares stderr with that output.
    let rust_sources = [
        "fn main() {\n    let port = 8080;\n    panic!(\"port {port} is taken\");\n}\nthread 'main' panicked at src/main.rs:3:5:\nport 8080 is taken\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
        "const EXPECTED: &str = \"\\\nthread 'main' panicked at src/main.rs:3:5:\nport 8080 is taken\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n\";\n\n#[test]\nfn taken_port_panics_with_its_number() {\n    let output = run_server(&[\"--port\", \"8080\"]);\n    assert_eq!(String::from_utf8_lossy(&output.stderr), EXPECTED);\n}",
    ];
    assert_derived_kinds(&rust_sources, &[Some("rust")], PasteKind::Code);
    for body in rust_sources.iter().flat_map(|body| with_crlf(body)) {
        assert!(!super::logs::looks_like_runtime_log(&body), "{body}");
        assert_ne!(detect_language(&body).as_deref(), Some("log"), "{body}");
        assert_ne!(stored_kind(&body), PasteKind::Log, "{body}");
    }
}
