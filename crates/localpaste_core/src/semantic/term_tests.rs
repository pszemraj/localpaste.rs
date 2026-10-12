//! Derived-term extraction regressions.

use super::{derive, normalize_term, MAX_HANDLE_ATOM_CHARS, MAX_HANDLE_CHARS, MAX_TERM_CHARS};

#[test]
fn giant_single_token_yields_no_terms_or_synthesized_handle() {
    for len in [20_000, 200_000] {
        let derived = derive("x".repeat(len).as_str(), Some("text"));
        assert!(
            derived.terms.is_empty(),
            "a {len}-char token must not become a term: {:?}",
            derived.terms.iter().map(String::len).collect::<Vec<_>>()
        );
        assert_eq!(derived.handle, None);
    }
}

#[test]
fn giant_token_does_not_displace_real_terms_or_pollute_the_handle() {
    let body = format!("{}\nfsdp2 cublaslt validation\n", "x".repeat(20_000));
    let derived = derive(body.as_str(), Some("text"));
    assert!(derived.terms.iter().any(|term| term == "fsdp2"));
    assert!(derived
        .terms
        .iter()
        .all(|term| term.len() <= MAX_TERM_CHARS));
    assert!(derived
        .handle
        .as_deref()
        .is_some_and(|handle| !handle.contains("xxx")));
}

#[test]
fn term_length_cap_keeps_boundary_token_and_drops_longer_ones() {
    let at_cap = "k".repeat(MAX_TERM_CHARS);
    let over_cap = "q".repeat(MAX_TERM_CHARS + 1);

    let kept = derive(at_cap.as_str(), Some("text"));
    assert_eq!(kept.terms, vec![at_cap.clone()]);

    let dropped = derive(over_cap.as_str(), Some("text"));
    assert!(dropped.terms.is_empty());

    let mixed = derive(format!("{over_cap} {at_cap}").as_str(), Some("text"));
    assert_eq!(mixed.terms, vec![at_cap]);
}

#[test]
fn realistic_long_identifiers_remain_terms() {
    let identifiers = [
        "std::collections::hash_map::entry",
        "HIGHLIGHT_DEBOUNCE_LARGE_BYTES",
        "my-long-kebab-case-package-name-v2",
    ];
    let body = format!("{}\n{}\n", identifiers.join(" "), identifiers.join(" "));
    let derived = derive(body.as_str(), Some("text"));
    for identifier in identifiers {
        assert!(
            identifier.len() <= MAX_TERM_CHARS,
            "fixture exceeds the cap: {identifier}"
        );
        assert!(
            derived
                .terms
                .iter()
                .any(|term| term.eq_ignore_ascii_case(identifier)),
            "{identifier} missing from {:?}",
            derived.terms
        );
    }
}

/// Assert the derived handle of each `(content, handle)` pair.
fn assert_handles(language: Option<&str>, cases: &[(&str, &str)]) {
    for (content, handle) in cases {
        let derived = derive(content, language);
        assert_eq!(derived.handle.as_deref(), Some(*handle), "{content:?}");
    }
}

#[test]
fn observed_pastes_get_clean_terms_and_synthesized_handles() {
    for (content, language, terms, handle) in [
        (
            "quick note: call the vet tomorrow",
            "text",
            &["quick", "note", "call", "tomorrow"][..],
            "quick note",
        ),
        (
            "Hi Sam,\nplease review and deploy.\nThanks",
            "text",
            &["please", "review", "deploy", "thanks"][..],
            "please review",
        ),
        (
            "name: web\nservice: api\nreplicas: 3\n",
            "yaml",
            &["name", "service", "replicas", "web"][..],
            "name web",
        ),
        (
            "https://example.com/docs/getting-started",
            "text",
            &["getting-started", "example.com", "docs"][..],
            "example.com",
        ),
        (
            "http://localhost:8080/api",
            "text",
            &["localhost:8080", "api"][..],
            "localhost:8080",
        ),
    ] {
        let derived = derive(content, Some(language));
        assert_eq!(derived.terms, terms, "{content:?}");
        assert_eq!(derived.handle.as_deref(), Some(handle), "{content:?}");
    }
}

