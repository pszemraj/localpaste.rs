//! Shared metadata collection rules for backend search and sidebar rendering.

use crate::backend::PasteSummary;
use chrono::{DateTime, Local, Utc};
use localpaste_core::semantic::PasteKind;

/// Smart collection applied to paste metadata before search limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarCollection {
    All,
    Today,
    Week,
    Recent,
    Unfiled,
    Code,
    Documents,
    Config,
    Logs,
    Links,
}

impl SidebarCollection {
    /// Stable value used to persist the active sidebar collection.
    ///
    /// # Returns
    /// The collection's storage label.
    pub(crate) fn storage_value(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Today => "today",
            Self::Week => "week",
            Self::Recent => "recent",
            Self::Unfiled => "unfiled",
            Self::Code => "code",
            Self::Documents => "documents",
            Self::Config => "config",
            Self::Logs => "logs",
            Self::Links => "links",
        }
    }

    /// Restores a collection from its persisted label.
    ///
    /// # Returns
    /// The recognized collection, or `None` for an unknown label.
    pub(crate) fn from_storage_value(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "today" => Some(Self::Today),
            "week" => Some(Self::Week),
            "recent" => Some(Self::Recent),
            "unfiled" => Some(Self::Unfiled),
            "code" => Some(Self::Code),
            "documents" => Some(Self::Documents),
            "config" => Some(Self::Config),
            "logs" => Some(Self::Logs),
            "links" => Some(Self::Links),
            _ => None,
        }
    }
}

/// Captures one set of time boundaries for a list/search projection.
///
/// # Returns
/// Local day, week cutoff day, and thirty-day UTC cutoff.
pub(crate) fn current_filter_cutoffs() -> (chrono::NaiveDate, chrono::NaiveDate, DateTime<Utc>) {
    let local_now = Local::now();
    let today = local_now.date_naive();
    (
        today,
        today - chrono::Duration::days(7),
        local_now.with_timezone(&Utc) - chrono::Duration::days(30),
    )
}

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
    languages: &[
        "json",
        "jsonl",
        "yaml",
        "toml",
        "xml",
        "dockerfile",
        "makefile",
    ],
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
const LOG_FILE_SUFFIXES: &[&str] = &[".log", ".out", ".err", ".trace", ".stdout", ".stderr"];
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

