//! Derived-term extraction regressions.

use super::{derive, MAX_TERM_CHARS};

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
