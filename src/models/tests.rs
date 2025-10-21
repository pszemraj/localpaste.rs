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
    fn test_paste_detect_language_python() {
        let paste = paste::Paste::new(
            "def main():\n    import sys\n    print('hello')".to_string(),
            "test".to_string(),
        );
        assert_eq!(paste.language, Some("python".to_string()));
    }

    #[test]
    fn test_paste_detect_language_rust() {
        let paste = paste::Paste::new(
            "fn main() {\n    let x = 5;\n    println!(\"hello\");\n}".to_string(),
            "test".to_string(),
        );
        assert_eq!(paste.language, Some("rust".to_string()));
    }

    #[test]
    fn test_paste_detect_language_javascript() {
        let paste = paste::Paste::new(
            "const hello = () => {\n    console.log('hello');\n}".to_string(),
            "test".to_string(),
        );
        assert_eq!(paste.language, Some("javascript".to_string()));
    }

    #[test]
    fn test_paste_detect_language_json() {
        let paste = paste::Paste::new(
            "{\n  \"name\": \"test\",\n  \"value\": 123\n}".to_string(),
            "test".to_string(),
        );
        assert_eq!(paste.language, Some("json".to_string()));
    }

    #[test]
    fn test_paste_request_validation() {
        let valid_req = paste::CreatePasteRequest {
            content: "test".to_string(),
            name: Some("test-paste".to_string()),
            language: Some("rust".to_string()),
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
