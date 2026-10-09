//! Detection module tests for canonicalization, fallback heuristics, and Magika integration.

use super::canonical::canonicalize;
use super::detect_language;
use super::heuristic;
use super::looks_like_flat_config_yaml;
use super::looks_like_yaml;
use super::refine_magika_label;

#[test]
fn review_regression_commented_python_and_unlabeled_fences() {
    use crate::models::paste::{Paste, PasteMeta};
    use crate::semantic::PasteKind;

    for source in [
        "from flask import Flask\napp = Flask(__name__)\n# routes\n@app.route('/')\ndef index():\n    return 'hello'",
        "from a import b\n# helper\ndef f():\n    return b()",
        "from dataclasses import dataclass\n\n# A point\n@dataclass\nclass Point:\n    x: float\n    y: float",
        "import os\n# Load paths\nroot = os.getcwd()\nprint(root)",
        "import os, sys\n# Platform info\nprint(os.name)\nprint(sys.version)",
        "import os  # operating system\n# Platform info\nprint(os.name)",
        "import os; import sys\n# Platform info\nprint(os.name)",
        "# Imports\nimport os, \\\n    sys\nprint(os.name)",
        "# Imports\nimport os,\n",
        "import os; x = 1\n# Value\nprint(x)",
        "root = '.'\nfrom pathlib import Path\n# Walk paths\nfor path in Path(root).glob('*.txt'):\n    print(path)",
        "from asyncio import sleep\n# Run task\nasync def run():\n    await sleep(1)\n    return 'done'",
        "from pathlib import Path\n# Read configuration\nconfig = load_config(\n    Path('settings.json'),\n    defaults={'mode': 'dev'},\n)",
        "from collections import defaultdict\n# Process rows\ntry:\n    rows = defaultdict(list)\n    rows['key'].append(1)\nexcept ValueError as exc:\n    raise RuntimeError('bad row') from exc",
        "from typing import Iterable\n# Typed values\nvalues: Iterable[int] = [\n    1,\n    2,\n]\ncache['values'] = values",
        "from pathlib import (\n    Path,\n    PurePath,\n)\n# Choose path\nroot = Path('.')",
        "from pathlib import Path\n# Document source\n\"\"\"Load the configured source directory.\nThis paragraph describes the module behavior.\n\"\"\"\nroot = Path('.')",
        "from math import sqrt\n# Unicode values\nα = sqrt(4)\n结果 = α + 1",
        "from pathlib import Path\n# Partial call\nroot = Path(\n    'unfinished',",
        "from pathlib import Path\n# Partial function\ndef read_config(",
        "\"\"\"Utilities for loading a configuration.\nThis documentation belongs to the Python module.\n\"\"\"\nfrom pathlib import Path\n# Load root\nroot = Path('.')",
        "from flags import enabled\n# Compare flags\nenabled is not None\nassert enabled is not None",
        "from totals import total, discount\n# Calculate remaining\ntotal - discount",
    ] {
        for source in [source.to_owned(), source.replace('\n', "\r\n")] {
            assert_eq!(heuristic::detect(&source).as_deref(), Some("python"), "{source}");
            assert_eq!(refine_magika_label("markdown", &source).as_deref(), Some("python"));
            let paste = Paste::new(source.clone(), "Python source".into());
            assert_eq!(paste.language.as_deref(), Some("python"), "{source}");
            assert!(paste.language_is_manual);
            assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Code);
            assert_eq!(super::preferred_extension(paste.language.as_deref()), "py");
            let fence = Paste::new(format!("```\n{source}\n```"), "Python fence".into());
            assert_eq!(fence.language.as_deref(), Some("markdown"));
            assert_eq!(PasteMeta::from(&fence).derived.kind, PasteKind::Code, "{source}");
        }
    }
    for source in [
        "# greeting\nprint('hello')",
        "# Values\nx = 1\nprint(x)",
        "# Compare\nenabled is not None",
        "# Index\n[Reference][1]\nfrom values import Reference",
    ] {
        for source in [source.to_owned(), source.replace('\n', "\r\n")] {
            assert_eq!(
                refine_magika_label("python", &source).as_deref(),
                Some("python"),
                "{source}"
            );
        }
    }
}

