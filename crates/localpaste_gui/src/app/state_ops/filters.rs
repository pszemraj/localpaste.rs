//! Helper filters and classifiers for app state operations.

use crate::app::SidebarCollection;
use crate::backend::PasteSummary;
use localpaste_core::semantic::PasteKind;

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
        "cs",
        "shell",
        "powershell",
        "sql",
        "html",
        "css",
        "scss",
        "sass",
        "markdown",
        "dart",
        "zig",
        "lua",
        "perl",
        "elixir",
    ],
    name_needles: &[
        ".rs", ".py", ".js", ".ts", ".go", ".java", ".cs", ".sql", ".sh", "snippet", "script",
        ".ps1", ".rb", ".php", ".cpp", ".kt", ".swift", ".lua", ".pl", ".zig", "class", "function",
        "cargo ", "pytest ", "python ", "docker ", "kubectl ", "npm ", "pnpm ", "make ", "just ",
    ],
    tag_needles: &["code", "snippet", "script"],
};

const CONFIG_SUMMARY_PATTERN: SummaryPattern = SummaryPattern {
    languages: &["json", "yaml", "toml", "xml", "dockerfile", "makefile"],
    name_needles: &[
        "config",
        "settings",
        ".env",
        ".yml",
        ".yaml",
        ".json",
        ".toml",
        ".xml",
        ".ini",
        ".cfg",
        ".conf",
        "dockerfile",
        "compose",
        "docker-compose",
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
    name_needles: &[
        "log",
        "logs",
        "trace",
        "stderr",
        "stdout",
        "error",
        ".out",
        ".err",
        "traceback",
        "panic",
        "crash",
        "journalctl",
    ],
    tag_needles: &[
        "log",
        "logs",
        "trace",
        "stderr",
        "stdout",
        "error",
        "traceback",
        "panic",
    ],
};

const LINK_SUMMARY_PATTERN: SummaryPattern = SummaryPattern {
    languages: &[],
    name_needles: &["http://", "https://", "www.", "url", "link", "links"],
    tag_needles: &["url", "link", "links", "bookmark"],
};

const CODE_FILE_SUFFIXES: &[&str] = &[
    ".rs", ".py", ".js", ".ts", ".go", ".java", ".cs", ".sql", ".sh", ".ps1", ".rb", ".php",
    ".cpp", ".c", ".h", ".hpp", ".kt", ".swift", ".lua", ".pl", ".zig",
];
const CONFIG_FILE_SUFFIXES: &[&str] = &[
    ".json", ".yml", ".yaml", ".toml", ".xml", ".ini", ".cfg", ".conf", ".env",
];
const LOG_FILE_SUFFIXES: &[&str] = &[".log", ".out", ".err", ".trace"];
const COMMAND_NAME_PREFIXES: &[&str] = &[
    "cargo ",
    "pytest ",
    "python ",
    "uv ",
    "pip ",
    "git ",
    "docker ",
    "kubectl ",
    "npm ",
    "pnpm ",
    "make ",
    "just ",
    "curl ",
    "wget ",
    "ssh ",
    "torchrun ",
];

/// Parses comma-separated tags, trimming whitespace and removing case-insensitive duplicates.
///
/// # Returns
/// Ordered unique tag list preserving first-seen casing.
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
    let canonical = localpaste_core::detection::canonical::canonicalize(language);
    values
        .iter()
        .any(|value| canonical.eq_ignore_ascii_case(value))
}

/// Normalizes an optional language filter into canonical storage form.
///
/// # Returns
/// Canonical language filter string, or `None` when unset/blank.
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

fn name_has_suffix_ci(value: &str, suffixes: &[&str]) -> bool {
    let value_lower = value.trim().to_ascii_lowercase();
    suffixes.iter().any(|suffix| value_lower.ends_with(suffix))
}

fn name_starts_with_any_ci(value: &str, prefixes: &[&str]) -> bool {
    let value_lower = value.trim().to_ascii_lowercase();
    prefixes
        .iter()
        .any(|prefix| value_lower.starts_with(prefix))
}