#[test]
fn key_names_count_once_and_only_url_schemes_are_stopwords() {
    // `name` and `name:` used to be counted as two distinct terms.
    let repeated = derive("name: a\nname: b\nname: c\n", Some("yaml"));
    assert_eq!(repeated.terms.first().map(String::as_str), Some("name"));

    // `www` is a stopword only on its own, not as the front of a host.
    let host = derive("www.example.com", Some("text"));
    assert_eq!(host.terms, ["www.example.com"]);
}

#[test]
fn edge_trimming_keeps_internal_punctuation() {
    let identifiers = [
        "std::collections::hash_map::entry",
        "localpaste.rs",
        "p.id",
        "max_paste_bytes",
        "v1.2.3",
        "my-long-kebab-case-name",
        // Underscores are identifier characters and are not trimmed.
        "__init__",
        "_private",
    ];
    for identifier in identifiers {
        for body in [
            format!("see {identifier}"),
            format!("see {identifier}."),
            format!("see {identifier}:"),
            format!("see {identifier}..."),
            format!("see {identifier}-"),
            format!("see --{identifier}"),
            format!("see {identifier}, then stop"),
        ] {
            let derived = derive(body.as_str(), Some("text"));
            assert!(
                derived.terms.iter().any(|term| term == identifier),
                "{identifier} missing from {:?} for {body:?}",
                derived.terms
            );
        }
    }
}

#[test]
fn tokens_that_are_short_after_trimming_are_dropped() {
    let derived = derive("ab. cd: ef- --- ... :: x-.", Some("text"));
    assert!(derived.terms.is_empty(), "{:?}", derived.terms);
    assert_eq!(derived.handle, None);

    // `1.` and `10.` trim to digits only, which are never terms.
    assert!(derive("1. 10. 200:", Some("text")).terms.is_empty());
}

#[test]
fn terms_need_a_letter_so_dates_times_and_numbers_are_dropped() {
    let numeric = derive(
        "deployed 1.2.3 at 2026-10-10 06:00:01 from 127.0.0.1",
        Some("text"),
    );
    assert_eq!(numeric.terms, ["deployed"]);
    assert_eq!(numeric.handle.as_deref(), Some("deployed"));

    // A letter anywhere keeps a token, so error codes and versions stay terms.
    let coded = derive("error E0308 in v1.2 on node-04", Some("text"));
    assert_eq!(coded.terms, ["node-04", "e0308", "v1.2"]);

    // Timestamp and time-column handles lead with words, not a date or time.
    for (content, language, handle) in [
        (
            "2026-10-10 06:00:01 INFO starting scheduler\n2026-10-10 06:00:02 INFO loaded 12 jobs",
            None,
            "info starting",
        ),
        (
            "2026-10-10 06:00:01,423 INFO Starting\n2026-10-10 06:00:02,117 ERROR Failed",
            None,
            "info starting",
        ),
        (
            "ts,count\n2026-10-10 06:00:01,42\n2026-10-10 06:00:02,17",
            Some("csv"),
            "count",
        ),
    ] {
        let derived = derive(content, language);
        assert!(
            derived
                .terms
                .iter()
                .all(|term| term.chars().any(char::is_alphabetic)),
            "{content:?}: {:?}",
            derived.terms
        );
        assert_eq!(derived.handle.as_deref(), Some(handle), "{content:?}");
    }
}

