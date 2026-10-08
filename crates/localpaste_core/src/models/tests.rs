//! Model-level unit tests.

#[cfg(test)]
mod model_tests {
    use super::super::*;

    #[test]
    fn test_paste_new() {
        let content = "Hello, World!";
        let name = "test-paste";
        let paste = paste::Paste::new(content.to_string(), name.to_string());

        assert_eq!(paste.content, content);
        assert_eq!(paste.name, name);
        assert!(!paste.id.is_empty());
        assert!(paste.folder_id.is_none());
        assert_eq!(paste.tags.len(), 0);
    }

    #[test]
    fn test_paste_new_with_language_uses_provided_values_without_detection_override() {
        let paste = paste::Paste::new_with_language(
            "fn main() { println!(\"hi\"); }".to_string(),
            "manual".to_string(),
            Some("python".to_string()),
            true,
        );
        assert_eq!(paste.language.as_deref(), Some("python"));
        assert!(paste.language_is_manual);
    }

    #[test]
    fn test_detect_language_plain_text() {
        assert_eq!(crate::detection::detect_language("just some words"), None);
    }

    #[test]
    fn test_paste_detect_language_matrix() {
        let cases = [
            ("fn main() { println!(\"hi\"); }", Some("rust")),
            ("def main():\n    print('hi')", Some("python")),
            ("const x = () => console.log('hi');", Some("javascript")),
            ("#!/bin/bash\necho hello", Some("shell")),
            ("name: app\nversion: 1\n...\n", Some("yaml")),
            ("select id from users where active = 1", Some("sql")),
            ("[tool]\nname = \"demo\"\nversion = \"0.1.0\"", Some("toml")),
            (
                "import Foundation\nfunc main() { print(\"hi\") }",
                Some("swift"),
            ),
            ("fun main() { println(\"hi\") }", Some("kotlin")),
            ("just some plain text words", None),
        ];
        for (content, expected) in cases {
            assert_eq!(
                crate::detection::detect_language(content).as_deref(),
                expected,
                "content: {content}"
            );
        }
    }

    #[test]
    fn test_paste_new_stores_detected_language_as_locked() {
        let paste = paste::Paste::new(
            "fn main() {\n    let value = 5;\n    println!(\"{value}\");\n}".to_string(),
            "rust".to_string(),
        );
        assert_eq!(paste.language.as_deref(), Some("rust"));
        assert!(paste.language_is_manual);
    }

    #[test]
    fn test_detect_language_handles_large_payload_without_losing_prefix_signal() {
        let mut content = String::from("pub fn main() {\n    let value = 42;\n}\n");
        content.push_str(&"x".repeat(256 * 1024));
        assert_eq!(
            crate::detection::detect_language(&content),
            Some("rust".to_string())
        );
    }

    #[test]
    fn test_detect_language_keeps_json_for_large_truncated_sample() {
        let mut content = String::from("{\"items\":[");
        for idx in 0..6000 {
            if idx > 0 {
                content.push(',');
            }
            content.push_str("{\"id\":");
            content.push_str(idx.to_string().as_str());
            content.push_str(",\"name\":\"entry\"}");
        }
        content.push_str("]}");
        assert!(
            content.len() > 64 * 1024,
            "test fixture must exceed sampled prefix size"
        );
        assert_eq!(
            crate::detection::detect_language(&content),
            Some("json".to_string())
        );
    }

    #[test]
    fn test_paste_meta_from_populates_derived_retrieval_fields() {
        let code = paste::Paste::new(
            "fn handle_request() {\n    let fsdp2 = \"cublaslt\";\n}\n".to_string(),
            "random-slug".to_string(),
        );
        let config = paste::Paste::new(
            "model: gpt-4\nservice: trainer\n".to_string(),
            "config".to_string(),
        );
        let link = paste::Paste::new("https://example.com/docs\n".to_string(), "link".to_string());

        let code_meta = paste::PasteMeta::from(&code);
        let config_meta = paste::PasteMeta::from(&config);
        let link_meta = paste::PasteMeta::from(&link);

        assert_eq!(code_meta.derived.kind, crate::semantic::PasteKind::Code);
        assert_eq!(
            code_meta.derived.handle.as_deref(),
            Some("fn handle_request")
        );
        assert!(code_meta.derived.terms.iter().any(|term| term == "fsdp2"));
        assert_eq!(config_meta.derived.kind, crate::semantic::PasteKind::Config);
        assert_eq!(config_meta.derived.handle.as_deref(), Some("model gpt-4"));
        assert_eq!(link_meta.derived.kind, crate::semantic::PasteKind::Link);
        assert_eq!(link_meta.derived.handle.as_deref(), Some("example.com"));
    }