fn looks_like_url_name(value: &str) -> bool {
    let value_lower = value.trim().to_ascii_lowercase();
    !value_lower.contains(char::is_whitespace)
        && (value_lower.starts_with("http://")
            || value_lower.starts_with("https://")
            || value_lower.starts_with("www.")
            || value_lower.contains("://"))
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

fn summary_has_kind(item: &PasteSummary, kind: PasteKind) -> bool {
    item.derived.kind == kind
}

fn summary_matches_kind_pattern_and_name(
    item: &PasteSummary,
    kind: PasteKind,
    pattern: &SummaryPattern,
    extra_name_match: bool,
) -> bool {
    summary_has_kind(item, kind) || summary_matches_pattern(item, pattern) || extra_name_match
}

/// Returns whether a summary matches one of the semantic sidebar collections.
///
/// # Arguments
/// - `item`: Sidebar summary under evaluation.
/// - `collection`: Semantic collection bucket to test.
///
/// # Returns
/// `true` when derived kind or legacy summary heuristics match the requested
/// semantic collection bucket.
pub(super) fn matches_semantic_collection(
    item: &PasteSummary,
    collection: SidebarCollection,
) -> bool {
    match collection {
        SidebarCollection::Code => summary_matches_kind_pattern_and_name(
            item,
            PasteKind::Code,
            &CODE_SUMMARY_PATTERN,
            name_has_suffix_ci(item.name.as_str(), CODE_FILE_SUFFIXES)
                || name_starts_with_any_ci(item.name.as_str(), COMMAND_NAME_PREFIXES),
        ),
        SidebarCollection::Config => summary_matches_kind_pattern_and_name(
            item,
            PasteKind::Config,
            &CONFIG_SUMMARY_PATTERN,
            name_has_suffix_ci(item.name.as_str(), CONFIG_FILE_SUFFIXES),
        ),
        SidebarCollection::Logs => summary_matches_kind_pattern_and_name(
            item,
            PasteKind::Log,
            &LOG_SUMMARY_PATTERN,
            name_has_suffix_ci(item.name.as_str(), LOG_FILE_SUFFIXES),
        ),
        SidebarCollection::Links => summary_matches_kind_pattern_and_name(
            item,
            PasteKind::Link,
            &LINK_SUMMARY_PATTERN,
            looks_like_url_name(item.name.as_str()),
        ),
        _ => false,
    }
}

/// Maps canonical language labels to preferred export file extensions.
///
/// # Returns
/// Extension without leading dot, defaulting to `"txt"`.
pub(super) fn language_extension(language: Option<&str>) -> &'static str {
    let canonical =
        localpaste_core::detection::canonical::canonicalize(language.unwrap_or_default().trim());
    match canonical.as_str() {
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
        "scss" => "scss",
        "sass" => "sass",
        "sql" => "sql",
        "shell" => "sh",
        "cs" => "cs",
        "cpp" => "cpp",
        "c" => "c",
        "go" => "go",
        "java" => "java",
        "kotlin" => "kt",
        "swift" => "swift",
        "ruby" => "rb",
        "php" => "php",
        "perl" => "pl",
        "lua" => "lua",
        "r" => "r",
        "scala" => "scala",
        "dart" => "dart",
        "elixir" => "ex",
        "haskell" => "hs",
        "zig" => "zig",
        "xml" => "xml",
        "dockerfile" => "dockerfile",
        "makefile" => "makefile",
        "powershell" => "ps1",
        _ => "txt",
    }
}

/// Sanitizes a filename candidate for cross-platform export compatibility.
///
/// # Returns
/// Safe filename with reserved characters replaced by `_`.
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
        assert_eq!(language_extension(Some("csharp")), "cs");
        assert_eq!(language_extension(Some("bash")), "sh");
        assert_eq!(language_extension(Some("scss")), "scss");
        assert_eq!(language_extension(Some("unknown")), "txt");
        assert_eq!(language_extension(None), "txt");
    }

    #[test]
    fn sanitize_filename_replaces_reserved_chars_and_falls_back() {
        assert_eq!(sanitize_filename("bad<>:\"/\\|?*name"), "bad_________name");
        assert_eq!(sanitize_filename("   "), "localpaste-export");
    }

    #[test]
    fn smart_summary_heuristics_cover_suffixes_commands_and_urls() {
        let base = PasteSummary {
            id: "id".to_string(),
            name: "sample".to_string(),
            language: None,
            content_len: 10,
            updated_at: chrono::Utc::now(),
            folder_id: None,
            tags: Vec::new(),
            derived: Default::default(),
        };

        let code = PasteSummary {
            name: "cargo test --workspace".to_string(),
            ..base.clone()
        };
        let config = PasteSummary {
            name: "docker-compose.override.yml".to_string(),
            ..base.clone()
        };
        let log = PasteSummary {
            name: "panic.stderr".to_string(),
            ..base.clone()
        };
        let link = PasteSummary {
            name: "https://example.com/docs".to_string(),
            ..base
        };

        assert!(matches_semantic_collection(&code, SidebarCollection::Code));
        assert!(matches_semantic_collection(
            &config,
            SidebarCollection::Config
        ));
        assert!(matches_semantic_collection(&log, SidebarCollection::Logs));
        assert!(matches_semantic_collection(&link, SidebarCollection::Links));
    }

    #[test]
    fn derived_kind_overrides_weak_name_and_tag_heuristics() {
        let base = PasteSummary {
            id: "id".to_string(),
            name: "plain".to_string(),
            language: None,
            content_len: 10,
            updated_at: chrono::Utc::now(),
            folder_id: None,
            tags: Vec::new(),
            derived: Default::default(),
        };

        let code = PasteSummary {
            derived: localpaste_core::semantic::DerivedMeta {
                kind: PasteKind::Code,
                handle: Some("fn handle_request".to_string()),
                terms: vec!["handle_request".to_string()],
            },
            ..base.clone()
        };
        let config = PasteSummary {
            derived: localpaste_core::semantic::DerivedMeta {
                kind: PasteKind::Config,
                handle: Some("model gpt-4".to_string()),
                terms: vec!["gpt-4".to_string()],
            },
            ..base.clone()
        };
        let log = PasteSummary {
            derived: localpaste_core::semantic::DerivedMeta {
                kind: PasteKind::Log,
                handle: Some("panic failed".to_string()),
                terms: vec!["panic".to_string()],
            },
            ..base.clone()
        };
        let link = PasteSummary {
            derived: localpaste_core::semantic::DerivedMeta {
                kind: PasteKind::Link,
                handle: Some("example.com".to_string()),
                terms: vec!["example".to_string()],
            },
            ..base
        };

        assert!(matches_semantic_collection(&code, SidebarCollection::Code));
        assert!(matches_semantic_collection(
            &config,
            SidebarCollection::Config
        ));
        assert!(matches_semantic_collection(&log, SidebarCollection::Logs));
        assert!(matches_semantic_collection(&link, SidebarCollection::Links));
    }
}