#[test]
fn iso_timestamps_and_dates_are_not_terms() {
    // ISO 8601's `T` and `Z` designators are not the letter a term needs.
    for timestamp in [
        "2026-10-09T14:02:11.384Z",
        "2026-10-09T14:02:11Z",
        "2026-10-09T14:02:11-05:00",
        "2026-10-09T14:02:11+02:00",
        "20261009T140211Z",
        "2026-10-09T14",
        "2026-10-09",
        "14:02:11.384",
        "14:02:11.384Z",
    ] {
        let derived = derive(&format!("deployed {timestamp} on node-04"), Some("text"));
        assert_eq!(derived.terms, ["node-04", "deployed"], "{timestamp}");
    }
    // Identifiers and locations keep their letters, `t` and `z` included.
    let coded = derive(
        "E0308 in v1.2.3 on node-04 at client.rs:212:38 for t3.micro and gzip",
        Some("text"),
    );
    for term in ["e0308", "v1.2.3", "node-04", "client.rs:212:38"] {
        assert!(
            coded.terms.iter().any(|found| found == term),
            "{term}: {:?}",
            coded.terms
        );
    }
    assert_eq!(
        derive("t3.micro gzip2", Some("text")).terms,
        ["t3.micro", "gzip2"]
    );

    let log = derive(super::log_tests::PANIC_AFTER_SERVICE_LOG, Some("log"));
    assert!(
        log.terms.iter().all(|term| !term.starts_with("2026")),
        "{:?}",
        log.terms
    );
    // A Log's handle names its first leveled event, not a location or timestamp.
    assert_eq!(log.handle.as_deref(), Some("info checkout::api"));
    assert_eq!(
        derive(super::log_tests::PANIC_AFTER_SERVICE_LOG, None).handle,
        log.handle
    );
}

#[test]
fn command_handles_keep_path_operands_whole_or_cut_them() {
    assert_handles(
        None,
        &[
            ("cd ~/projects/app", "cd ~/projects/app"),
            ("cd ~/projects/app\ncargo build", "cd ~/projects/app"),
            (
                "cd C:\\Users\\me\\app\ncargo build",
                "cd C:\\Users\\me\\app",
            ),
            ("ls ./a/b.txt", "ls ./a/b.txt"),
            ("scp user@host:/srv/x.tgz .", "scp user@host:/srv/x.tgz"),
            // The operand stops at the first character that is not path-like,
            // so the handle never contains text that is absent from the paste.
            ("ls *.txt", "ls"),
            ("echo $HOME/bin", "echo"),
            ("grep foo; ls", "grep foo"),
            ("rm %TEMP%\\x", "rm"),
            ("cp file.txt>out", "cp file.txt"),
        ],
    );
}

#[test]
fn handle_operands_are_capped_at_a_prefix_of_the_paste_text() {
    let path = "~/projects/a-very-long-directory-name/with/nested/parts";
    assert!(path.len() > MAX_HANDLE_ATOM_CHARS);
    let derived = derive(format!("cd {path}\ncargo build").as_str(), None);
    let handle = derived.handle.expect("command handle");
    let operand = handle.strip_prefix("cd ").expect("cd handle");
    assert_eq!(operand.chars().count(), MAX_HANDLE_ATOM_CHARS);
    assert!(path.starts_with(operand), "{operand:?}");
}

#[test]
fn config_handles_keep_path_values_and_unmangled_text() {
    assert_handles(
        Some("yaml"),
        &[
            (
                "dataset: /data/train/v2\nmodel: gpt-4\n",
                "dataset /data/train/v2",
            ),
            ("project: ~/projects/app\n", "project ~/projects/app"),
            ("image: ghcr.io/org/app:1.2\n", "image ghcr.io/org/app:1.2"),
            ("name = \"build\\out\"\n", "name build\\out"),
            // Surrounding quotes and a trailing comma are not part of the value.
            ("{\n  \"name\": \"web\",\n}\n", "name web"),
            ("name: web,\n", "name web"),
            ("name: web, api\n", "name web, api"),
            // Flow-style one-liners still open with their container.
            ("{\"name\":\"Ada\"}", "name Ada"),
        ],
    );
}

#[test]
fn config_keys_must_match_exactly_so_prose_does_not_become_a_config_handle() {
    // Keeping spaces in values must not let a prose line pose as `service ...`:
    // the whole text before the separator has to be a preferred key.
    assert_handles(
        Some("text"),
        &[
            ("service is down: restart it", "service down"),
            ("name of the file = report.txt", "name file"),
        ],
    );
}