fn language_in_set(language: Option<&str>, values: &[&str]) -> bool {
    let Some(language) = language.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let canonical = localpaste_core::detection::canonical::canonicalize(language);
    values
        .iter()
        .any(|value| canonical.eq_ignore_ascii_case(value))
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

/// Tests document overrides using explicit metadata rather than title fragments.
fn document_override_matches(item: &PasteSummary, collection: SidebarCollection) -> bool {
    let name = item.name.trim().to_ascii_lowercase();
    let (pattern, name_match) = match collection {
        SidebarCollection::Code => (
            &CODE_SUMMARY_PATTERN,
            name_has_suffix_ci(&item.name, CODE_FILE_SUFFIXES)
                // These two command names also introduce ordinary prose titles;
                // a command-shaped body still supplies the authoritative Code kind.
                || COMMAND_NAME_PREFIXES.iter().any(|prefix|
                    !matches!(*prefix, "make " | "just ") && item.name.trim().starts_with(prefix)),
        ),
        SidebarCollection::Config => (
            &CONFIG_SUMMARY_PATTERN,
            name_has_suffix_ci(&item.name, CONFIG_FILE_SUFFIXES)
                || matches!(name.as_str(), "dockerfile" | "makefile"),
        ),
        SidebarCollection::Logs => (
            &LOG_SUMMARY_PATTERN,
            name_has_suffix_ci(&item.name, LOG_FILE_SUFFIXES),
        ),
        SidebarCollection::Links => (&LINK_SUMMARY_PATTERN, looks_like_url_name(&item.name)),
        _ => return false,
    };
    name_match
        || language_in_set(item.language.as_deref(), pattern.languages)
        || item.tags.iter().any(|tag| {
            tag.split(|ch: char| !ch.is_alphanumeric()).any(|word| {
                pattern
                    .tag_needles
                    .iter()
                    .any(|needle| word.eq_ignore_ascii_case(needle))
            })
        })
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
fn matches_semantic_collection(item: &PasteSummary, collection: SidebarCollection) -> bool {
    if localpaste_core::semantic::is_document_language(item.language.as_deref())
        && matches!(item.derived.kind, PasteKind::Other | PasteKind::Document)
    {
        return collection == SidebarCollection::Documents;
    }
    if summary_has_kind(item, PasteKind::Document) {
        return if collection == SidebarCollection::Documents {
            ![
                SidebarCollection::Code,
                SidebarCollection::Config,
                SidebarCollection::Logs,
                SidebarCollection::Links,
            ]
            .into_iter()
            .any(|collection| document_override_matches(item, collection))
        } else {
            document_override_matches(item, collection)
        };
    }
    match collection {
        SidebarCollection::Documents => false,
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

/// Tests one sidebar summary against active collection and language filters.
///
/// # Arguments
/// - `item`: Sidebar summary to test.
/// - `active_collection`: Active smart collection filter.
/// - `active_language_filter`: Optional active language filter.
/// - `today_local`: Current local calendar day.
/// - `week_cutoff_day`: Oldest local calendar day included in `This Week`.
/// - `recent_cutoff`: Oldest UTC instant included in `Recent`.
///
/// # Returns
/// `true` when the item should remain visible under the provided filters.
pub(crate) fn matches_active_filters(
    item: &PasteSummary,
    active_collection: &SidebarCollection,
    active_language_filter: Option<&str>,
    today_local: chrono::NaiveDate,
    week_cutoff_day: chrono::NaiveDate,
    recent_cutoff: DateTime<Utc>,
) -> bool {
    let updated_local_day = item.updated_at.with_timezone(&Local).date_naive();
    let collection_match = match active_collection {
        SidebarCollection::All => true,
        SidebarCollection::Today => updated_local_day == today_local,
        SidebarCollection::Week => updated_local_day >= week_cutoff_day,
        SidebarCollection::Recent => item.updated_at >= recent_cutoff,
        SidebarCollection::Unfiled => item.folder_id.is_none(),
        SidebarCollection::Code
        | SidebarCollection::Documents
        | SidebarCollection::Config
        | SidebarCollection::Logs
        | SidebarCollection::Links => matches_semantic_collection(item, active_collection.clone()),
    };
    if !collection_match {
        return false;
    }
    match active_language_filter {
        None => true,
        Some(lang) => {
            let canonical_filter = localpaste_core::detection::canonical::canonicalize(lang);
            item.language
                .as_deref()
                .map(localpaste_core::detection::canonical::canonicalize)
                .map(|value| value == canonical_filter)
                .unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_documents_ignore_title_fragments_and_partial_tags() {
        let mut item = PasteSummary {
            id: "prose".into(),
            name: "notes".into(),
            language: None,
            content_len: 30,
            updated_at: Utc::now(),
            folder_id: None,
            tags: vec!["blog".into(), "transcript".into()],
            derived: localpaste_core::semantic::DerivedMeta {
                kind: PasteKind::Document,
                ..Default::default()
            },
            match_excerpt: None,
        };
        for title in [
            "holographic-otter",
            "analog-badger",
            "Blog draft",
            "Changelog",
            "Technology notes",
            "Meeting transcript",
            "Product description",
            "Things to make tomorrow",
            "just notes",
            "make a note",
            "Python 3.12 is now installed",
            "Git is down for maintenance",
            "Brew is ready for deployment",
        ] {
            item.name = title.into();
            assert!(
                matches_semantic_collection(&item, SidebarCollection::Documents),
                "{title}"
            );
            for collection in [
                SidebarCollection::Code,
                SidebarCollection::Config,
                SidebarCollection::Logs,
                SidebarCollection::Links,
            ] {
                assert!(
                    !matches_semantic_collection(&item, collection.clone()),
                    "{title}: {collection:?}"
                );
            }
        }
        item.name = "notes".into();
        item.tags = vec!["server-logs".into()];
        assert!(matches_semantic_collection(&item, SidebarCollection::Logs));
        assert!(!matches_semantic_collection(
            &item,
            SidebarCollection::Documents
        ));
    }

    #[test]
    fn documents_exclude_code_even_with_code_titles_and_tags() {
        let mut item = PasteSummary {
            id: "doc".into(),
            name: "function snippet.rs".into(),
            language: Some("markdown".into()),
            content_len: 30,
            updated_at: Utc::now(),
            folder_id: None,
            tags: vec!["code".into()],
            derived: localpaste_core::semantic::DerivedMeta {
                kind: PasteKind::Document,
                ..Default::default()
            },
            match_excerpt: None,
        };
        assert!(matches_semantic_collection(
            &item,
            SidebarCollection::Documents
        ));
        assert!(!matches_semantic_collection(&item, SidebarCollection::Code));
        item.language = None;
        item.derived.kind = PasteKind::Document;
        assert!(matches_semantic_collection(&item, SidebarCollection::Code));
        assert!(!matches_semantic_collection(
            &item,
            SidebarCollection::Documents
        ));
        item.name = "notes".into();
        item.tags.clear();
        assert!(matches_semantic_collection(
            &item,
            SidebarCollection::Documents
        ));
        assert!(!matches_semantic_collection(&item, SidebarCollection::Code));
    }

    #[test]
    fn detected_fenced_snippets_and_documents_reach_their_semantic_collections() {
        for (content, kind, collection) in [
            (
                "```python\nprint('hello')\n```",
                PasteKind::Code,
                SidebarCollection::Code,
            ),
            (
                "```json\n{\"name\":\"Ada\"}\n```",
                PasteKind::Config,
                SidebarCollection::Config,
            ),
            (
                "{\"name\":\"Ada\"}\n{\"name\":\"Grace\"}\n",
                PasteKind::Config,
                SidebarCollection::Config,
            ),
            (
                "~~~sh\necho hello world\n~~~",
                PasteKind::Code,
                SidebarCollection::Code,
            ),
            (
                "```\necho hello world\n```",
                PasteKind::Code,
                SidebarCollection::Code,
            ),
            (
                "# Notes\n```python\nprint('hello')\n```",
                PasteKind::Document,
                SidebarCollection::Documents,
            ),
            (
                "```markdown\n# Read me\n```",
                PasteKind::Document,
                SidebarCollection::Documents,
            ),
            (
                "sudo systemctl restart nginx",
                PasteKind::Code,
                SidebarCollection::Code,
            ),
            ("echo hello world", PasteKind::Code, SidebarCollection::Code),
            (
                "INFO Starting the server",
                PasteKind::Log,
                SidebarCollection::Logs,
            ),
            (
                "[INFO] Server started successfully",
                PasteKind::Log,
                SidebarCollection::Logs,
            ),
            (
                "thread 'main' panicked at src/main.rs:12:5",
                PasteKind::Log,
                SidebarCollection::Logs,
            ),
            (
                "Python 3.12 is now installed on the workstation.",
                PasteKind::Document,
                SidebarCollection::Documents,
            ),
            (
                "Git is down for scheduled maintenance.",
                PasteKind::Document,
                SidebarCollection::Documents,
            ),
        ] {
            let paste = localpaste_core::models::paste::Paste::new(content.into(), "notes".into());
            let meta = localpaste_core::models::paste::PasteMeta::from(&paste);
            assert_eq!(meta.derived.kind, kind, "{content}: {:?}", meta.language);
            let item = PasteSummary {
                id: meta.id,
                name: meta.name,
                language: meta.language,
                content_len: meta.content_len,
                updated_at: meta.updated_at,
                folder_id: meta.folder_id,
                tags: meta.tags,
                derived: meta.derived,
                match_excerpt: None,
            };
            assert!(
                matches_semantic_collection(&item, collection.clone()),
                "{content}: {collection:?}"
            );
            if kind != PasteKind::Document {
                assert!(
                    !matches_semantic_collection(&item, SidebarCollection::Documents),
                    "{content}"
                );
            }
        }
    }

    #[test]
    fn smart_summary_heuristics_cover_suffixes_commands_and_urls() {
        let mut base = PasteSummary {
            id: "id".to_string(),
            name: "sample".to_string(),
            language: None,
            content_len: 10,
            updated_at: chrono::Utc::now(),
            folder_id: None,
            tags: Vec::new(),
            derived: Default::default(),
            match_excerpt: None,
        };
        base.derived.kind = PasteKind::Document;

        let explicit_document = PasteSummary {
            name: "deploy.log".to_string(),
            language: Some("markdown".to_string()),
            ..base.clone()
        };
        assert!(matches_semantic_collection(
            &explicit_document,
            SidebarCollection::Documents
        ));
        assert!(!matches_semantic_collection(
            &explicit_document,
            SidebarCollection::Logs
        ));
        let tagged_log = PasteSummary {
            tags: vec!["logs".to_string()],
            ..base.clone()
        };
        assert!(matches_semantic_collection(
            &tagged_log,
            SidebarCollection::Logs
        ));
        assert!(!matches_semantic_collection(
            &tagged_log,
            SidebarCollection::Documents
        ));

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
        for item in [code, config, log, link] {
            assert!(!matches_semantic_collection(
                &item,
                SidebarCollection::Documents
            ));
        }
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
            match_excerpt: None,
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