#[test]
fn review_regression_shell_control_flow_and_python_tracebacks() {
    use crate::models::paste::{Paste, PasteMeta};
    use crate::semantic::PasteKind;

    for (content, language, kind) in [
        ("# build all\nfor f in *.c; do cc \"$f\"; done", "shell", PasteKind::Code),
        ("# build all\nfor f in *.c\ndo\n    cc \"$f\"\ndone", "shell", PasteKind::Code),
        ("# setup\nsource .env", "shell", PasteKind::Code),
        ("# install\npip uninstall old-package", "shell", PasteKind::Code),
        ("# install\nnpm ci", "shell", PasteKind::Code),
        ("# deps\nnpm i", "shell", PasteKind::Code),
        ("# run tests\npytest", "shell", PasteKind::Code),
        (
            "# fetch the page\ncurl https://example.com",
            "shell",
            PasteKind::Code,
        ),
        ("# connect\nssh user@host", "shell", PasteKind::Code),
        ("# containers\ndocker ps", "shell", PasteKind::Code),
        ("# deps\npip freeze", "shell", PasteKind::Code),
        ("# deps\nnpm ls", "shell", PasteKind::Code),
        ("# run tests\npytest tests", "shell", PasteKind::Code),
        ("# containers\ndocker images", "shell", PasteKind::Code),
        ("# fetch the page\nwget https://example.com", "shell", PasteKind::Code),
        ("# setup\nmkdir out", "shell", PasteKind::Code),
        ("# build\nmake all", "shell", PasteKind::Code),
        ("# build\njust build", "shell", PasteKind::Code),
        ("Traceback (most recent call last):\n  File \"demo.py\", line 1, in <module>\n    int('bad')\n> ValueError: invalid literal for int()", "log", PasteKind::Log),
        ("stderr:\nTraceback (most recent call last):\n  File \"demo.py\", line 1, in <module>\n    raise ValueError()\n> ValueError", "log", PasteKind::Log),
        ("Traceback (most recent call last):\n  File \"demo.py\", line 1, in <module>\n    fetch()\n> requests.exceptions.HTTPError: failed request", "log", PasteKind::Log),
    ] {
        for content in [content.to_owned(), content.replace('\n', "\r\n")] {
            assert_eq!(heuristic::detect(&content).as_deref(), Some(language), "{content}");
            assert_eq!(refine_magika_label("markdown", &content).as_deref(), Some(language));
            let paste = Paste::new(content.clone(), "runtime fixture".into());
            assert_eq!(paste.language.as_deref(), Some(language), "{content}");
            assert_eq!(PasteMeta::from(&paste).derived.kind, kind);
            let fence = Paste::new(format!("```\n{content}\n```"), "runtime fence".into());
            assert_eq!(PasteMeta::from(&fence).derived.kind, kind, "{content}");
        }
    }
}

#[test]
fn review_regression_prose_imports_and_command_wrappers_stay_documents() {
    use crate::models::paste::{Paste, PasteMeta};
    use crate::semantic::PasteKind;

    for content in [
        "> python rocks",
        "> pytest",
        "> curl up by the fireplace",
        "# Todo\nls",
        "# Todo\ncurl up by the fireplace",
        "# Notes\npytest is useful",
        "# Notes\nmake sure to call mom",
        "# references\n[api]: https://example.com\n\t\"API reference\"",
        "# Tasks\nMeeting notes:\n\tDiscuss timeline tomorrow",
        "# Meeting agenda\nTopics:\n\tReview project status",
        "Reminders\nimport taxes\ncall mom",
        "Reminders\nimport taxes; call mom",
        "Reminders\nimport taxes; call mom tomorrow",
        "import os; this is a note\n# check\nprint(os.name)",
        "# Reminders\nimport taxes\ncall mom",
        "# Python notes\n\nThe example below reads a file.\nfrom pathlib import Path\nprint(Path('note.txt').read_text())",
        "# Loop notes\n\nThe script loops over source files.\nfor f in *.c; do cc \"$f\"; done",
        "# Error notes\nTraceback (most recent call last):\n  File \"demo.py\", line 1, in <module>\n> ValueError: bad value",
        "# Python notes\nfrom pathlib import Path\nThis paragraph explains how the example works.",
        "# Python notes\nfrom pathlib import Path\nThis is a prose explanation of the code.",
        "# Python notes\nfrom pathlib import Path\nThis paragraph explains the code at https://example.com/docs.",
        "# Source notes\nRead the path from the setting.\nfrom pathlib import Path\nroot = Path('.')",
        "# Import notes\n- Read the input first\nfrom pathlib import Path\nroot = Path('.')",
        "# Python notes\nfrom pathlib import Path\n1. Read the input.\n2. Write the output.",
        "# Python notes\nfrom pathlib import Path\n1) Read the input.\n2) Write the output.",
        "# Python notes\nfrom pathlib import Path\n[Reference][1]\n\n[1]: https://example.com/docs",
    ].into_iter().flat_map(|content| [content.to_owned(), content.replace('\n', "\r\n")]) {
        assert!(!super::looks_like_shell_command_sequence(&content), "{content}");
        assert!(!matches!(heuristic::detect(&content).as_deref(), Some("shell" | "python")), "{content}");
        let paste = Paste::new(content.clone(), "ordinary note".into());
        assert_eq!(paste.content, content);
        assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Document, "{content}: {:?}", paste.language);
    }
    for content in [
        "# Tasks\nMeeting notes:\n\tDiscuss timeline tomorrow",
        "# Meeting agenda\nTopics:\n\tReview project status",
    ] {
        assert_eq!(
            refine_magika_label("makefile", content).as_deref(),
            Some("markdown"),
            "{content}"
        );
    }
    let real_makefiles = [
        "# Makefile for app\nCC = gcc\napp: main.c\n\t$(CC) main.c -o app",
        "# Project tasks\ninclude config.mk\nall:\n\t$(MAKE) build",
        "## Variables\nPREFIX ?= /usr/local\ninstall:\n\tmkdir -p $(PREFIX)/bin",
        "# OS detection\nifeq ($(OS),Windows_NT)\nEXE := .exe\nelse\nEXE :=\nendif\napp:\n\t$(CC) main.c -o app$(EXE)",
    ];
    for content in [
        "# build\nall: main.c\n\tcc main.c",
        "# build\nall: main.c\n\t$(CC) main.c",
    ]
    .into_iter()
    .chain(real_makefiles)
    {
        assert_eq!(
            refine_magika_label("makefile", content).as_deref(),
            Some("makefile"),
            "{content}"
        );
    }
    #[cfg(feature = "magika")]
    for content in real_makefiles {
        let paste = Paste::new(content.to_owned(), "makefile regression".into());
        assert_eq!(paste.language.as_deref(), Some("makefile"), "{content}");
        assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Config);
    }
    for content in [
        "# Python notes\n\nThe example below reads a file.\nfrom pathlib import Path\nprint(Path('note.txt').read_text())",
        "# Python notes\nfrom pathlib import Path\n1. Read the input.\n2. Write the output.",
        "# Python notes\nfrom pathlib import Path\n1) Read the input.\n2) Write the output.",
        "# Python notes\nfrom pathlib import Path\n[Reference][1]\n\n[1]: https://example.com/docs",
    ] {
        assert_eq!(refine_magika_label("python", content), None, "{content}");
    }
}

