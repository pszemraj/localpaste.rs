//! Detection module tests for canonicalization, fallback heuristics, and Magika integration.

use super::canonical::canonicalize;
use super::detect_language;

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
        ("[tool]\nname = \"demo\"\nversion = \"0.1.0\"", Some("toml")),
        ("just some plain text words", None),
    ];
    assert_detection_cases(cases.as_slice());
}

#[test]
fn heuristic_detects_expanded_fallback_languages() {
    let cases = [
        ("fun main() { println(\"hi\") }", Some("kotlin")),
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
    ];
    assert_detection_cases(cases.as_slice());
}

#[test]
fn heuristic_does_not_treat_param_call_alone_as_powershell() {
    let cases = [("param(foo)\nvalue = 1\n", None)];
    assert_detection_cases(cases.as_slice());
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
    ];
    for (input, expected) in cases {
        assert_eq!(canonicalize(input), expected, "input: {input}");
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
