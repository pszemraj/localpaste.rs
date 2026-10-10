//! Leading-item evidence for short Rust function and impl snippets.

use crate::text::{utf8_prefix_by_bytes, TEXT_SAMPLE_MAX_BYTES};

/// Recognize a short Rust snippet that opens with a function or impl item.
///
/// Keyword scoring needs two Rust keywords, so a lone function such as
/// `fn handle_request(input: &str) -> usize { ... }` scores one hit. This check
/// instead reads the structure: after leading blank lines, `//` comments,
/// attributes, and `use` declarations, the first line must open a `fn` item
/// (with optional `pub`, `pub(..)`, `const`, `async`, `unsafe`, or `extern`
/// qualifiers and generics) or an `impl` block, and a `{` must open a body.
/// Zig, V, Gleam, and Elixir also write `fn`, so the sample must also contain
/// syntax those languages lack: a `->` return type, a `&` reference parameter
/// or `self` receiver, a `::` path, `let mut`, a macro call such as
/// `println!(`, or the `impl` header itself. An unindented prose line after the
/// item marks a note that quotes a signature, which is not source.
///
/// # Arguments
/// - `content`: Paste body, inspected within the shared text sample limit.
///
/// # Returns
/// Whether the leading item and the body read as Rust source.
pub(super) fn leading_item(content: &str) -> bool {
    let sample = utf8_prefix_by_bytes(content.trim(), TEXT_SAMPLE_MAX_BYTES);
    let mut lines = sample
        .lines()
        .take(512)
        .filter(|line| !line.trim().is_empty());
    let Some(head) = lines.by_ref().map(str::trim).find(|line| {
        !(line.starts_with("//")
            || line.starts_with("#[")
            || line.starts_with("#!")
            || (line.starts_with("use ") && line.ends_with(';')))
    }) else {
        return false;
    };
    let impl_head = impl_header(head);
    if !impl_head && !fn_header(head) {
        return false;
    }
    let mut body = head.contains('{');
    for line in lines {
        if !line.starts_with(char::is_whitespace) && prose_line(line) {
            return false;
        }
        body |= line.contains('{');
    }
    body && (impl_head || rust_only_syntax(sample))
}

/// Match `[qualifiers] fn name[<generics>](` at the start of a line.
fn fn_header(line: &str) -> bool {
    let mut rest = line;
    loop {
        let stripped = if let Some(scope) = rest.strip_prefix("pub(") {
            scope.split_once(')').map(|(_, tail)| tail)
        } else if let Some(abi) = rest.strip_prefix("extern \"") {
            abi.split_once('"').map(|(_, tail)| tail)
        } else {
            ["pub ", "const ", "async ", "unsafe ", "extern "]
                .iter()
                .find_map(|qualifier| rest.strip_prefix(qualifier))
        };
        match stripped {
            Some(tail) => rest = tail.trim_start(),
            None => break,
        }
    }
    let Some(rest) = rest.strip_prefix("fn ") else {
        return false;
    };
    let rest = rest.trim_start();
    let name_len = rest
        .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .unwrap_or(rest.len());
    if name_len == 0 || rest.starts_with(|ch: char| ch.is_numeric()) {
        return false;
    }
    let mut rest = rest[name_len..].trim_start();
    if rest.starts_with('<') {
        let Some(end) = generics_end(rest) else {
            return false;
        };
        rest = rest[end..].trim_start();
    }
    rest.starts_with('(')
}

/// Byte offset just past the `>` closing a leading generic parameter list.
///
/// A `->` inside a bound such as `<F: Fn() -> T>` does not close the list.
fn generics_end(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut previous = ' ';
    for (idx, ch) in text.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if previous != '-' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx + 1);
                }
            }
            _ => {}
        }
        previous = ch;
    }
    None
}

/// Match an `impl` block header that opens its body on the same line.
fn impl_header(line: &str) -> bool {
    let line = line.strip_prefix("unsafe ").unwrap_or(line);
    (line.starts_with("impl ") || line.starts_with("impl<")) && line.ends_with('{')
}

/// Syntax that Rust has and the other `fn` languages lack.
fn rust_only_syntax(sample: &str) -> bool {
    [
        "->",
        "::",
        "let mut ",
        ": &",
        "&mut ",
        "&self)",
        "&self,",
        "(self)",
        "(self,",
        "(mut self",
    ]
    .iter()
    .any(|token| sample.contains(token))
        || sample.match_indices('!').any(|(idx, _)| {
            sample[..idx]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
                && sample[idx + 1..].starts_with(['(', '[', '{'])
        })
}

/// A line of three or more plain words, such as a sentence after a snippet.
/// A `where T: Clone` clause also has only words, but it is not prose.
fn prose_line(line: &str) -> bool {
    let words: Vec<_> = line.split_whitespace().collect();
    words.len() >= 3
        && words[0] != "where"
        && words.iter().all(|word| {
            let word = word.trim_end_matches(['.', ',', '!', '?', ';', ':']);
            !word.is_empty()
                && word
                    .chars()
                    .all(|ch| ch.is_alphabetic() || matches!(ch, '\'' | '’' | '-'))
        })
}

