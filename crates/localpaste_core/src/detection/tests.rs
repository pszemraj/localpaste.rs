//! Detection module tests for canonicalization, fallback heuristics, and Magika integration.

use super::canonical::canonicalize;
use super::detect_language;
use super::looks_like_yaml;
#[cfg(feature = "magika")]
use super::refine_magika_label;

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
        ("name: app", Some("yaml")),
        ("services: [web]\nversion: 3", Some("yaml")),
        ("[tool]\nname = \"demo\"\nversion = \"0.1.0\"", Some("toml")),
        ("just some plain text words", None),
    ];
    assert_detection_cases(cases.as_slice());
}

#[test]
fn yaml_shape_helper_handles_flow_values_and_single_list_guard() {
    assert!(looks_like_yaml("root: {child: value}\n"));
    assert!(looks_like_yaml("services: [web]\nversion: 3\n"));
    assert!(!looks_like_yaml("- item\n"));
}

#[test]
fn yaml_shape_helper_requires_yaml_body_after_doc_start() {
    assert!(looks_like_yaml("---\nname: app\n"));
    assert!(!looks_like_yaml("---\njust a separator\n"));
    assert!(!looks_like_yaml("---\n- item\n"));
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

#[cfg(feature = "magika")]
#[test]
fn magika_refinement_rejects_weak_yaml_shape() {
    assert_eq!(refine_magika_label("yaml", "status report:\ndone\n"), None);
    assert_eq!(refine_magika_label("yaml", "- item\n"), None);
    assert_eq!(refine_magika_label("yaml", "---\njust a separator\n"), None);
    assert_eq!(
        refine_magika_label("yaml", "---\nname: app\n"),
        Some("yaml".to_string())
    );
    assert_eq!(
        refine_magika_label("yaml", "name: app"),
        Some("yaml".to_string())
    );
    assert_eq!(
        refine_magika_label("yaml", "name: app\nservices:\n  - web\n"),
        Some("yaml".to_string())
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
}

#[cfg(feature = "magika")]
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
}