#[test]
fn config_values_keep_internal_spaces_and_cap_at_a_word_boundary() {
    assert_handles(
        Some("yaml"),
        &[
            ("name: My Project\n", "name My Project"),
            // Whitespace runs collapse to one space.
            ("name:   My \t  Project  \n", "name My Project"),
            ("name: \"My Project\"\n", "name My Project"),
            // The value ends at the first character outside the allowed set.
            ("name: web # prod\n", "name web"),
            ("name: \"My Project\", extra\n", "name My Project"),
            ("name: web {prod}\n", "name web"),
            // Past the cap only whole words are kept: 20 characters here, and
            // the next word would reach 26.
            (
                "task: Fix the flaky upload retry in the nightly build\n",
                "task Fix the flaky upload",
            ),
            // A value that fits exactly is kept whole.
            (
                "task: aaaa bbbb cccc dddd eeee\n",
                "task aaaa bbbb cccc dddd eeee",
            ),
        ],
    );

    // A first word longer than the cap is the only mid-word cut.
    let long_word = "w".repeat(MAX_HANDLE_ATOM_CHARS + 10);
    let derived = derive(format!("name: {long_word} tail\n").as_str(), Some("yaml"));
    assert_eq!(
        derived.handle.as_deref(),
        Some(format!("name {}", &long_word[..MAX_HANDLE_ATOM_CHARS]).as_str())
    );
}

#[test]
fn assigned_values_are_not_copied_into_handles() {
    // The variable name is the useful part of the handle; the value after `=`
    // is often a secret or noise.
    assert_handles(
        None,
        &[
            ("export API_KEY=sk-live-abc123", "export API_KEY"),
            (
                "export API_KEY=sk-live-abc123\ncargo build",
                "export API_KEY",
            ),
            ("export RUST_LOG=debug\ncargo run", "export RUST_LOG"),
            ("set FOO=bar\ncargo build", "set FOO"),
            ("set /p prompt=Enter your full name", "set /p"),
        ],
    );
    // Config values stop at `=` too, and a URL query is cut at `?` before that.
    assert_handles(
        Some("yaml"),
        &[
            ("name: a=b\n", "name a"),
            ("name: token=sk-live-abc123\n", "name token"),
            (
                "project: https://example.com/p?id=1\n",
                "project https://example.com/p",
            ),
        ],
    );

    for content in [
        "export API_KEY=sk-live-abc123",
        "set TOKEN=sk-live-abc123\ncargo build",
        "name: token=sk-live-abc123",
    ] {
        let handle = derive(content, None).handle.expect("handle");
        assert!(!handle.contains("sk-live"), "{content:?} -> {handle:?}");
    }
}

#[test]
fn non_ascii_words_stay_whole_and_symbols_separate_them() {
    let derived = derive("naïve café ☕ — 日本語 🚀", Some("text"));
    // A run of CJK letters without spaces is one token; emoji and dashes are
    // separators, never term characters.
    assert_eq!(derived.terms, ["naïve", "café", "日本語"]);
    assert_eq!(derived.handle.as_deref(), Some("naïve café"));

    assert_eq!(derive("über über", Some("text")).terms, ["über"]);
    assert!(derive("🚀🚀🚀 ☕☕☕ —— ★★★", Some("text"))
        .terms
        .is_empty());
}

#[test]
fn unicode_terms_lowercase_fully_and_query_terms_normalize_the_same_way() {
    let derived = derive("ÜBER Straße ÉCOLE Μάθημα", Some("text"));
    assert_eq!(derived.terms, ["über", "straße", "école", "μάθημα"]);
    for (query, term) in [
        ("--Release", "release"),
        ("Note:", "note"),
        ("CAFÉ.", "café"),
        ("ÉCOLE", "école"),
    ] {
        assert_eq!(normalize_term(query), term);
    }
}

#[test]
fn term_length_limits_count_characters_not_bytes() {
    // 30 two-byte letters are 60 bytes but well within the 48-character cap.
    let word = "é".repeat(30);
    assert_eq!(derive(word.as_str(), Some("text")).terms, [word.as_str()]);
    let at_cap = "ü".repeat(MAX_TERM_CHARS);
    assert_eq!(
        derive(at_cap.as_str(), Some("text")).terms,
        [at_cap.as_str()]
    );
    let over_cap = "ü".repeat(MAX_TERM_CHARS + 1);
    assert!(derive(over_cap.as_str(), Some("text")).terms.is_empty());
    // Two characters stay below the three-character minimum despite four bytes.
    assert!(derive("éé ßß", Some("text")).terms.is_empty());
}

