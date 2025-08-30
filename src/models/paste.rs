use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paste {
    pub id: String,
    pub name: String,
    pub content: String,
    pub language: Option<String>,
    pub folder_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub tags: Vec<String>,
    pub is_markdown: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreatePasteRequest {
    pub content: String,
    pub language: Option<String>,
    pub folder_id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePasteRequest {
    pub content: Option<String>,
    pub name: Option<String>,
    pub language: Option<String>,
    pub folder_id: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub folder_id: Option<String>,
    pub language: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<usize>,
    pub folder_id: Option<String>,
}

impl Paste {
    pub fn new(content: String, name: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            content: content.clone(),
            language: detect_language(&content),
            folder_id: None,
            created_at: now,
            updated_at: now,
            tags: Vec::new(),
            is_markdown: content.contains("```") || content.contains('#'),
        }
    }
}

fn detect_language(content: &str) -> Option<String> {
    let patterns = [
        (
            "rust",
            vec![
                "fn ", "impl ", "use ", "let ", "mut ", "pub ", "struct ", "enum ",
            ],
        ),
        (
            "javascript",
            vec!["function", "const ", "let ", "var ", "=>"],
        ),
        (
            "typescript",
            vec!["interface ", "type ", ": string", ": number"],
        ),
        (
            "python",
            vec!["def ", "import ", "from ", "class ", "if __name__"],
        ),
        ("go", vec!["package ", "func ", "import (", "var ", "type "]),
        (
            "java",
            vec!["public class", "private ", "protected ", "static void"],
        ),
        ("c", vec!["#include", "int main", "void ", "char *"]),
        ("cpp", vec!["#include", "std::", "template", "namespace"]),
        (
            "sql",
            vec!["SELECT ", "INSERT ", "UPDATE ", "DELETE ", "FROM "],
        ),
        ("shell", vec!["#!/bin/", "echo ", "export ", "if [", "then"]),
        ("yaml", vec!["- ", ": ", "---"]),
        ("toml", vec!["[", "] =", "[[", "]]"]),
        ("json", vec!["{", "}", ":", ","]),
    ];

    for (lang, keywords) in patterns {
        if keywords.iter().filter(|k| content.contains(*k)).count() >= 2 {
            return Some(lang.to_string());
        }
    }
    None
}