#[test]
fn comment_prefixed_commands_and_log_wrappers_avoid_markdown_locks() {
    let python_data = "# Compute things\nimport numpy as np\nprint('[label](url)')";
    for content in [python_data.to_owned(), python_data.replace('\n', "\r\n")] {
        assert_eq!(
            refine_magika_label("markdown", &content).as_deref(),
            Some("python")
        );
        assert_eq!(heuristic::detect(&content).as_deref(), Some("python"));
    }
    for (content, language) in [
        ("# install\npip install foo", "shell"),
        ("# run it\ndocker run -it --rm ubuntu bash", "shell"),
        ("# update\nrustup update stable", "shell"),
        ("# install\n$ pip install foo bar", "shell"),
        ("# install\nprintf '[label](url)'", "shell"),
        ("stderr:\n> error: failed\nexit code 1", "log"),
        ("stderr:\n> error: [label](url)\nexit code 1", "log"),
    ] {
        for content in [content.to_owned(), content.replace('\n', "\r\n")] {
            assert_eq!(
                detect_language(&content).as_deref(),
                Some(language),
                "{content}"
            );
            assert_eq!(
                refine_magika_label("markdown", &content).as_deref(),
                Some(language)
            );
            let paste = crate::models::paste::Paste::new(content, "review technical body".into());
            assert_eq!(paste.language.as_deref(), Some(language));
            assert!(paste.language_is_manual);
        }
    }
    for content in [
        "# install\napt install -y git curl",
        "# build\nFOO=bar cargo build --release",
        "# install\n> apt install -y git curl",
        "# build\n> FOO=bar cargo build --release",
    ] {
        for content in [content.to_owned(), content.replace('\n', "\r\n")] {
            assert_eq!(refine_magika_label("markdown", &content), None, "{content}");
            assert_ne!(
                heuristic::detect(&content).as_deref(),
                Some("markdown"),
                "{content}"
            );
            let paste = crate::models::paste::Paste::new(content, "review technical body".into());
            assert_ne!(
                crate::models::paste::PasteMeta::from(&paste).derived.kind,
                crate::semantic::PasteKind::Document
            );
        }
    }
    for content in [
        "# Notes\n\nThis is ordinary explanatory prose.",
        "> This is a quoted note for tomorrow.",
        "# Notes\n> This is quoted prose about /usr/bin tools.",
        "# Notes\n\n```shell\npip install foo\n```",
        "# Notes\nimport numpy as np\nrelease details",
        "# Notes\n[label](url)",
    ] {
        assert_eq!(
            refine_magika_label("markdown", content).as_deref(),
            Some("markdown")
        );
        assert_eq!(heuristic::detect(content).as_deref(), Some("markdown"));
    }
}

