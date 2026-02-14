//! Helper filters and classifiers for app state operations.

use crate::backend::PasteSummary;

struct SummaryPattern {
    languages: &'static [&'static str],
    name_needles: &'static [&'static str],
    tag_needles: &'static [&'static str],
}

const CODE_SUMMARY_PATTERN: SummaryPattern = SummaryPattern {
    languages: &[
        "rust",
        "python",
        "javascript",
        "typescript",
        "go",
        "java",
        "kotlin",
        "swift",
        "ruby",
        "php",
        "c",
        "cpp",
        "c++",
        "csharp",
        "cs",
        "shell",
        "bash",
        "zsh",
        "sql",
        "html",
        "css",
        "markdown",
    ],
    name_needles: &[
        ".rs", ".py", ".js", ".ts", ".go", ".java", ".cs", ".sql", ".sh", "snippet", "script",
        "class", "function",
    ],
    tag_needles: &["code", "snippet", "script"],
};

const CONFIG_SUMMARY_PATTERN: SummaryPattern = SummaryPattern {
    languages: &[
        "json",
        "yaml",
        "yml",
        "toml",
        "ini",
        "env",
        "xml",
        "hcl",
        "properties",
    ],
    name_needles: &[
        "config",
        "settings",
        ".env",
        "dockerfile",
        "compose",
        "k8s",
        "kubernetes",
        "helm",
    ],
    tag_needles: &[
        "config",
        "settings",
        "env",
        "docker",
        "k8s",
        "kubernetes",
        "helm",
    ],
};

const LOG_SUMMARY_PATTERN: SummaryPattern = SummaryPattern {
    languages: &["log"],
    name_needles: &["log", "logs", "trace", "stderr", "stdout", "error"],
    tag_needles: &["log", "logs", "trace", "stderr", "stdout", "error"],
};

const LINK_SUMMARY_PATTERN: SummaryPattern = SummaryPattern {
    languages: &[],
    name_needles: &["http://", "https://", "www.", "url", "link", "links"],
    tag_needles: &["url", "link", "links", "bookmark"],
};

pub(super) fn parse_tags_csv(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in input.split(',') {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        if out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        out.push(trimmed.to_string());
    }
    out
}

fn language_in_set(language: Option<&str>, values: &[&str]) -> bool {
    let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    values
        .iter()
        .any(|value| language.eq_ignore_ascii_case(value))
}

pub(super) fn normalize_language_filter_value(value: Option<&str>) -> Option<String> {
    localpaste_core::models::paste::normalize_language_filter(value)
}

fn contains_any_ci(value: &str, needles: &[&str]) -> bool {
    let value_lower = value.to_ascii_lowercase();
    needles.iter().any(|needle| value_lower.contains(needle))
}

fn tags_contain_any(tags: &[String], needles: &[&str]) -> bool {
    tags.iter().any(|tag| contains_any_ci(tag, needles))
}

fn summary_matches(
    item: &PasteSummary,
    languages: &[&str],
    name_needles: &[&str],
    tag_needles: &[&str],
) -> bool {
    language_in_set(item.language.as_deref(), languages)
        || contains_any_ci(item.name.as_str(), name_needles)
        || tags_contain_any(item.tags.as_slice(), tag_needles)
}

fn summary_matches_pattern(item: &PasteSummary, pattern: &SummaryPattern) -> bool {
    summary_matches(
        item,
        pattern.languages,
        pattern.name_needles,
        pattern.tag_needles,
    )
}

pub(super) fn is_code_summary(item: &PasteSummary) -> bool {
    summary_matches_pattern(item, &CODE_SUMMARY_PATTERN)
}

pub(super) fn is_config_summary(item: &PasteSummary) -> bool {
    summary_matches_pattern(item, &CONFIG_SUMMARY_PATTERN)
}

pub(super) fn is_log_summary(item: &PasteSummary) -> bool {
    summary_matches_pattern(item, &LOG_SUMMARY_PATTERN)
}

pub(super) fn is_link_summary(item: &PasteSummary) -> bool {
    summary_matches_pattern(item, &LINK_SUMMARY_PATTERN)
}

pub(super) fn language_extension(language: Option<&str>) -> &'static str {
    match language
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "rust" => "rs",
        "python" => "py",
        "javascript" => "js",
        "typescript" => "ts",
        "json" => "json",
        "yaml" => "yaml",
        "toml" => "toml",
        "markdown" => "md",
        "html" => "html",
        "css" => "css",
        "sql" => "sql",
        "shell" => "sh",
        _ => "txt",
    }
}

pub(super) fn sanitize_filename(value: &str) -> String {
    let mut out: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect();
    out = out.trim().to_string();
    if out.is_empty() {
        "localpaste-export".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tags_csv_trims_and_dedupes_case_insensitively() {
        let parsed = parse_tags_csv(" rust,CLI, rust , cli ,");
        assert_eq!(parsed, vec!["rust".to_string(), "CLI".to_string()]);
    }

    #[test]
    fn language_extension_maps_known_and_unknown_languages() {
        assert_eq!(language_extension(Some("rust")), "rs");
        assert_eq!(language_extension(Some(" Python ")), "py");
        assert_eq!(language_extension(Some("unknown")), "txt");
        assert_eq!(language_extension(None), "txt");
    }

    #[test]
    fn sanitize_filename_replaces_reserved_chars_and_falls_back() {
        assert_eq!(sanitize_filename("bad<>:\"/\\|?*name"), "bad_________name");
        assert_eq!(sanitize_filename("   "), "localpaste-export");
    }
}