#[test]
fn handles_keep_non_ascii_text_and_cap_on_character_boundaries() {
    assert_handles(
        Some("yaml"),
        &[("name: café-münchen-projekt\n", "name café-münchen-projekt")],
    );
    assert_handles(
        None,
        &[("cd ~/Été/projets\ncargo build", "cd ~/Été/projets")],
    );
    assert_handles(
        Some("python"),
        &[("def größe(x):\n    return x\n", "def größe")],
    );

    let long_value = "ü".repeat(MAX_HANDLE_ATOM_CHARS + 10);
    let config = derive(format!("name: {long_value}\n").as_str(), Some("yaml"));
    let expected = format!("name {}", "ü".repeat(MAX_HANDLE_ATOM_CHARS));
    assert_eq!(config.handle.as_deref(), Some(expected.as_str()));
    let command = derive(format!("cd ~/{long_value}\ncargo build").as_str(), None);
    let expected = format!("cd ~/{}", "ü".repeat(MAX_HANDLE_ATOM_CHARS - 2));
    assert_eq!(command.handle.as_deref(), Some(expected.as_str()));

    // A handle synthesized from two long terms is cut at 48 characters.
    let (first, second) = ("é".repeat(40), "ü".repeat(40));
    let derived = derive(format!("{first} {second}").as_str(), Some("text"));
    let handle = derived.handle.expect("synthesized handle");
    assert_eq!(handle.chars().count(), MAX_HANDLE_CHARS);
    assert_eq!(
        handle,
        format!("{first} {}", "ü".repeat(MAX_HANDLE_CHARS - 41))
    );
}

#[test]
fn definition_handles_skip_visibility_and_modifier_qualifiers() {
    assert_handles(
        Some("rust"),
        &[
            (
                "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}",
                "fn add",
            ),
            (
                "pub async fn fetch(url: &str) -> Result<String, Error> {\n    todo!()\n}",
                "fn fetch",
            ),
            (
                "pub(crate) fn helper(x: &str) -> usize {\n    x.len()\n}",
                "fn helper",
            ),
            (
                "pub(in crate::db) struct Row {\n    id: u64,\n}",
                "struct Row",
            ),
            ("pub const unsafe fn raw() {}", "fn raw"),
            (
                "pub unsafe extern \"C\" fn callback(x: i32) {}",
                "fn callback",
            ),
            ("unsafe impl Send for Handle {}", "impl Send"),
            ("pub enum Kind {\n    A,\n}", "enum Kind"),
        ],
    );
    assert_handles(
        Some("typescript"),
        &[
            (
                "export function render(props) {\n  return null;\n}",
                "export function render",
            ),
            ("export async function load() {}", "export function load"),
            ("export default function App() {}", "export function App"),
            ("export default class Store {}", "export class Store"),
            ("export abstract class Base {}", "export class Base"),
            ("async function main() {}", "function main"),
            ("declare const VERSION: string;", "const VERSION"),
        ],
    );
    assert_handles(
        Some("python"),
        &[
            (
                "async def fetch(url):\n    return await get(url)",
                "def fetch",
            ),
            (
                "@app.get(\"/\")\nasync def root():\n    return {}",
                "def root",
            ),
        ],
    );
    assert_handles(
        Some("go"),
        &[(
            "func (s *Server) Handle(w http.ResponseWriter, r *http.Request) {}",
            "func Handle",
        )],
    );
    // Words that only begin like a qualifier or keyword are not definitions.
    for (line, language) in [
        ("public void run() {}", "rust"),
        ("fnord = 3", "rust"),
        ("default: return 0;", "javascript"),
        ("asyncio.run(main())", "python"),
        ("extern crate serde;", "rust"),
    ] {
        assert_eq!(
            super::extract_definition_handle_from_line(line, Some(language)),
            None,
            "{line}"
        );
    }
}