#[test]
fn shell_sequence_sampling_drops_only_a_cut_final_line() {
    for ending in ["\n", "\r\n"] {
        for offset in 0..20 {
            let long_row = format!(
                "# install{ending}echo {}{ending}{}",
                "x".repeat(65_495 + offset),
                format!("git push origin main{ending}").repeat(2)
            );
            assert_eq!(
                heuristic::detect(&long_row).as_deref(),
                Some("shell"),
                "offset {offset} {ending:?}"
            );
            assert_eq!(
                refine_magika_label("markdown", &long_row).as_deref(),
                Some("shell")
            );
            let paste = crate::models::paste::Paste::new(long_row, "long command rows".into());
            assert_eq!(paste.language.as_deref(), Some("shell"));
            assert_eq!(
                crate::models::paste::PasteMeta::from(&paste).derived.kind,
                crate::semantic::PasteKind::Code
            );
        }
        for offset in 0..40 {
            let content = format!(
                "# {}{ending}{}",
                "x".repeat(offset),
                format!("git push origin main{ending}").repeat(4_000)
            );
            assert!(
                super::looks_like_shell_command_sequence(&content),
                "offset {offset} {ending:?}"
            );
            assert_eq!(
                detect_language(&content).as_deref(),
                Some("shell"),
                "offset {offset} {ending:?}"
            );
        }
    }
}

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
            let paste = crate::models::paste::Paste::new(content.clone(), "CMD assignment".into());
            assert_eq!(
                refine_magika_label("batch", &content).as_deref(),
                Some("batch"),
                "{content}"
            );
            assert_eq!(
                crate::models::paste::PasteMeta::from(&paste).derived.kind,
                crate::semantic::PasteKind::Code,
                "{content}"
            );
            if content.starts_with("set my variable=hello world") {
                assert_eq!(paste.language.as_deref(), Some("batch"));
                assert!(paste.language_is_manual);
            }
        }
    }
}

