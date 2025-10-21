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

#[derive(Debug, Deserialize, Clone)]
pub struct UpdatePasteRequest {
    pub content: Option<String>,
    pub name: Option<String>,
    pub language: Option<Option<String>>,
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
        let language = detect_language(&content);
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            content: content.clone(),
            language,
            folder_id: None,
            created_at: now,
            updated_at: now,
            tags: Vec::new(),
            is_markdown: is_probably_markdown(&content),
        }
    }
}

pub fn detect_language(content: &str) -> Option<String> {
    let trimmed = content
        .trim_start_matches('\u{feff}')
        .trim_start_matches(|c: char| c.is_whitespace());

    if trimmed.is_empty() {
        return None;
    }

    if let Some(lang) = detect_from_shebang(trimmed) {
        return Some(lang.to_string());
    }

    if looks_like_json(trimmed) {
        return Some("json".to_string());
    }

    if looks_like_toml(trimmed) {
        return Some("toml".to_string());
    }

    if looks_like_yaml(trimmed) {
        return Some("yaml".to_string());
    }

    if looks_like_latex(trimmed) {
        return Some("latex".to_string());
    }

    if looks_like_markdown(trimmed) {
        return Some("markdown".to_string());
    }

    if let Some(markup) = detect_markup_language(trimmed) {
        return Some(markup.to_string());
    }

    if looks_like_css(trimmed) {
        return Some("css".to_string());
    }

    let lowered = trimmed.to_lowercase();

    let mut best_lang: Option<&'static str> = None;
    let mut best_score: u32 = 0;

    for rule in LANGUAGE_RULES {
        let mut score = 0u32;

        for (pattern, weight) in rule.case_sensitive {
            if trimmed.contains(pattern) {
                score += u32::from(*weight);
            }
        }

        for (pattern, weight) in rule.case_insensitive {
            if lowered.contains(pattern) {
                score += u32::from(*weight);
            }
        }

        if score >= u32::from(rule.min_score) && score > best_score {
            best_score = score;
            best_lang = Some(rule.name);
        }
    }

    best_lang.map(|name| name.to_string())
}

pub fn is_probably_markdown(content: &str) -> bool {
    looks_like_markdown(content)
}

pub fn apply_update(paste: &mut Paste, update: &UpdatePasteRequest) {
    let mut content_changed = false;

    if let Some(content) = &update.content {
        paste.content = content.clone();
        paste.is_markdown = is_probably_markdown(&paste.content);
        content_changed = true;
    }

    if let Some(name) = &update.name {
        paste.name = name.clone();
    }

    match update.language.clone() {
        Some(Some(lang)) => {
            paste.language = Some(lang);
        }
        Some(None) => {
            paste.language = detect_language(&paste.content);
        }
        None => {
            if content_changed {
                paste.language = detect_language(&paste.content);
            }
        }
    }

    if let Some(ref fid) = update.folder_id {
        paste.folder_id = if fid.is_empty() {
            None
        } else {
            Some(fid.clone())
        };
    }

    if let Some(tags) = &update.tags {
        paste.tags = tags.clone();
    }

    paste.updated_at = Utc::now();
}

fn detect_from_shebang(content: &str) -> Option<&'static str> {
    let first_line = content.lines().next()?.trim().to_lowercase();
    if !first_line.starts_with("#!") {
        return None;
    }

    if first_line.contains("python") {
        Some("python")
    } else if first_line.contains("bash") || first_line.contains("sh") {
        Some("shell")
    } else if first_line.contains("node") || first_line.contains("deno") {
        Some("javascript")
    } else {
        None
    }
}

fn looks_like_json(text: &str) -> bool {
    let trimmed = text.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}'))
        && !(trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return false;
    }
    trimmed.contains(':') && trimmed.contains('"')
}

fn looks_like_yaml(text: &str) -> bool {
    let mut colon_pairs = 0;
    let mut list_markers = 0;

    for line in text.lines().take(40) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('-') {
            list_markers += 1;
        }
        if trimmed.contains(": ") {
            colon_pairs += 1;
        }
    }

    colon_pairs >= 3 || (colon_pairs >= 1 && list_markers >= 1)
}

fn looks_like_toml(text: &str) -> bool {
    let mut section_headers = 0;
    let mut assignments = 0;

    for line in text.lines().take(40) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section_headers += 1;
        }
        if trimmed.contains('=') && trimmed.split('=').next().map(|s| !s.trim().is_empty()).unwrap_or(false) {
            assignments += 1;
        }
    }

    section_headers >= 1 && assignments >= 1 || assignments >= 3
}

fn looks_like_markdown(text: &str) -> bool {
    let mut score = 0;
    if text.contains("```") {
        score += 2;
    }
    if text.contains("\n# ") || text.starts_with('#') {
        score += 2;
    }
    if text.contains("\n- ") || text.contains("\n* ") {
        score += 1;
    }
    if text.contains("[") && text.contains("](") {
        score += 1;
    }
    score >= 3
}

fn looks_like_latex(text: &str) -> bool {
    text.contains("\\begin{") || text.contains("\\end{") || text.contains("\\section") || text.contains("\\usepackage")
}

