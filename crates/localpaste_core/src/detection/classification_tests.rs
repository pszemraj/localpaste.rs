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
fn non_python_compound_shapes_keep_their_language() {
    for (source, label) in [
        ("function reduce(action) {\n switch (action.type) {\n case 'ADD':\n  run();\n  break;\n }\n}\n", "javascript"),
        ("#include <stdio.h>\nint main() {\n switch (op) {\n case 1:\n  run();\n  break;\n }\n}\n", "c"),
        ("public class Main {\n public static void main(String[] args) {\n switch (op) {\n case 1:\n  run();\n  break;\n }\n }\n}\n", "java"),
        ("else:\n  retries: 3\n  timeout: 10\n", "yaml"),
        ("switch (op) {\ncase 1:\n  run();\n  break;\n}\n", "c"),
        ("values = (\n switch (op) {\n case 1:\n  run();\n  break;\n }\n)\n", "c"),
    ] {
        for body in [source.to_owned(), source.replace('\n', "\r\n")] {
            assert!(!python::compound_body(&body), "{body}");
            assert_ne!(heuristic::detect(&body).as_deref(), Some("python"), "{body}");
            assert_eq!(refine_magika_label(label, &body).as_deref(), Some(label));
            assert_ne!(detect_language(&body).as_deref(), Some("python"), "{body}");
            #[cfg(feature = "magika")]
            if let Some(raw) = magika::detect(&body) {
                assert_eq!(detect_language(&body).as_deref(), Some(canonical::canonicalize(&raw).as_str()), "{body}");
            }
        }
    }
}

#[test]
fn unknown_make_tools_and_documentary_command_examples_stay_distinct() {
    for recipe in [
        "mvn clean install",
        "go mod tidy",
        "bundle exec rake test",
        "helm package",
        "zig build",
    ] {
        let source = format!("# Build\nbuild:\n\t{recipe}\n");
        for body in [source.clone(), source.replace('\n', "\r\n")] {
            assert!(!crate::semantic::makefile_note_body(&body), "{body}");
            assert_eq!(
                refine_magika_label("makefile", &body).as_deref(),
                Some("makefile"),
                "{body}"
            );
            let paste = Paste::new(body.clone(), "build".into());
            assert_eq!(paste.language.as_deref(), Some("makefile"), "{body}");
            assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Config);
            assert!(matches!(
                heuristic::detect(&body).as_deref(),
                Some("makefile" | "shell")
            ));
        }
    }
    for source in [
        "ALL:\n\tzig build\n",
        "Build:\n\tzig build\n",
        "Build: src/main.zig\n\tzig build\n",
        "Build:\n\tzig build\nclean:\n\tzig clean\n",
        ".PHONY: Build\nBuild:\n\tzig build\n",
        ".PHONY: example\nexample:\n\tzig build\n",
        "example:\n\tzig build\nclean:\n\tzig clean\n",
        "example:\n\tzig build\nsteps:\n\tzig build\n",
        ".PHONY: Agenda\nAgenda:\n\treview budget\n",
        "Agenda: budget.txt\n\treview budget\n",
        "Agenda:\n\treview budget\nBuild:\n\tzig build\n",
    ] {
        for body in [source.to_owned(), source.replace('\n', "\r\n")] {
            assert!(!crate::semantic::makefile_note_body(&body), "{body}");
            assert_eq!(
                detect_language(&body).as_deref(),
                Some("makefile"),
                "{body}"
            );
            assert_eq!(
                refine_magika_label("makefile", &body).as_deref(),
                Some("makefile")
            );
        }
    }
    for source in [
        "# Usage\n\nExample:\n\tfoo --bar\n",
        "## Install\nSteps:\n\tpip install foo\n",
    ] {
        for body in [source.to_owned(), source.replace('\n', "\r\n")] {
            #[cfg(feature = "magika")]
            assert_eq!(magika::detect(&body).as_deref(), Some("markdown"), "{body}");
            assert_eq!(
                refine_magika_label("markdown", &body).as_deref(),
                Some("markdown"),
                "{body}"
            );
            let paste = Paste::new(body.clone(), "instructions".into());
            assert_eq!(paste.language.as_deref(), Some("markdown"), "{body}");
            assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Document);
        }
    }
}

#[test]
fn tabbed_notes_without_markdown_and_two_word_items_get_document_labels() {
    for source in [
        "Shopping list:\n\tmilk\n\teggs\n",
        "Agenda:\n\treview budget\n",
        "Todo:\n\tcall mom\n\tbuy milk\n",
    ] {
        for body in [source.to_owned(), source.replace('\n', "\r\n")] {
            assert_eq!(
                refine_magika_label("makefile", &body).as_deref(),
                Some("markdown"),
                "{body}"
            );
            let paste = Paste::new(body.clone(), "note".into());
            assert_eq!(paste.language.as_deref(), Some("markdown"), "{body}");
            assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Document);
        }
    }
}

#[test]
fn deeply_nested_assignment_detection_has_a_bounded_scan() {
    for target in [
        format!("{}x{}", "(".repeat(30_000), ")".repeat(30_000)),
        format!("({}last)", "item, ".repeat(8_000)),
    ] {
        let body = format!("if ready:\n\t{target} = 1\n");
        let started = std::time::Instant::now();
        assert!(python::compound_body(&body));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }
    for target in [
        "'['", "'('", "x[", "([x)]", "x[]", "(x,,y)", "',['", "',[x]'", "\",[\"",
    ] {
        assert!(!python::compound_body(&format!(
            "if ready:\n\t{target} = 1\n"
        )));
    }
}