#[cfg(feature = "magika")]
#[test]
fn automatic_setup_word_notes_remain_documents() {
    let pastes: Vec<_> = [
        "set timer for 10 minutes\r\n",
        "source code is at https://example.com\r\n",
    ]
    .into_iter()
    .map(|content| crate::models::paste::Paste::new(content.into(), "review note".into()))
    .collect();
    for paste in pastes {
        assert_eq!(paste.language, None);
        assert!(!paste.language_is_manual);
        assert_eq!(
            crate::models::paste::PasteMeta::from(&paste).derived.kind,
            crate::semantic::PasteKind::Document,
            "{}",
            paste.content
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

fn assert_detection_cases(cases: &[(&str, Option<&str>)]) {
    for (content, expected) in cases {
        assert_eq!(
            detect_language(content).as_deref(),
            *expected,
            "content: {content}"
        );
    }
}

#[test]
fn heuristic_detects_existing_language_matrix() {
    let cases = [
        ("fn main() { let x = 1; }", Some("rust")),
        ("def main():\n    print('hi')", Some("python")),
        ("const x = () => console.log('hi');", Some("javascript")),
        ("#!/bin/bash\necho hello", Some("shell")),
        ("name: app\nservices:\n  - web", Some("yaml")),
        ("name: app\nversion: 1\nport: 8080\n", Some("yaml")),
        ("name: app\nversion: 1\n...\n", Some("yaml")),
        ("name: app", None),
        ("display name: app\nport: 8080", None),
        ("services: [web]\nversion: 3", Some("yaml")),
        (
            "jobs:\n  build:\n    runs-on: ubuntu-latest\n",
            Some("yaml"),
        ),
        ("select id, name from users where active = 1", Some("sql")),
        ("[tool]\nname = \"demo\"\nversion = \"0.1.0\"", Some("toml")),
        ("just some plain text words", None),
    ];
    assert_detection_cases(cases.as_slice());
}

#[test]
fn yaml_shape_helper_handles_flow_values_and_single_list_guard() {
    assert!(looks_like_yaml("root: {child: value}\n"));
    assert!(!looks_like_yaml("display name: api\nport: 8080\n"));
    assert!(looks_like_yaml("services: [web]\nversion: 3\n"));
    assert!(looks_like_yaml(
        "jobs:\n  build:\n    runs-on: ubuntu-latest\n"
    ));
    assert!(looks_like_yaml("script: |\n  echo hi\n"));
    assert!(looks_like_yaml("defaults: &defaults\n  timeout: 30\n"));
    assert!(looks_like_yaml("- name: web\n- name: worker\n"));
    assert!(!looks_like_yaml("- item\n"));
}

#[test]
fn yaml_shape_helper_requires_supporting_structure_for_marker_values() {
    assert!(!looks_like_yaml("pattern: *.glob\nmode: strict\n"));
    assert!(!looks_like_yaml("cmd: &background\nmode: async\n"));
    assert!(!looks_like_yaml("rule: >threshold\nstatus: active\n"));
    assert!(!looks_like_yaml("script: |\nstatus: active\n"));
    assert!(looks_like_yaml("script: |-\n  echo hi\n"));
    assert!(looks_like_yaml("script: |2\n  echo hi\n"));
    assert!(looks_like_yaml("script: >2-\n  echo hi\n"));
    assert!(!looks_like_yaml("script: |0\n  echo hi\n"));
    assert!(!looks_like_yaml("script: >0\n  echo hi\n"));
    assert!(looks_like_yaml("defaults: &defaults\n  timeout: 30\n"));
}

#[test]
fn yaml_shape_helper_keeps_single_line_spaced_key_guardrail() {
    assert!(!looks_like_yaml("status report: done\n"));
    assert!(!looks_like_yaml("name: app\n"));
    assert!(!looks_like_yaml("From: jane@example.com\nSubject: demo\n"));
    assert!(!looks_like_yaml(
        "ERROR: connection refused\nWARN: retrying\n"
    ));
    assert!(!looks_like_yaml("Author: Jane\nStatus: draft\n"));
}

#[test]
fn yaml_shape_helper_requires_yaml_body_after_doc_start() {
    assert!(looks_like_yaml("---\nname: app\n"));
    assert!(!looks_like_yaml("---\njust a separator\n"));
    assert!(!looks_like_yaml("---\n- item\n"));
    assert!(looks_like_yaml("---\n- alpha\n- beta\n"));
}

#[test]
fn heuristic_detects_fallback_languages_and_conflict_matrix() {
    let cases = [
        ("fun main() { println(\"hi\") }", Some("kotlin")),
        (
            "import java.util.Scanner\nfun main() { println(\"hi\") }",
            Some("kotlin"),
        ),
        (
            "import Foundation\nfunc main() { print(\"hi\") }",
            Some("swift"),
        ),
        (
            "import 'package:foo/bar.dart';\nvoid main() {}",
            Some("dart"),
        ),
        (
            "const std = @import(\"std\");\npub fn main() void {}",
            Some("zig"),
        ),
        ("local x = 1\nfunction test()\nend", Some("lua")),
        ("use strict;\nuse warnings;\nmy $x = 1;", Some("perl")),
        ("defmodule Demo do\n  IO.puts(\"hi\")\nend", Some("elixir")),
        ("param($Name)\nWrite-Host $Name", Some("powershell")),
        ("#!/bin/python\nprint('hi')", Some("python")),
        (
            "from sqlite3 import connect\nquery = \"\"\"\nSELECT * FROM users WHERE id = 1\n\"\"\"",
            Some("python"),
        ),
        ("#!/usr/bin/env python\nprint('hi')", Some("python")),
        ("#!/bin/node\nconsole.log('hi')", Some("javascript")),
        ("#!/usr/bin/env bash\necho hi", Some("shell")),
        (
            "import { x } from 'module';\nconsole.log(x);",
            Some("javascript"),
        ),
    ];
    assert_detection_cases(cases.as_slice());
}

#[test]
fn heuristic_does_not_treat_param_call_alone_as_powershell() {
    let cases = [("param(foo)\nvalue = 1\n", None)];
    assert_detection_cases(cases.as_slice());
}

#[test]
fn shell_command_sequences_override_leading_setup_comments() {
    for content in [
        "cd app\nnpm install\nnpm run dev\n",
        "conda activate misc\npip install requests\n",
        "# setup environment\nexport MODE=dev\nsource .env\nmkdir -p out\ncargo build --release\n",
        "# bootstrap\ncd app\n# install dependencies\nnpm install\nnpm run dev\n",
        "cd repo\ngit cherry-pick abc123\ngit revert def456\ngit rm stale.txt\n",
        "cd app\necho ready\n",
        "cd Program Files\ngit status\n",
        "set alpha beta\ncargo check\n",
        "export PATH HOME\ncargo check\n",
        "source activate myenv\ncargo check\n",
        "export PATH\nsource env\ncargo check\n",
        "cd repo\ngit submodule update\n",
        "git rev-parse HEAD\ngit blame README.md\n",
        "cd 'my repo'\r\ngit submodule update\r\n",
    ] {
        assert!(
            super::looks_like_shell_command_sequence(content),
            "{content}"
        );
        assert_eq!(
            detect_language(content).as_deref(),
            Some("shell"),
            "{content}"
        );
    }
    for content in [
        "# Setup notes\nRun npm install before the demo.\n",
        "just wanted to say thanks\nmake yourself at home\n",
        "export controls are discussed here\nsource material follows\n",
        "cd app\nsudo is required for installation\n",
        "set timer for 10 minutes\ncargo check\n",
        "export markets are down 5% this quarter\ncargo check\n",
        "source code is at https://example.com\ncargo check\n",
        "cd albums are on the shelf/desk\ncargo check\n",
    ] {
        assert!(
            !super::looks_like_shell_command_sequence(content),
            "{content}"
        );
    }
}

#[test]
fn heuristic_avoids_common_single_token_false_positives() {
    let cases = [
        (
            "const tpl = \"<div class='x'>\";\nconsole.log(tpl);\n",
            Some("javascript"),
        ),
        (
            "fn main() { let d = std::time::Duration::from_secs(1); println!(\"{:?}\", d); }",
            Some("rust"),
        ),
        ("status report:\ndone\n", None),
        ("status report: done", None),
        ("please select one option from menu where possible", None),
        ("if then end", None),
        ("note: use strict; while migrating config", None),
        ("- item", Some("markdown")),
    ];
    assert_detection_cases(cases.as_slice());
}

#[test]
fn markdown_separator_content_is_not_mislabeled_as_yaml() {
    assert_ne!(
        detect_language("---\njust a separator\n").as_deref(),
        Some("yaml")
    );
}

#[test]
fn markdown_heading_with_colon_prefers_markdown_over_yaml() {
    let content = "Consolidate this nonsense:\n\n# test duplication review:\n";
    assert_eq!(detect_language(content).as_deref(), Some("markdown"));
}

#[test]
fn markdown_fence_prefers_markdown_over_structured_yaml() {
    let content = "```json\n{\"a\":1}\n```\n";
    assert_eq!(detect_language(content).as_deref(), Some("markdown"));
}

#[test]
fn markdown_yaml_fence_prefers_markdown_over_yaml_body() {
    let content = "```yaml\nname: app\nport: 8080\n```\n";
    assert_eq!(detect_language(content).as_deref(), Some("markdown"));
}

#[test]
fn shell_heredoc_with_fenced_example_stays_shell() {
    let content = "if [ -n \"$DEMO\" ]; then\ncat <<'EOF'\n```bash\necho hi\n```\nEOF\nfi\n";
    assert_eq!(detect_language(content).as_deref(), Some("shell"));
}

#[test]
fn yaml_block_scalar_with_indented_fences_stays_yaml() {
    let content = "script: |\n  ```bash\n  echo hi\n  ```\ntimeout: 30\n";
    assert_eq!(detect_language(content).as_deref(), Some("yaml"));
}

#[test]
fn markdown_bullet_list_prefers_markdown_over_yaml_sequence() {
    let content = "- alpha\n- beta\n";
    assert_eq!(detect_language(content).as_deref(), Some("markdown"));
}

#[test]
fn canonicalization_matrix_handles_aliases() {
    let cases = [
        ("csharp", "cs"),
        ("C#", "cs"),
        ("c++", "cpp"),
        ("bash", "shell"),
        ("yml", "yaml"),
        ("js", "javascript"),
        ("ts", "typescript"),
        ("md", "markdown"),
        ("plain text", "text"),
        ("pwsh", "powershell"),
        ("scss", "scss"),
        ("sass", "sass"),
        ("rust", "rust"),
        ("JSONL", "jsonl"),
    ];
    for (input, expected) in cases {
        assert_eq!(canonicalize(input), expected, "input: {input}");
    }
}

#[test]
fn json_lines_keep_format_identity_through_detection_and_export() {
    let large_record = format!("{{\"payload\":\"{}\"}}\n", "é".repeat(120));
    let large_records = large_record.repeat(400);
    let forty_kib_record = format!("{{\"payload\":\"{}\"}}\n", "x".repeat(40 * 1024));
    let two_large_records = forty_kib_record.repeat(2);
    for content in [
        "{\"name\":\"Ada\"}\n{\"name\":\"Grace\"}\n",
        "{\"a\":1}\n{\"a\":2}\n",
        large_records.as_str(),
        two_large_records.as_str(),
    ] {
        assert_eq!(heuristic::detect(content).as_deref(), Some("jsonl"));
        for label in ["json", "jsonl"] {
            assert_eq!(
                refine_magika_label(label, content).as_deref(),
                Some("jsonl")
            );
        }
        let language = detect_language(content).unwrap();
        assert_eq!(language, "jsonl");
        assert_eq!(super::preferred_extension(Some(&language)), "jsonl");
        assert_eq!(
            crate::semantic::derive(content, Some(&language)).kind,
            crate::semantic::PasteKind::Config
        );
    }
    assert_eq!(
        heuristic::detect("{\n\"name\":\"Ada\"\n}").as_deref(),
        Some("json")
    );
    for malformed in [
        "{\"name\":\"Ada\"}\n{broken}",
        "{\"name\":\"Ada\"}\n42",
        "{\"name\":\"Ada\"}\nnot a record",
    ] {
        assert_ne!(heuristic::detect(malformed).as_deref(), Some("jsonl"));
    }
    for content in [
        large_records.replace('\n', "\r\n"),
        format!(
            "{{\"a\":1}}\n{{\"padding\":\"{}\"}}\n{{\"a\":2}}\n",
            "x".repeat(crate::text::TEXT_SAMPLE_MAX_BYTES - 24)
        ),
    ] {
        assert!(heuristic::looks_like_json_lines(&content));
        assert_eq!(detect_language(&content).as_deref(), Some("jsonl"));
    }
    let malformed = format!("{{broken}}\n{large_records}");
    assert!(!heuristic::looks_like_json_lines(&malformed));
    let malformed_crossing_record =
        format!("{forty_kib_record}{{broken:{}}}\n", "x".repeat(30 * 1024));
    assert!(!heuristic::looks_like_json_lines(
        &malformed_crossing_record
    ));
    let oversized_crossing_record = format!(
        "{forty_kib_record}{{\"payload\":\"{}\"}}\n",
        "x".repeat(crate::text::TEXT_SAMPLE_MAX_BYTES)
    );
    assert!(!heuristic::looks_like_json_lines(
        &oversized_crossing_record
    ));
    let malformed_after_line_cap = format!("{}not-json\n", "{\"ok\":true}\n".repeat(512));
    assert!(malformed_after_line_cap.len() < crate::text::TEXT_SAMPLE_MAX_BYTES);
    assert!(!heuristic::looks_like_json_lines(&malformed_after_line_cap));
    assert_ne!(
        detect_language(&malformed_after_line_cap).as_deref(),
        Some("jsonl")
    );
}

#[test]
fn panic_detection_requires_a_leading_runtime_header() {
    let panic = "thread 'main' panicked at src/main.rs:12:5";
    assert!(super::looks_like_rust_panic(&format!("\n  {panic}\n")));
    assert_eq!(detect_language(panic).as_deref(), Some("log"));
    for content in [
        format!(
            "   Compiling demo v0.1.0\n    Finished `dev` profile\n     Running `target/debug/demo`\n{panic}\n"
        ),
        format!("$cargo run\n{panic}\n"),
        format!("$ cargo run --quiet\n{panic}\n"),
    ] {
        assert!(super::looks_like_rust_panic(&content), "{content}");
        assert_eq!(detect_language(&content).as_deref(), Some("log"));
        assert_eq!(
            crate::semantic::derive(&content, Some("text")).kind,
            crate::semantic::PasteKind::Log
        );
    }
    for (content, language) in [
        (
            format!(
                "# Bug report\nSteps to reproduce:\n```text\n{panic}\n```\nPlease investigate."
            ),
            "markdown",
        ),
        (
            format!(
                "fn main() {{\n    panic!(\"boom\");\n}}\n{panic}\nnote: run with RUST_BACKTRACE=1 to display a backtrace"
            ),
            "rust",
        ),
    ] {
        assert!(!super::looks_like_rust_panic(&content));
        assert_eq!(detect_language(&content).as_deref(), Some(language));
        assert_eq!(
            crate::semantic::derive(&content, Some(language)).kind,
            if language == "rust" {
                crate::semantic::PasteKind::Code
            } else {
                crate::semantic::PasteKind::Document
            }
        );
    }
    for prose in [
        format!("Finished painting the wall\n{panic}\n"),
        format!("Running errands before lunch\n{panic}\n"),
        format!("Compiling notes for the meeting\n{panic}\n"),
    ] {
        assert!(!super::looks_like_rust_panic(&prose), "{prose}");
    }
}

#[test]
fn rust_display_implementation_is_not_css() {
    let content = r#"use std::fmt::{self, Display};
impl Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "panic: {self}")
    }
}"#;
    assert_eq!(heuristic::detect(content).as_deref(), Some("rust"));
    assert_eq!(detect_language(content).as_deref(), Some("rust"));
    assert_eq!(
        crate::semantic::derive(content, Some("rust")).kind,
        crate::semantic::PasteKind::Code
    );
}

