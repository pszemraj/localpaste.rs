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
    fn test_paste_detect_language_matrix() {
        let cases = [
            (
                "python",
                "def main():\n    import sys\n    print('hello')",
                "python",
            ),
            (
                "rust",
                "fn main() {\n    let x = 5;\n    println!(\"hello\");\n}",
                "rust",
            ),
            (
                "javascript",
                "const hello = () => {\n    console.log('hello');\n}",
                "javascript",
            ),
            ("json", "{\n  \"name\": \"test\",\n  \"value\": 123\n}", "json"),
            (
                "csharp",
                "using System;\nnamespace Demo {\n    public class Program {\n        public static void Main(string[] args) {\n            Console.WriteLine(\"hi\");\n        }\n    }\n}",
                "cs",
            ),
            (
                "html",
                "<!DOCTYPE html>\n<html>\n  <body>\n    <h1>Hello</h1>\n  </body>\n</html>",
                "html",
            ),
            ("css", "body {\n  color: #333;\n  margin: 0;\n}", "css"),
            (
                "shell",
                "#!/bin/bash\nname=$1\necho \"Hello ${name}\"",
                "shell",
            ),
            ("toml", "[tool]\nname = \"demo\"\nversion = \"0.1.0\"", "toml"),
            ("yaml", "name: demo\nservices:\n  - web\n  - worker", "yaml"),
        ];

        for (name, content, expected_language) in cases {
            let paste = paste::Paste::new(content.to_string(), name.to_string());
            assert_eq!(
                paste.language.as_deref(),
                Some(expected_language),
                "language detection mismatch for case '{}'",
                name
            );
        }
    }

    #[test]
    fn test_detect_language_plain_text() {
        assert_eq!(paste::detect_language("just some words"), None);
    }

    #[test]
    fn test_detect_language_handles_large_payload_without_losing_prefix_signal() {
        let mut content = String::from("pub fn main() {\n    let value = 42;\n}\n");
        content.push_str(&"x".repeat(256 * 1024));
        assert_eq!(paste::detect_language(&content), Some("rust".to_string()));
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
        assert_eq!(paste::detect_language(&content), Some("json".to_string()));
    }

    #[test]
    fn test_paste_request_validation() {
        let valid_req = paste::CreatePasteRequest {
            content: "test".to_string(),
            name: Some("test-paste".to_string()),
            language: Some("rust".to_string()),
            language_is_manual: Some(true),
            folder_id: None,
            tags: None,
        };

        assert!(!valid_req.content.is_empty());
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

    #[test]
    fn test_folder_request() {
        let req = folder::CreateFolderRequest {
            name: "Test Folder".to_string(),
            parent_id: None,
        };

        assert_eq!(req.name, "Test Folder");
        assert!(req.parent_id.is_none());
    }
}
