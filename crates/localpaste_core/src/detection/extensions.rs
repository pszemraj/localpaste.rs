//! Preferred text export extensions, independent of syntax-highlighting availability.

/// Select the conventional extension for a recognized text format.
///
/// This table covers the pinned detector's text labels and manual language choices.
/// It deliberately does not depend on the installed highlighting grammar set.
///
/// # Returns
/// A filename extension without the dot; unknown formats use `txt`.
pub fn preferred_extension(language: Option<&str>) -> &'static str {
    let raw = language.unwrap_or_default().trim().to_ascii_lowercase();
    let canonical = super::canonical::canonicalize(&raw);
    let label = if raw == "jsonl" {
        raw.as_str()
    } else {
        canonical.as_str()
    };
    match label {
        "aidl" => "aidl",
        "appleplist" => "plist",
        "asm" => "s",
        "asp" => "aspx",
        "autohotkey" => "ahk",
        "autoit" => "au3",
        "awk" => "awk",
        "batch" => "bat",
        "bazel" => "bzl",
        "bib" => "bib",
        "c" => "c",
        "clojure" => "clj",
        "cmake" => "cmake",
        "cobol" => "cbl",
        "coffeescript" => "coffee",
        "cpp" => "cpp",
        "cs" => "cs",
        "csproj" => "csproj",
        "css" => "css",
        "csv" => "csv",
        "dart" => "dart",
        "diff" => "diff",
        "dm" => "dm",
        "dockerfile" => "dockerfile",
        "dxf" => "dxf",
        "elixir" => "ex",
        "eml" => "eml",
        "erb" => "erb",
        "erlang" => "erl",
        "fortran" => "f90",
        "gemfile" => "gemfile",
        "gemspec" => "gemspec",
        "gitattributes" => "gitattributes",
        "gitmodules" => "gitmodules",
        "go" => "go",
        "gradle" => "gradle",
        "groovy" => "groovy",
        "handlebars" => "hbs",
        "haskell" => "hs",
        "hcl" => "hcl",
        "htaccess" => "htaccess",
        "html" => "html",
        "ics" => "ics",
        "ignorefile" => "gitignore",
        "ini" => "ini",
        "internetshortcut" => "url",
        "ipynb" => "ipynb",
        "java" => "java",
        "javascript" => "js",
        "jinja" => "jinja",
        "json" => "json",
        "jsonl" => "jsonl",
        "julia" => "jl",
        "kotlin" => "kt",
        "latex" => "tex",
        "lisp" => "lisp",
        "log" => "log",
        "lua" => "lua",
        "m3u" => "m3u8",
        "m4" => "m4",
        "makefile" => "makefile",
        "markdown" => "md",
        "matlab" => "m",
        "mht" => "mht",
        "mum" => "mum",
        "objectivec" => "m",
        "ocaml" => "ml",
        "pascal" => "pas",
        "pem" => "pem",
        "perl" => "pl",
        "php" => "php",
        "po" => "po",
        "powershell" => "ps1",
        "prolog" => "pl",
        "proteindb" => "pdb",
        "proto" => "proto",
        "python" => "py",
        "r" => "r",
        "randomtxt" => "txt",
        "rdf" => "rdf",
        "rst" => "rst",
        "rtf" => "rtf",
        "ruby" => "rb",
        "rust" => "rs",
        "sass" => "sass",
        "scala" => "scala",
        "scss" => "scss",
        "sgml" => "sgml",
        "shell" => "sh",
        "smali" => "smali",
        "solidity" => "sol",
        "sql" => "sql",
        "srt" => "srt",
        "stltext" => "stl",
        "sum" => "sum",
        "svg" => "svg",
        "swift" => "swift",
        "tcl" => "tcl",
        "text" => "txt",
        "textproto" => "textproto",
        "toml" => "toml",
        "tsv" => "tsv",
        "twig" => "twig",
        "txt" => "txt",
        "typescript" => "ts",
        "vba" => "vbs",
        "vcxproj" => "vcxproj",
        "verilog" => "v",
        "vhdl" => "vhd",
        "vtt" => "vtt",
        "vue" => "vue",
        "winregistry" => "reg",
        "xml" => "xml",
        "yaml" => "yaml",
        "yara" => "yar",
        "zig" => "zig",
        _ => "txt",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_cover_tables_documents_aliases_and_unhighlighted_formats() {
        for (label, extension) in [
            ("csv", "csv"),
            ("tsv", "tsv"),
            ("md", "md"),
            ("tex", "tex"),
            ("rst", "rst"),
            ("restructuredtext", "rst"),
            ("jsonl", "jsonl"),
            ("ipynb", "ipynb"),
            ("cmake", "cmake"),
            ("PowerShell", "ps1"),
            ("text", "txt"),
            ("unknown-format", "txt"),
        ] {
            assert_eq!(preferred_extension(Some(label)), extension, "{label}");
        }
        assert_eq!(preferred_extension(None), "txt");
        for option in super::super::canonical::MANUAL_LANGUAGE_OPTIONS {
            if option.value != "text" {
                assert_ne!(
                    preferred_extension(Some(option.value)),
                    "txt",
                    "{}",
                    option.value
                );
            }
        }
    }
}