#[test]
fn gitattributes_refinement_rejects_short_clipboard_prose() {
    for content in [
        "clipboard review text",
        "Hi team please review this",
        "Python 3.12 is now installed",
    ] {
        assert_eq!(
            refine_magika_label("gitattributes", content),
            None,
            "{content}"
        );
    }
    for content in [
        "* text=auto",
        "*.rs diff=rust",
        "README export-ignore",
        "*.foo custom-attribute",
    ] {
        assert_eq!(
            refine_magika_label("gitattributes", content).as_deref(),
            Some("gitattributes"),
            "{content}"
        );
    }
}

#[cfg(feature = "magika")]
#[test]
fn magika_detects_high_signal_code_snippets() {
    let cases = [
        ("fn main() { println!(\"hello\"); }", &["rust"][..]),
        ("import os\nprint(os.getcwd())", &["python"][..]),
        (
            "const x = () => console.log('hi');",
            &["javascript", "typescript"][..],
        ),
        ("#!/bin/bash\necho hi", &["shell", "powershell"][..]),
        ("{\"key\": \"value\"}", &["json"][..]),
    ];
    for (content, expected_any) in cases {
        let detected = detect_language(content);
        assert!(
            detected
                .as_deref()
                .map(|value| expected_any.contains(&value))
                .unwrap_or(false),
            "content: {content}, detected: {:?}",
            detected
        );
    }
}

