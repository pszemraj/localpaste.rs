//! Language canonicalization and shared manual selection options.

/// Manual language option metadata for UI selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManualLanguageOption {
    pub label: &'static str,
    pub value: &'static str,
}

/// Sorted language options for manual selection.
pub const MANUAL_LANGUAGE_OPTIONS: &[ManualLanguageOption] = &[
    ManualLanguageOption {
        label: "C",
        value: "c",
    },
    ManualLanguageOption {
        label: "C++",
        value: "cpp",
    },
    ManualLanguageOption {
        label: "C#",
        value: "cs",
    },
    ManualLanguageOption {
        label: "CSS",
        value: "css",
    },
    ManualLanguageOption {
        label: "Dart",
        value: "dart",
    },
    ManualLanguageOption {
        label: "Elixir",
        value: "elixir",
    },
    ManualLanguageOption {
        label: "Go",
        value: "go",
    },
    ManualLanguageOption {
        label: "HTML",
        value: "html",
    },
    ManualLanguageOption {
        label: "Java",
        value: "java",
    },
    ManualLanguageOption {
        label: "JavaScript",
        value: "javascript",
    },
    ManualLanguageOption {
        label: "JSON",
        value: "json",
    },
    ManualLanguageOption {
        label: "Kotlin",
        value: "kotlin",
    },
    ManualLanguageOption {
        label: "LaTeX",
        value: "latex",
    },
    ManualLanguageOption {
        label: "Lua",
        value: "lua",
    },
    ManualLanguageOption {
        label: "Markdown",
        value: "markdown",
    },
    ManualLanguageOption {
        label: "Perl",
        value: "perl",
    },
    ManualLanguageOption {
        label: "Plain text",
        value: "text",
    },
    ManualLanguageOption {
        label: "PowerShell",
        value: "powershell",
    },
    ManualLanguageOption {
        label: "Python",
        value: "python",
    },
    ManualLanguageOption {
        label: "Rust",
        value: "rust",
    },
    ManualLanguageOption {
        label: "Shell",
        value: "shell",
    },
    ManualLanguageOption {
        label: "SQL",
        value: "sql",
    },
    ManualLanguageOption {
        label: "Swift",
        value: "swift",
    },
    ManualLanguageOption {
        label: "TOML",
        value: "toml",
    },
    ManualLanguageOption {
        label: "TypeScript",
        value: "typescript",
    },
    ManualLanguageOption {
        label: "XML",
        value: "xml",
    },
    ManualLanguageOption {
        label: "YAML",
        value: "yaml",
    },
    ManualLanguageOption {
        label: "Zig",
        value: "zig",
    },
];

/// Convert aliases/legacy names to canonical labels.
///
/// # Returns
/// Canonical, lowercase label (or empty string for empty/whitespace input).
pub fn canonicalize(language: &str) -> String {
    let lowered = language.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "csharp" | "c#" => "cs".to_string(),
        "c++" => "cpp".to_string(),
        "bash" | "sh" | "zsh" => "shell".to_string(),
        "pwsh" | "ps1" => "powershell".to_string(),
        "yml" => "yaml".to_string(),
        "jsonl" => "json".to_string(),
        "js" => "javascript".to_string(),
        "ts" => "typescript".to_string(),
        "md" => "markdown".to_string(),
        "plaintext" | "plain text" | "plain" | "txt" => "text".to_string(),
        "py" => "python".to_string(),
        "rs" => "rust".to_string(),
        "rb" => "ruby".to_string(),
        "kt" => "kotlin".to_string(),
        "m" | "mm" | "objc" | "objective-c" => "objectivec".to_string(),
        "pl" => "perl".to_string(),
        "ex" | "exs" => "elixir".to_string(),
        "scss" | "sass" => "css".to_string(),
        _ => lowered,
    }
}

/// Find the friendly label for a canonical/manual language value.
///
/// # Returns
/// The display label for a known manual option, otherwise `None`.
pub fn manual_option_label(value: &str) -> Option<&'static str> {
    let canonical = canonicalize(value);
    MANUAL_LANGUAGE_OPTIONS
        .iter()
        .find(|option| option.value == canonical)
        .map(|option| option.label)
}