fn detect_markup_language(text: &str) -> Option<&'static str> {
    let first_line = text.lines().next().unwrap_or("");
    let lower = first_line.to_lowercase();

    if lower.contains("<!doctype html") || lower.contains("<html") {
        return Some("html");
    }

    if lower.starts_with("<?xml") || text.contains("</") {
        if lower.contains("<html") {
            Some("html")
        } else {
            Some("xml")
        }
    } else if text.contains("<div") || text.contains("<span") || text.contains("<body") {
        Some("html")
    } else {
        None
    }
}

fn looks_like_css(text: &str) -> bool {
    let mut braces = 0;
    let mut selectors = 0;
    let mut property_lines = 0;

    for line in text.lines().take(80) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("/*") {
            continue;
        }

        if trimmed.ends_with('{') && !trimmed.starts_with('@') && trimmed.chars().any(|c| c.is_alphabetic()) {
            selectors += 1;
        }
        if trimmed.contains(':') && trimmed.ends_with(';') {
            property_lines += 1;
        }
        braces += trimmed.matches('{').count();
        braces += trimmed.matches('}').count();
    }

    selectors >= 1 && property_lines >= 2 && braces >= 2
}

struct LanguageRule {
    name: &'static str,
    case_sensitive: &'static [(&'static str, u8)],
    case_insensitive: &'static [(&'static str, u8)],
    min_score: u8,
}

const RUST_CASE_SENSITIVE: [(&str, u8); 7] = [
    ("fn ", 3),
    ("impl ", 2),
    ("::", 1),
    (" match ", 2),
    (" pub ", 2),
    ("println!", 2),
    (" let ", 1),
];

const PY_CASE_SENSITIVE: [(&str, u8); 8] = [
    ("def ", 3),
    ("class ", 2),
    ("import ", 2),
    (" from ", 2),
    ("self", 1),
    ("async ", 1),
    ("lambda", 1),
    ("print(", 1),
];

const JS_CASE_SENSITIVE: [(&str, u8); 7] = [
    ("function", 2),
    ("const ", 2),
    ("let ", 2),
    ("=>", 2),
    ("import ", 2),
    ("export ", 2),
    ("console.", 2),
];

const GO_CASE_SENSITIVE: [(&str, u8); 6] = [
    ("package ", 2),
    ("func ", 3),
    (" fmt.", 2),
    (" go ", 1),
    ("chan", 1),
    ("defer", 1),
];

const JAVA_CASE_SENSITIVE: [(&str, u8); 6] = [
    ("public class", 3),
    ("System.out", 2),
    ("implements", 2),
    (" extends ", 2),
    ("@Override", 1),
    (" new ", 1),
];

const CSHARP_CASE_SENSITIVE: [(&str, u8); 6] = [
    ("using System", 3),
    ("namespace", 2),
    ("Console.Write", 2),
    (" async Task", 2),
    ("public class", 2),
    (" get;", 1),
];

const C_CASE_SENSITIVE: [(&str, u8); 5] = [
    ("#include", 3),
    ("int main", 3),
    ("printf", 2),
    ("scanf", 1),
    ("->", 1),
];

const CPP_CASE_SENSITIVE: [(&str, u8); 6] = [
    ("#include", 3),
    ("std::", 3),
    ("cout", 2),
    ("template", 2),
    ("typename", 1),
    ("::iterator", 1),
];

const SHELL_CASE_SENSITIVE: [(&str, u8); 6] = [
    ("if [", 2),
    (" then", 1),
    (" fi", 1),
    ("$((", 2),
    ("case ", 2),
    ("done", 1),
];

const LANGUAGE_RULES: &[LanguageRule] = &[
    LanguageRule {
        name: "rust",
        case_sensitive: &RUST_CASE_SENSITIVE,
        case_insensitive: &[],
        min_score: 5,
    },
    LanguageRule {
        name: "python",
        case_sensitive: &PY_CASE_SENSITIVE,
        case_insensitive: &[],
        min_score: 5,
    },
    LanguageRule {
        name: "javascript",
        case_sensitive: &JS_CASE_SENSITIVE,
        case_insensitive: &[],
        min_score: 5,
    },
    LanguageRule {
        name: "go",
        case_sensitive: &GO_CASE_SENSITIVE,
        case_insensitive: &[],
        min_score: 6,
    },
    LanguageRule {
        name: "java",
        case_sensitive: &JAVA_CASE_SENSITIVE,
        case_insensitive: &[],
        min_score: 6,
    },
    LanguageRule {
        name: "csharp",
        case_sensitive: &CSHARP_CASE_SENSITIVE,
        case_insensitive: &[],
        min_score: 6,
    },
    LanguageRule {
        name: "c",
        case_sensitive: &C_CASE_SENSITIVE,
        case_insensitive: &[],
        min_score: 6,
    },
    LanguageRule {
        name: "cpp",
        case_sensitive: &CPP_CASE_SENSITIVE,
        case_insensitive: &[],
        min_score: 6,
    },
    LanguageRule {
        name: "shell",
        case_sensitive: &SHELL_CASE_SENSITIVE,
        case_insensitive: &[],
        min_score: 5,
    },
];