#[cfg(feature = "magika")]
#[test]
fn magika_and_fallback_agree_on_flat_config_yaml() {
    assert_eq!(
        detect_language("name: app\nversion: 1\nport: 8080\n").as_deref(),
        Some("yaml")
    );
}

#[test]
fn magika_refinement_rejects_weak_yaml_shape() {
    assert_eq!(refine_magika_label("yaml", "status report:\ndone\n"), None);
    assert_eq!(refine_magika_label("yaml", "- item\n"), None);
    assert_eq!(refine_magika_label("yaml", "---\njust a separator\n"), None);
    assert_eq!(
        refine_magika_label("yaml", "---\nname: app\n"),
        Some("yaml".to_string())
    );
    assert_eq!(refine_magika_label("yaml", "name: app"), None);
    assert_eq!(
        refine_magika_label("yaml", "name: app\nversion: 1\nport: 8080\n"),
        Some("yaml".to_string())
    );
    assert_eq!(
        refine_magika_label("yaml", "apiVersion: v1\nkind: Pod\n"),
        Some("yaml".to_string())
    );
    assert_eq!(
        refine_magika_label("yaml", "name: app\nservices:\n  - web\n"),
        Some("yaml".to_string())
    );
    assert_eq!(
        refine_magika_label("yaml", "display name: api\nport: 8080\n"),
        None
    );
    assert_eq!(
        refine_magika_label("yaml", "services:\n  web:\n    image: nginx\n"),
        Some("yaml".to_string())
    );
    assert_eq!(
        refine_magika_label("yaml", "root: {child: value}\n"),
        Some("yaml".to_string())
    );
    assert_eq!(
        refine_magika_label("yaml", "services: [web]\nversion: 3\n"),
        Some("yaml".to_string())
    );
    assert_eq!(refine_magika_label("yaml", "status report: done\n"), None);
    assert_eq!(
        refine_magika_label("yaml", "```json\n{\"k\":1}\n```\n"),
        Some("markdown".to_string())
    );
}