    #[test]
    fn test_paste_new_classifies_structural_runtime_fixtures() {
        use crate::semantic::PasteKind;

        let cases = [
            (
                "# setup environment\nexport MODE=dev\nsource .env\nmkdir -p out\ncargo build --release\n",
                PasteKind::Code,
            ),
            (
                "```text\nINFO (main) Starting worker\nERROR (main) Worker stopped\n```",
                PasteKind::Log,
            ),
            (
                "yarn install v1.22.22\ninfo Resolving packages\nwarning Retrying request\nsuccess Saved lockfile",
                PasteKind::Log,
            ),
            (
                "yarn install v1.22.22\ninfo resolving package graph\ninfo fetching packages",
                PasteKind::Log,
            ),
            (
                "INFO\tStarting worker\nERROR\tWorker stopped",
                PasteKind::Log,
            ),
            (
                "info about the meeting is below\nerror handling is a topic we should discuss",
                PasteKind::Document,
            ),
            ("cd app\nsudo is required for installation", PasteKind::Document),
            (
                "fn main() {\n    println!(\"ready\");\n    // thread 'main' panicked at src/main.rs:12:5\n}",
                PasteKind::Code,
            ),
            (
                "# Runtime report\n\n```text\nthread 'main' panicked at src/main.rs:12:5\n```\n",
                PasteKind::Document,
            ),
        ];
        for (content, expected) in cases {
            let paste = paste::Paste::new(content.to_string(), "fixture".to_string());
            assert_eq!(
                paste::PasteMeta::from(&paste).derived.kind,
                expected,
                "language {:?}: {content}",
                paste.language
            );
        }

        let manual_markdown = paste::Paste::new_with_language(
            "# setup environment\nexport MODE=dev\nsource .env\nmkdir -p out\ncargo build --release\n"
                .to_string(),
            "manual notes".to_string(),
            Some("markdown".to_string()),
            true,
        );
        assert_eq!(
            paste::PasteMeta::from(&manual_markdown).derived.kind,
            PasteKind::Code
        );
        assert_eq!(manual_markdown.language.as_deref(), Some("markdown"));
        assert!(manual_markdown.language_is_manual);
        assert!(manual_markdown.is_markdown);
        let manual_command = paste::Paste::new_with_language(
            "# Todo\ngit pull".into(),
            "manual command".into(),
            Some("markdown".into()),
            true,
        );
        assert_eq!(
            paste::PasteMeta::from(&manual_command).derived.kind,
            PasteKind::Code
        );
        assert_eq!(manual_command.language.as_deref(), Some("markdown"));
        assert!(manual_command.language_is_manual);
        let manual_note = paste::Paste::new_with_language(
            "# Setup notes\n\nThese commands prepare the environment.\n\ncargo build --release"
                .to_string(),
            "manual notes".to_string(),
            Some("markdown".to_string()),
            true,
        );
        assert_eq!(
            paste::PasteMeta::from(&manual_note).derived.kind,
            PasteKind::Document
        );
        assert_eq!(manual_note.language.as_deref(), Some("markdown"));
        assert!(manual_note.language_is_manual);
        assert!(manual_note.is_markdown);
    }

    #[test]
    fn test_paste_is_markdown() {
        let md_paste = paste::Paste::new(
            "# Header\n```rust\ncode\n```".to_string(),
            "test".to_string(),
        );
        assert!(md_paste.is_markdown);

        let not_md = paste::Paste::new("just plain text".to_string(), "test".to_string());
        assert!(!not_md.is_markdown);

        let rust_attr = paste::Paste::new(
            "#[derive(Debug)]\nstruct Example;".to_string(),
            "rust-attr".to_string(),
        );
        assert!(
            !rust_attr.is_markdown,
            "Rust attributes should not be treated as markdown headings"
        );

        let css_hex = paste::Paste::new(
            "body {\n  color: #333;\n}".to_string(),
            "css-hex".to_string(),
        );
        assert!(
            !css_hex.is_markdown,
            "CSS hex colors should not trigger markdown detection"
        );

        let shebang =
            paste::Paste::new("#!/bin/bash\necho hello".to_string(), "script".to_string());
        assert!(
            !shebang.is_markdown,
            "shell shebang/comments should not trigger markdown detection"
        );
    }

    #[test]
    fn test_folder_new() {
        let name = "My Folder";
        let folder = folder::Folder::new(name.to_string());

        assert_eq!(folder.name, name);
        assert!(!folder.id.is_empty());
        assert_eq!(folder.paste_count, 0);
        assert!(folder.parent_id.is_none());
    }
}
