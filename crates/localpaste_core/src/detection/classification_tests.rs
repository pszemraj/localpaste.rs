//! Review regression corpus for Makefiles and ordinary notes.

use super::*;
use crate::models::paste::{Paste, PasteMeta};
use crate::semantic::{derive, PasteKind};

const MAKEFILES: &[&str] = &[
    "# Build the project\nSRCS = a.c \\\n       b.c\nall: $(SRCS)\n\t$(CC) $(SRCS) -o app\n",
    "# Build\nCC = gcc\noverride CFLAGS += -g\nall:\n\t$(CC) main.c\n",
    "# Search paths\nvpath %.c src\nall: main.c\n\tcc $< -o $@\n",
    "# Build\nall:\n\t$(call compile,main.c)\n",
    "# Build\nall:\n\t[ -d out ] || mkdir out\n",
    "# Build\n$(info Building the project)\nall:\n\tcc main.c\n",
    "# Build\nall:\n\tproject-builder --release\n",
    "# Build\nall:\n\t./build\n",
    "# Build\nall:\n\tproject-builder $FLAGS\n",
    "# Build\nall:\n\trsync ./source ./dest\n",
];
const NOTES: &[&str] = &[
    "# Shopping\nDairy:\n\tmilk\n\teggs\n",
    "# Todo\nToday:\n\tcall the bank\n\treply to emails\n",
    "# Budget\nToday:\n\tbudget is $5\n\tspend $10\n",
    "Let me know if the function works for you",
    "Let me know if the function works for you (please)",
    "Let me know if the function works for you; thanks",
    "make sure it's done before friday",
];

#[test]
fn tabbed_python_definitions_are_not_makefile_targets() {
    for source in [
        "def main():\n\tpass\n",
        "def main():\n\tprint(\"a=b\")\n",
        "def main():\n\treturn x > 1\n",
        "# Values\nclass Values:\n\tresult = 2\n",
    ] {
        for body in [source.to_string(), source.replace('\n', "\r\n")] {
            assert_eq!(detect_language(&body).as_deref(), Some("python"), "{body}");
        }
    }
}

#[test]
fn makefile_constructs_and_lowercase_notes_keep_their_meaning() {
    for content in MAKEFILES {
        for body in [content.to_string(), content.replace('\n', "\r\n")] {
            assert_eq!(
                refine_magika_label("makefile", &body).as_deref(),
                Some("makefile"),
                "{body}"
            );
            let paste = Paste::new(body.clone(), "build".into());
            assert_eq!(paste.language.as_deref(), Some("makefile"), "{body}");
            assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Config);
            assert_eq!(preferred_extension(paste.language.as_deref()), "makefile");
            assert_eq!(paste.content, body);
            assert!(paste.language_is_manual);
            assert_eq!(derive(&body, Some("markdown")).kind, PasteKind::Code);
        }
    }
    for content in NOTES {
        for body in [
            content.to_string(),
            content.replace('\n', "\r\n"),
            format!("{content}\n"),
            format!("{content}\r\n"),
        ] {
            let paste = Paste::new(body.clone(), "note".into());
            assert_eq!(
                PasteMeta::from(&paste).derived.kind,
                PasteKind::Document,
                "{body}: {:?}",
                paste.language
            );
            assert_eq!(derive(&body, None).kind, PasteKind::Document, "{body}");
            assert!(!matches!(
                heuristic::detect(&body).as_deref(),
                Some("shell" | "javascript")
            ));
            if body.starts_with('#') {
                assert_eq!(
                    refine_magika_label("makefile", &body).as_deref(),
                    Some("markdown")
                );
                assert_eq!(derive(&body, Some("markdown")).kind, PasteKind::Document);
            }
        }
    }
    for body in [
        "# Build\n.RECIPEPREFIX := >\nall:\n>project-builder\n",
        "# Build\nall:\n\tproject-builder\n",
    ] {
        assert_eq!(
            refine_magika_label("makefile", body).as_deref(),
            Some("makefile")
        );
    }
    for body in [
        "function run() { let result = 1; return result; }",
        "make 'sure it is done'",
        "make build \"before the release\"",
        "make sure --release",
    ] {
        assert_eq!(
            PasteMeta::from(&Paste::new(body.into(), "source".into()))
                .derived
                .kind,
            PasteKind::Code
        );
    }
    for (body, language) in [
        ("# Shopping\nDairy:\n\tmilk\n\teggs\n", "shell"),
        (
            "# Todo\nToday:\n\tcall the bank\n\treply to emails\n",
            "makefile",
        ),
        ("Let me know if the function works for you", "javascript"),
    ] {
        let paste =
            Paste::new_with_language(body.into(), "old note".into(), Some(language.into()), true);
        assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Document);
        assert_eq!(paste.language.as_deref(), Some(language));
        assert!(paste.language_is_manual);
    }
    for body in [
        "throw new Error",
        "let value\nfunction example",
        "you instanceof the",
        "// let the function finish for you",
    ] {
        assert_eq!(derive(body, Some("javascript")).kind, PasteKind::Code);
    }
}