#[test]
fn flat_config_yaml_helper_accepts_config_shaped_flat_mappings_only() {
    assert!(looks_like_flat_config_yaml(
        "name: app\nversion: 1\nport: 8080\n"
    ));
    assert!(looks_like_flat_config_yaml("name: app\nversion: 1\n...\n"));
    assert!(looks_like_flat_config_yaml("apiVersion: v1\nkind: Pod\n"));
    assert!(!looks_like_flat_config_yaml(
        "display name: api\nport: 8080\n"
    ));
    assert!(!looks_like_flat_config_yaml(
        "Author: Jane\nStatus: draft\n"
    ));
    assert!(!looks_like_flat_config_yaml("name: app\n"));
}

#[test]
fn magika_refinement_does_not_override_shell_to_markdown_on_comment_heading() {
    assert_eq!(
        refine_magika_label("shell", "# install script\necho hi\n"),
        Some("shell".to_string())
    );
}

#[test]
fn magika_refinement_does_not_override_json_with_fenced_string_content() {
    assert_eq!(
        refine_magika_label("json", r#"{"note":"```bash\nls\n```"}"#),
        Some("json".to_string())
    );
}

#[test]
fn magika_refinement_converts_plain_css_mislabeled_as_scss() {
    assert_eq!(
        refine_magika_label("scss", "body {\n  color: #333;\n  margin: 0;\n}"),
        Some("css".to_string())
    );
    assert_eq!(
        refine_magika_label("scss", ".parent {\n  .child {\n    color: red;\n  }\n}\n"),
        Some("scss".to_string())
    );
    assert_eq!(
        refine_magika_label("scss", ".button {\n  &:hover {\n    color: red;\n  }\n}\n"),
        Some("scss".to_string())
    );
    assert_eq!(
        refine_magika_label("scss", "$primary: #333;\nbody { color: $primary; }\n"),
        Some("scss".to_string())
    );
    assert_eq!(
        refine_magika_label("scss", "%button-base {\n  color: red;\n}\n"),
        Some("scss".to_string())
    );
}