#[cfg(test)]
mod tests {
    use super::leading_item;
    use crate::detection::detect_language;
    use crate::models::paste::{Paste, PasteMeta};
    use crate::semantic::PasteKind;

    const FUNCTION: &str = "fn handle_request(input: &str) -> usize {\n    input.len()\n}";

    #[test]
    fn short_rust_items_detect_as_rust_and_classify_as_code() {
        for source in [
            FUNCTION,
            "fn handle_request(input: &str) -> usize { input.len() }",
            "pub(crate) async fn load(&self) -> Result<(), Error> {\n    Ok(())\n}",
            "pub const unsafe fn raw(ptr: *const u8) -> u8 {\n    *ptr\n}",
            "pub extern \"C\" fn tick(counter: &mut u64) {\n    *counter += 1;\n}",
            "fn into_inner(self) -> T {\n    self.value\n}",
            "fn parse<T: FromStr>(s: &str) -> Option<T> {\n    s.parse().ok()\n}",
            "fn apply<F: Fn(u32) -> u32>(f: F) -> u32 {\n    f(1)\n}",
            "fn sum(\n    a: u32,\n    b: u32,\n) -> u32 {\n    a + b\n}",
            "fn first<T>(items: &[T]) -> Option<&T>\nwhere\n    T: Clone,\n{\n    items.first()\n}",
            "fn render() -> String {\n    \"<html><body></body></html>\".to_string()\n}",
            "/// Adds two numbers.\n#[inline]\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}",
            "use std::fmt;\nfn show() {}",
            "fn greet(name: String) {\n    println!(\"hi {name}\");\n}",
            "impl Default for Config {\n    fn default() -> Self {\n        Self { retries: 3 }\n    }\n}",
            "fn main() {\n    panic!(\"boom\");\n}\nthread 'main' panicked at src/main.rs:2:5",
        ] {
            for body in [source.to_owned(), source.replace('\n', "\r\n")] {
                assert!(leading_item(&body), "{body}");
                assert_eq!(detect_language(&body).as_deref(), Some("rust"), "{body}");
                let paste = Paste::new(body.clone(), "snippet".into());
                assert_eq!(PasteMeta::from(&paste).derived.kind, PasteKind::Code, "{body}");
            }
        }
        let meta = PasteMeta::from(&Paste::new(FUNCTION.into(), "handler".into()));
        assert_eq!(meta.derived.handle.as_deref(), Some("fn handle_request"));
        // An unlabeled fence is classified by its heuristic body language.
        let fence = Paste::new(format!("```\n{FUNCTION}\n```"), "fence".into());
        assert_eq!(fence.language.as_deref(), Some("markdown"));
        assert_eq!(PasteMeta::from(&fence).derived.kind, PasteKind::Code);
    }

    #[test]
    fn other_fn_languages_prose_and_wrappers_are_not_rust_items() {
        for content in [
            // Zig, V, Mojo, and Elixir also write `fn`.
            "pub fn add(a: i32, b: i32) i32 {\n    return a + b;\n}",
            "pub fn main() !void {\n    const out = std.io.getStdOut().writer();\n    try out.print(\"hi\", .{});\n}",
            "fn add(a int, b int) int {\n\treturn a + b\n}",
            "fn add(a: Int, b: Int) -> Int:\n    return a + b",
            "fn(x) -> x * 2 end",
            "Enum.map(list, fn x -> x * 2 end)",
            // An item without a body or without Rust-only syntax.
            "fn main() {}",
            "fn handle_request(input: &str) -> usize",
            "fn foo() -> Bar is the signature I want",
            // Prose, notes, Markdown, and logs that quote a signature.
            "The fn handle_request(input: &str) -> usize function returns the length.",
            "fn handle_request(input: &str) -> usize {\n    input.len()\n}\nthat's the signature I want, what do you think?",
            "Here is a function:\n\n```rust\nfn handle_request(input: &str) -> usize {\n    input.len()\n}\n```",
            "# Notes\n\nfn handle_request(input: &str) -> usize {\n    input.len()\n}",
            "- fn handle_request(input: &str) -> usize {}\n- fn other() {}",
            "2026-10-10 06:00:01 INFO fn handle_request(input: &str) -> usize { called",
        ] {
            assert!(!leading_item(content), "{content}");
        }
        for (content, language) in [
            ("The fn handle_request(input: &str) -> usize function returns the length.", None),
            ("Here is a function:\n\n```rust\nfn handle_request(input: &str) -> usize {\n    input.len()\n}\n```", Some("markdown")),
            ("# Notes\n\nfn handle_request(input: &str) -> usize {\n    input.len()\n}", Some("markdown")),
            ("2026-10-10 06:00:01 INFO fn handle_request(input: &str) -> usize { called", Some("log")),
        ] {
            assert_eq!(detect_language(content).as_deref(), language, "{content}");
        }
    }
}
