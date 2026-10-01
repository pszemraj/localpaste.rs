//! Unit tests for check-ast-dupes CLI validation and workflow guarantees.

use super::*;
use tempfile::TempDir;

fn write_file(path: &Path, content: &str) {
    fs::write(path, content).expect("write fixture file");
}

fn base_args(root: PathBuf) -> Args {
    Args {
        root,
        threshold: 0.78,
        near_miss_threshold: 0.70,
        k: 5,
        min_nodes: 28,
        top: 60,
        near_miss_top: 25,
        max_dead: 80,
        include_tests: false,
        include_pub: false,
        allow_parse_errors: false,
        fail_on_findings: false,
    }
}

#[test]
fn jaccard_is_stable() {
    let left = HashSet::from([1u64, 2u64, 3u64]);
    let right = HashSet::from([2u64, 3u64, 4u64]);
    let (score, overlap, union) = jaccard(&left, &right);
    assert!((score - 0.5).abs() < f64::EPSILON);
    assert_eq!(overlap, 2);
    assert_eq!(union, 4);
}

#[test]
fn infer_base_module_handles_mod_file() {
    let module = infer_base_module(Path::new("crates/localpaste_gui/src/app/mod.rs"));
    assert_eq!(
        module,
        vec!["localpaste_gui".to_string(), "app".to_string()]
    );
}

#[test]
fn fail_on_findings_accounts_for_near_miss_only_results() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path().join("src");
    fs::create_dir_all(&root).expect("create src dir");
    let file = root.join("lib.rs");
    write_file(
        &file,
        r#"
        pub fn alpha(v: i32) -> i32 { v + 1 }
        pub fn beta(v: i32) -> i32 { v * 2 }
        "#,
    );

    let mut args = base_args(root);
    args.threshold = 1.0;
    args.near_miss_threshold = 0.0;
    args.k = 1;
    args.min_nodes = 1;
    args.fail_on_findings = true;

    let err = run(args).expect_err("near-miss findings should fail when requested");
    assert!(
        err.contains("findings detected"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn parse_errors_fail_by_default_and_can_be_explicitly_allowed() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path().join("src");
    fs::create_dir_all(&root).expect("create src dir");
    write_file(root.join("good.rs").as_path(), "pub fn ok() -> i32 { 1 }");
    write_file(
        root.join("bad.rs").as_path(),
        "pub fn broken( { let x = 1; }",
    );

    let args = base_args(root.clone());
    let err = run(args).expect_err("parse errors should fail by default");
    assert!(
        err.contains("parse errors detected"),
        "unexpected error: {}",
        err
    );

    let mut allow_args = base_args(root);
    allow_args.allow_parse_errors = true;
    run(allow_args).expect("allow-parse-errors should continue past parse failures");
}

#[test]
fn parse_helpers_reject_out_of_range_values() {
    assert!(parse_unit_interval("1.1").is_err());
    assert!(parse_positive_usize("0").is_err());
}

#[test]
fn test_module_context_applies_to_inline_and_external_helpers() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path().join("src");
    fs::create_dir_all(root.join("support")).expect("create fixture directories");
    write_file(
        &root.join("lib.rs"),
        r#"
        #[cfg(test)]
        mod resolver_checks {
            fn test_helper() {}
            #[test] fn uses_helper() { test_helper(); }
        }
        #[cfg(test)]
        #[path = "support.rs"]
        mod fixture_support;
        pub struct State;
        #[cfg(test)]
        impl State {
            fn test_method() { fixture_support::setup(); }
        }
        "#,
    );
    write_file(
        &root.join("support.rs"),
        "mod nested; pub(crate) fn setup() { nested::setup_nested(); }",
    );
    write_file(
        &root.join("support/nested.rs"),
        "pub(crate) fn setup_nested() {}",
    );

    let mut args = base_args(root.clone());
    args.fail_on_findings = true;
    run(args).expect("test-only helpers must not appear in the production audit");
    let files = collect_rust_files(&root).expect("collect fixtures");
    let scan = scan_sources(temp.path(), files.clone(), 5, false);
    assert!(scan.functions.is_empty());
    let scan = scan_sources(temp.path(), files, 5, true);
    assert_eq!(scan.functions.len(), 5);
    assert!(scan.functions.iter().all(|info| info.has_cfg));

    write_file(&root.join("unused.rs"), "fn unused_production_helper() {}");
    let mut args = base_args(root);
    args.fail_on_findings = true;
    assert!(
        run(args).is_err(),
        "real unreferenced helpers must still fail"
    );
}

#[test]
fn derive_attribute_callbacks_count_as_live_references() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path().join("src");
    fs::create_dir_all(&root).expect("create fixture directory");
    write_file(
        &root.join("main.rs"),
        r#"
        #[derive(Parser)]
        struct Args {
            #[arg(value_parser = parse_limit)]
            limit: usize,
            #[arg(value_parser = parsers::parse_count)]
            count: usize,
        }
        fn parse_limit(raw: &str) -> usize { parse_number(raw) }
        fn parse_number(raw: &str) -> usize { raw.parse().unwrap() }
        mod parsers;
        fn main() {}
        "#,
    );
    write_file(
        &root.join("parsers.rs"),
        "pub(crate) fn parse_count(raw: &str) -> usize { raw.len() }
         pub fn default_count() -> usize { parse_count(\"one\") }",
    );
    let mut args = base_args(root);
    args.fail_on_findings = true;
    run(args).expect("attribute callbacks and their callees must remain live");
}

#[test]
fn production_declaration_keeps_shared_test_support_in_the_audit() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path();
    write_file(
        &root.join("lib.rs"),
        r#"
        #[cfg(test)] #[path = "shared.rs"] mod checks;
        #[path = "shared.rs"] mod production;
    "#,
    );
    write_file(&root.join("shared.rs"), "fn unused_shared_helper() {}");
    let scan = scan_sources(root, collect_rust_files(root).unwrap(), 5, false);
    assert_eq!(scan.functions.len(), 1);
    assert!(!scan.functions[0].has_cfg);
    let mut args = base_args(root.to_path_buf());
    args.fail_on_findings = true;
    assert!(run(args).is_err());
}

#[test]
fn test_cfg_detection_preserves_possible_production_code() {
    for (predicate, test_only) in [
        ("test", true),
        ("all(test, unix)", true),
        ("any(test, all(test, windows))", true),
        ("not(test)", false),
        ("any(test, unix)", false),
        ("feature = \"test-support\"", false),
    ] {
        let item: syn::ItemFn = syn::parse_str(&format!("#[cfg({predicate})] fn sample() {{}}"))
            .expect("parse cfg fixture");
        assert_eq!(has_cfg_attr(&item.attrs), test_only, "{predicate}");
    }
}