#[test]
fn tabbed_python_source_is_not_a_makefile_target() {
    for source in [
        "def main():\n\tpass\n",
        "def main():\n\tprint(\"a=b\")\n",
        "def main():\n\treturn x > 1\n",
        "# Values\nclass Values:\n\tresult = 2\n",
        "#!/usr/bin/env python3\nif ready:\n\tresult = 1\n",
        "if ready:\n\tresult = 1\n",
        "while ready:\n\tcount += 1\n",
        "if ready:\n\tmask <<= 1\n",
        "if ready:\n\tmask >>= 1\n",
        "if ready:\n\tmatrix @= rotation\n",
        "try:\n\tx = 1\nexcept:\n\tx = 2\n",
        "if ready:\n\tprint('hello')\n",
        "if ready:\n\tx = lambda n: n + 1\n",
        "if ready:\n\tprice = '$5'\n",
        "if ready:\n\tprint('$5')\n",
        "if ready:\n\tself.result = 1\n",
        "if ready:\n\tresults[0] = 1\n",
        "if ready:\n\tx, y = point\n",
        "if ready:\n\t(x, (y, z)) = point\n",
        "if ready:\n\t[first, *remaining] = items\n",
        "if ready:\n\t(élément, état[0]) = values\n",
        "if ready:\n\tself.values['x'][0] = 1\n",
        "if ready:\n\tcache['x'] = value\n",
        "if ready:\n\tvalue: int = 1\n",
        "if ready:\n\tvalue: int\n",
        "if ready:\n\tresult = 1 # costs $5\n",
        "if ready:\n\tcache['cost $5'] = value # costs $5\n",
        "if ready:\n\tvalue: dict[str, int] = {'count': 1}\n",
        "if ready:\n\tvalue = {\n\t\t'count': 1,\n\t}\n",
        "if ready:\n\tvalues = [\n\t\t{\n\t\t\t'count': 1,\n\t\t},\n\t]\n",
        "if ready:\n\tprint({\n\t\t'count': 1,\n\t})\n",
        "def result():\n\treturn {\n\t\t'count': 1,\n\t}\n",
        "if ready:\n\tvalue = 1; print(value)\n",
        "if ready:\n\tglobal result\n\tresult = 1\n",
        "if ready:\n\tnonlocal result\n\tresult = 1\n",
        "if ready:\n\tmessage = '''Hello\nworld\n'''\n\tprint(message)\n",
        "if ready:\n\tmessage = \"\"\"Hello\nworld # costs $5\n\"\"\"\n\tprint(message)\n",
        "if (\n\tready\n):\n\tresult = 1\n",
        "if (ready\n\tand enabled\n):\n\tresult = 1\n",
        "if ready:\n\tresult = first + \\\n\t\tsecond\n",
        "for item in items:\n\tprocess(item)\n",
        "with open('data') as stream:\n\tdata = stream.read()\n",
    ] {
        for body in [source.to_string(), source.replace('\n', "\r\n")] {
            assert_eq!(detect_language(&body).as_deref(), Some("python"), "{body}");
            assert_eq!(
                heuristic::detect(&body).as_deref(),
                Some("python"),
                "{body}"
            );
            let paste = Paste::new(body.clone(), "Python source".into());
            assert!(paste.language_is_manual);
            assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Code);
            assert_eq!(preferred_extension(paste.language.as_deref()), "py");
            assert_eq!(paste.content, body);
        }
    }
    for source in [
        "if ready:\n\t$(CC) main.c\n",
        "while ready:\n\tcc main.c\n",
        "try:\n\tproject-builder --release\n",
        "if ready:\n\tCC = $(COMPILER)\n",
        ".PHONY: try\ntry:\n\tDEBUG=1\nall:\n\tcc main.c\n",
        "try:\n\tDEBUG=1\n.PHONY: try\n",
        "try:\n\tDEBUG=1\nall:\n\tcc main.c\n",
        "try:\n\tDEBUG=1\ninclude rules.mk\n",
        "define python_script\nimport os\nprint(os.getcwd())\nendef\nall:\n\tcc main.c\n",
        "define python_script\nif ready:\n\tmessage = '''Hello\nworld\n'''\nendef\nall:\n\tcc main.c\n",
        ".PHONY: try\ntry:\n\techo first \\\n\t\tsecond\nall:\n\tcc main.c\n",
    ] {
        assert_eq!(
            detect_language(source).as_deref(),
            Some("makefile"),
            "{source}"
        );
        let paste = Paste::new(source.into(), "Makefile".into());
        assert!(paste.language_is_manual);
        assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Config);
        assert_eq!(preferred_extension(paste.language.as_deref()), "makefile");
    }
}

#[test]
fn source_shebang_precedes_makefile_shaped_body() {
    for (shebang, language) in [
        ("#!/bin/bash", "shell"),
        ("#!/usr/bin/env node", "javascript"),
        ("#!/usr/bin/perl", "perl"),
    ] {
        let source = format!("{shebang}\nall:\n\tcc main.c\n");
        assert_eq!(detect_language(&source).as_deref(), Some(language));
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
