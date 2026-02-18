//! AST-normalized duplicate and likely-dead internal symbol checker.
//!
//! Duplicate detection:
//! - Function-level only.
//! - AST node sequence normalization anonymizes identifiers and literal values.
//! - Jaccard similarity is computed over k-gram shingles.
//!
//! Likely-dead detection:
//! - Conservative heuristic for internal (non-`pub`) free functions.
//! - Flags symbols with zero confidently-resolved callers.
//! - Skips trait impl methods and cfg/test escape hatches.

use clap::Parser;
use quote::ToTokens;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::{collections::hash_map::DefaultHasher, ffi::OsStr};
use syn::visit::Visit;
use syn::{Attribute, File, ImplItem, ItemFn, ItemImpl, ItemMod, Visibility};
use walkdir::WalkDir;

#[path = "check_ast_dupes/normalize.rs"]
mod normalize;
use normalize::{collect_call_refs, AstNormalizer};
#[path = "check_ast_dupes/similarity.rs"]
mod similarity;
use similarity::{find_similarity_pairs, print_duplicate_findings, print_near_miss_findings};

#[derive(Debug, Parser)]
#[command(
    name = "check-ast-dupes",
    about = "Find AST-normalized duplicate functions and likely dead internal symbols"
)]
struct Args {
    /// Root directory to scan recursively.
    #[arg(long, default_value = "crates")]
    root: PathBuf,

    /// Jaccard threshold (0.0 to 1.0) for duplicate pairs.
    #[arg(long, default_value_t = 0.78)]
    threshold: f64,

    /// Lower bound (0.0 to 1.0) for near-miss similarity reporting.
    #[arg(long, default_value_t = 0.70)]
    near_miss_threshold: f64,

    /// k for k-gram shingles.
    #[arg(long, default_value_t = 5)]
    k: usize,

    /// Minimum normalized node-sequence length before duplicate comparison.
    #[arg(long, default_value_t = 28)]
    min_nodes: usize,

    /// Maximum duplicate pairs to print.
    #[arg(long, default_value_t = 60)]
    top: usize,

    /// Maximum near-miss pairs to print.
    #[arg(long, default_value_t = 25)]
    near_miss_top: usize,

    /// Maximum likely-dead symbols to print.
    #[arg(long, default_value_t = 80)]
    max_dead: usize,

    /// Include files under tests/ and cfg(test)/#[test] items.
    #[arg(long, default_value_t = false)]
    include_tests: bool,

    /// Include externally public (pub) symbols in dead-symbol checks.
    #[arg(long, default_value_t = false)]
    include_pub: bool,

    /// Fail with non-zero exit code when findings are present.
    #[arg(long, default_value_t = false)]
    fail_on_findings: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum VisibilityKind {
    Public,
    Crate,
    Restricted,
    Private,
}

#[derive(Debug, Clone)]
struct FunctionInfo {
    id: usize,
    symbol: String,
    module_path: Vec<String>,
    simple_name: String,
    file: PathBuf,
    line: usize,
    vis: VisibilityKind,
    is_method: bool,
    is_trait_impl_method: bool,
    has_cfg: bool,
    is_test: bool,
    allow_dead_code: bool,
    normalized_nodes: Vec<String>,
    shingles: HashSet<u64>,
    call_refs: Vec<CallRef>,
}

#[derive(Debug, Clone)]
struct CallRef {
    segments: Vec<String>,
    is_method: bool,
}

#[derive(Debug)]
struct DeadFinding {
    id: usize,
    refs_total: usize,
    refs_outside_module: usize,
}

fn main() {
    run(Args::parse()).unwrap_or_else(|message| {
        eprintln!("error: {}", message);
        std::process::exit(1);
    });
}

fn run(args: Args) -> Result<(), String> {
    if !(0.0..=1.0).contains(&args.threshold) {
        return Err("--threshold must be in [0.0, 1.0]".to_string());
    }
    if !(0.0..=1.0).contains(&args.near_miss_threshold) {
        return Err("--near-miss-threshold must be in [0.0, 1.0]".to_string());
    }
    if args.near_miss_threshold > args.threshold {
        return Err("--near-miss-threshold must be <= --threshold".to_string());
    }
    if args.k == 0 {
        return Err("--k must be greater than zero".to_string());
    }
    if args.min_nodes == 0 {
        return Err("--min-nodes must be greater than zero".to_string());
    }

    let cwd = std::env::current_dir().map_err(|err| err.to_string())?;
    let scan_root = cwd.join(&args.root);
    if !scan_root.exists() {
        return Err(format!(
            "scan root does not exist: {}",
            normalize_path(args.root.as_path())
        ));
    }

    let files = collect_rust_files(scan_root.as_path(), args.include_tests)?;
    let mut functions = Vec::new();
    let mut parse_errors = Vec::new();
    for file in files {
        match parse_file_functions(&cwd, file.as_path(), args.k, args.include_tests) {
            Ok(mut parsed) => functions.append(&mut parsed),
            Err(err) => parse_errors.push(err),
        }
    }

    functions.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then(left.line.cmp(&right.line))
            .then(left.symbol.cmp(&right.symbol))
    });

    for (idx, info) in functions.iter_mut().enumerate() {
        info.id = idx;
    }

    let (duplicates, near_misses) = find_similarity_pairs(&functions, &args);
    let resolved_calls = resolve_callers(&functions);
    let dead = find_likely_dead_symbols(&functions, &resolved_calls, &args);
    let visibility_tighten = find_visibility_tighten_candidates(&functions, &resolved_calls);

    println!(
        "scanned {} Rust files under {}",
        count_unique_files(&functions),
        normalize_path(args.root.as_path())
    );
    println!("parsed {} function-like bodies", functions.len());

    if parse_errors.is_empty() {
        println!("parse errors: none");
    } else {
        println!("parse errors ({}):", parse_errors.len());
        for err in parse_errors.iter().take(20) {
            println!("  {}", err);
        }
        if parse_errors.len() > 20 {
            println!("  ... and {} more", parse_errors.len() - 20);
        }
    }

    print_duplicate_findings(&functions, &duplicates, &args);
    print_near_miss_findings(&functions, &near_misses, &args);
    print_dead_findings(&functions, &dead, &args);
    print_visibility_candidates(&functions, &visibility_tighten);

    let has_findings = !duplicates.is_empty() || !dead.is_empty() || !visibility_tighten.is_empty();
    if args.fail_on_findings && has_findings {
        return Err("findings detected".to_string());
    }

    Ok(())
}

fn collect_rust_files(root: &Path, include_tests: bool) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let walker = WalkDir::new(root).into_iter().filter_entry(|entry| {
        if !entry.file_type().is_dir() {
            return true;
        }
        let name = entry.file_name();
        if name == OsStr::new("target") || name == OsStr::new(".git") {
            return false;
        }
        if !include_tests && name == OsStr::new("tests") {
            return false;
        }
        true
    });

    for entry in walker {
        let entry = entry.map_err(|err| err.to_string())?;
        let path = entry.path();
        if entry.file_type().is_file()
            && path.extension().and_then(OsStr::to_str) == Some("rs")
            && (include_tests || !path_has_tests_segment(path))
        {
            out.push(path.to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

fn parse_file_functions(
    cwd: &Path,
    file: &Path,
    k: usize,
    include_tests: bool,
) -> Result<Vec<FunctionInfo>, String> {
    let src =
        fs::read_to_string(file).map_err(|err| format!("{}: {}", normalize_path(file), err))?;
    let ast = syn::parse_file(src.as_str())
        .map_err(|err| format!("{}: failed to parse: {}", normalize_path(file), err))?;

    let rel = file.strip_prefix(cwd).unwrap_or(file).to_path_buf();
    let base_module = infer_base_module(rel.as_path());
    let mut collector = AstCollector::new(rel, base_module, k, include_tests);
    collector.visit_file(&ast);
    Ok(collector.functions)
}

fn find_likely_dead_symbols(
    functions: &[FunctionInfo],
    resolved: &[ResolvedCall],
    args: &Args,
) -> Vec<DeadFinding> {
    let mut incoming_total = vec![0usize; functions.len()];
    let mut unresolved_name_hits: HashMap<String, usize> = HashMap::new();
    let mut outgoing_targets: Vec<Vec<usize>> = vec![Vec::new(); functions.len()];

    for call in resolved {
        if let Some(target_id) = call.target_id {
            if target_id == call.caller_id {
                continue;
            }
            incoming_total[target_id] += 1;
            outgoing_targets[call.caller_id].push(target_id);
        } else if let Some(name) = call.last_segment.as_ref() {
            *unresolved_name_hits.entry(name.clone()).or_insert(0) += 1;
        }
    }

    let mut eligible_ids = vec![false; functions.len()];
    for info in functions {
        if eligible_for_dead_check(info, args)
            && !unresolved_name_hits.contains_key(info.simple_name.as_str())
        {
            eligible_ids[info.id] = true;
        }
    }

    let mut live_incoming = incoming_total;
    let mut queue = VecDeque::new();
    for info in functions {
        if eligible_ids[info.id] && live_incoming[info.id] == 0 {
            queue.push_back(info.id);
        }
    }

    let mut dead_ids = HashSet::new();
    while let Some(dead_id) = queue.pop_front() {
        if !eligible_ids[dead_id] || !dead_ids.insert(dead_id) {
            continue;
        }
        for callee_id in &outgoing_targets[dead_id] {
            if !eligible_ids[*callee_id] || live_incoming[*callee_id] == 0 {
                continue;
            }
            live_incoming[*callee_id] -= 1;
            if live_incoming[*callee_id] == 0 {
                queue.push_back(*callee_id);
            }
        }
    }

    let mut dead: Vec<DeadFinding> = dead_ids
        .into_iter()
        .map(|id| DeadFinding {
            id,
            refs_total: 0,
            refs_outside_module: 0,
        })
        .collect();
    dead.sort_by(|left, right| {
        functions[left.id]
            .file
            .cmp(&functions[right.id].file)
            .then(functions[left.id].line.cmp(&functions[right.id].line))
    });
    dead.truncate(args.max_dead);
    dead
}

fn find_visibility_tighten_candidates(
    functions: &[FunctionInfo],
    resolved: &[ResolvedCall],
) -> Vec<DeadFinding> {
    let mut incoming_total = vec![0usize; functions.len()];
    let mut incoming_outside_module = vec![0usize; functions.len()];

    for call in resolved {
        if let Some(target_id) = call.target_id {
            if target_id == call.caller_id {
                continue;
            }
            incoming_total[target_id] += 1;
            if call.caller_module != functions[target_id].module_path {
                incoming_outside_module[target_id] += 1;
            }
        }
    }

    let mut out = Vec::new();
    for info in functions {
        if is_test_or_cfg_symbol(info) {
            continue;
        }
        if info.is_method || info.is_trait_impl_method || info.allow_dead_code {
            continue;
        }
        if !matches!(info.vis, VisibilityKind::Crate | VisibilityKind::Restricted) {
            continue;
        }
        if incoming_total[info.id] == 0 {
            continue;
        }
        if incoming_outside_module[info.id] == 0 {
            out.push(DeadFinding {
                id: info.id,
                refs_total: incoming_total[info.id],
                refs_outside_module: 0,
            });
        }
    }

    out.sort_by(|left, right| {
        functions[left.id]
            .file
            .cmp(&functions[right.id].file)
            .then(functions[left.id].line.cmp(&functions[right.id].line))
    });
    out.truncate(40);
    out
}

fn eligible_for_dead_check(info: &FunctionInfo, args: &Args) -> bool {
    if is_test_or_cfg_symbol(info) {
        return false;
    }
    if info.is_method || info.is_trait_impl_method {
        return false;
    }
    if info.allow_dead_code {
        return false;
    }
    if !args.include_pub && matches!(info.vis, VisibilityKind::Public) {
        return false;
    }
    if info.simple_name == "main" {
        return false;
    }
    true
}

fn is_test_or_cfg_symbol(info: &FunctionInfo) -> bool {
    info.is_test
        || info.has_cfg
        || info.module_path.iter().any(|segment| segment == "tests")
        || path_has_tests_segment(info.file.as_path())
}

#[derive(Debug)]
struct ResolvedCall {
    caller_id: usize,
    caller_module: Vec<String>,
    target_id: Option<usize>,
    last_segment: Option<String>,
}

fn resolve_callers(functions: &[FunctionInfo]) -> Vec<ResolvedCall> {
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for info in functions {
        by_name
            .entry(info.simple_name.as_str())
            .or_default()
            .push(info.id);
    }

    let mut out = Vec::new();
    for info in functions {
        for call in &info.call_refs {
            if call.is_method {
                continue;
            }
            let last = call.segments.last().cloned();
            let Some(last_name) = last.clone() else {
                continue;
            };
            let Some(candidates) = by_name.get(last_name.as_str()) else {
                out.push(ResolvedCall {
                    caller_id: info.id,
                    caller_module: info.module_path.clone(),
                    target_id: None,
                    last_segment: Some(last_name),
                });
                continue;
            };

            let resolved = if call.segments.len() == 1 {
                resolve_unqualified_call(info, candidates, functions)
            } else {
                resolve_qualified_call(call.segments.as_slice(), candidates, functions)
            };

            out.push(ResolvedCall {
                caller_id: info.id,
                caller_module: info.module_path.clone(),
                target_id: resolved,
                last_segment: if resolved.is_some() {
                    None
                } else {
                    Some(last_name)
                },
            });
        }
    }
    out
}

fn resolve_unqualified_call(
    caller: &FunctionInfo,
    candidates: &[usize],
    functions: &[FunctionInfo],
) -> Option<usize> {
    let same_module: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|id| functions[*id].module_path == caller.module_path)
        .collect();
    if same_module.len() == 1 {
        return Some(same_module[0]);
    }
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }
    None
}

fn resolve_qualified_call(
    call_segments: &[String],
    candidates: &[usize],
    functions: &[FunctionInfo],
) -> Option<usize> {
    let normalized = normalize_call_segments(call_segments);
    if normalized.is_empty() {
        return None;
    }
    let target_name = normalized.last()?;

    let mut matches = Vec::new();
    for id in candidates {
        let info = &functions[*id];
        if info.simple_name != *target_name {
            continue;
        }
        let mut candidate = info.module_path.clone();
        candidate.push(info.simple_name.clone());
        if ends_with_segments(candidate.as_slice(), normalized.as_slice()) {
            matches.push(*id);
        }
    }

    if matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

fn normalize_call_segments(segments: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for seg in segments {
        if seg == "crate" || seg == "self" || seg == "super" {
            continue;
        }
        out.push(seg.clone());
    }
    out
}

fn ends_with_segments(candidate: &[String], suffix: &[String]) -> bool {
    if suffix.len() > candidate.len() {
        return false;
    }
    candidate[candidate.len() - suffix.len()..] == *suffix
}

fn print_dead_findings(functions: &[FunctionInfo], dead: &[DeadFinding], args: &Args) {
    print_dead_report(
        functions,
        dead,
        format!(
            "likely-dead internal symbols (heuristic, top {}, pub included: {}):",
            dead.len(),
            args.include_pub
        )
        .as_str(),
        "likely-dead internal symbols: none",
        |_finding| String::new(),
    );
}

fn print_visibility_candidates(functions: &[FunctionInfo], findings: &[DeadFinding]) {
    print_dead_report(
        functions,
        findings,
        "visibility-tighten candidates (used only in defining module):",
        "visibility-tighten candidates (pub(crate)/restricted): none",
        |finding| {
            format!(
                " refs_total={} refs_outside_module={}",
                finding.refs_total, finding.refs_outside_module
            )
        },
    );
}

fn print_dead_report<F>(
    functions: &[FunctionInfo],
    findings: &[DeadFinding],
    header: &str,
    empty_line: &str,
    mut extra: F,
) where
    F: FnMut(&DeadFinding) -> String,
{
    if findings.is_empty() {
        println!("{}", empty_line);
        return;
    }

    println!("{}", header);
    for finding in findings {
        let info = &functions[finding.id];
        println!(
            "  {}:{} `{}` vis={:?}{}",
            normalize_path(info.file.as_path()),
            info.line,
            info.symbol,
            info.vis,
            extra(finding)
        );
    }
}

fn count_unique_files(functions: &[FunctionInfo]) -> usize {
    let mut seen: HashSet<&PathBuf> = HashSet::new();
    for f in functions {
        seen.insert(&f.file);
    }
    seen.len()
}

struct AstCollector {
    functions: Vec<FunctionInfo>,
    module_path: Vec<String>,
    file: PathBuf,
    include_tests: bool,
    k: usize,
}

impl AstCollector {
    fn new(file: PathBuf, base_module: Vec<String>, k: usize, include_tests: bool) -> Self {
        Self {
            functions: Vec::new(),
            module_path: base_module,
            file,
            include_tests,
            k,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_function(
        &mut self,
        name: String,
        owner_type: Option<String>,
        vis: VisibilityKind,
        is_method: bool,
        is_trait_impl_method: bool,
        attrs: &[Attribute],
        body: &syn::Block,
        span: proc_macro2::Span,
    ) {
        let has_cfg = has_cfg_attr(attrs);
        let is_test = has_test_attr(attrs);
        if !self.include_tests && (has_cfg || is_test) {
            return;
        }

        let mut normalizer = AstNormalizer::default();
        normalizer.visit_block(body);
        let normalized_nodes = normalizer.nodes;
        let shingles = build_shingles(normalized_nodes.as_slice(), self.k);
        let calls = collect_call_refs(body);

        let mut symbol = self.module_path.join("::");
        if !symbol.is_empty() {
            symbol.push_str("::");
        }
        if let Some(owner) = owner_type.as_ref() {
            symbol.push_str(owner);
            symbol.push_str("::");
        }
        symbol.push_str(name.as_str());

        self.functions.push(FunctionInfo {
            id: 0,
            symbol,
            module_path: self.module_path.clone(),
            simple_name: name,
            file: self.file.clone(),
            line: span.start().line,
            vis,
            is_method,
            is_trait_impl_method,
            has_cfg,
            is_test,
            allow_dead_code: allows_dead_code(attrs),
            normalized_nodes,
            shingles,
            call_refs: calls,
        });
    }
}

impl<'ast> Visit<'ast> for AstCollector {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if let Some((_, items)) = node.content.as_ref() {
            self.module_path.push(node.ident.to_string());
            for item in items {
                self.visit_item(item);
            }
            self.module_path.pop();
        }
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let vis = classify_visibility(&node.vis);
        self.push_function(
            node.sig.ident.to_string(),
            None,
            vis,
            false,
            false,
            node.attrs.as_slice(),
            node.block.as_ref(),
            node.sig.ident.span(),
        );
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let owner = normalize_type_name(node.self_ty.as_ref());
        let trait_impl = node.trait_.is_some();
        for item in &node.items {
            if let ImplItem::Fn(method) = item {
                let vis = classify_visibility(&method.vis);
                self.push_function(
                    method.sig.ident.to_string(),
                    Some(owner.clone()),
                    vis,
                    true,
                    trait_impl,
                    method.attrs.as_slice(),
                    &method.block,
                    method.sig.ident.span(),
                );
            }
        }
    }

    fn visit_file(&mut self, node: &'ast File) {
        for item in &node.items {
            self.visit_item(item);
        }
    }
}

fn build_shingles(tokens: &[String], k: usize) -> HashSet<u64> {
    if tokens.len() < k {
        return HashSet::new();
    }

    let mut out = HashSet::new();
    for window in tokens.windows(k) {
        let mut hasher = DefaultHasher::new();
        for token in window {
            token.hash(&mut hasher);
        }
        out.insert(hasher.finish());
    }
    out
}

fn jaccard(left: &HashSet<u64>, right: &HashSet<u64>) -> (f64, usize, usize) {
    if left.is_empty() && right.is_empty() {
        return (1.0, 0, 0);
    }

    let (small, large) = if left.len() < right.len() {
        (left, right)
    } else {
        (right, left)
    };
    let mut overlap = 0usize;
    for value in small {
        if large.contains(value) {
            overlap += 1;
        }
    }
    let union = left.len() + right.len() - overlap;
    if union == 0 {
        (0.0, overlap, union)
    } else {
        (overlap as f64 / union as f64, overlap, union)
    }
}

fn infer_base_module(file: &Path) -> Vec<String> {
    let components: Vec<String> = file
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect();

    let mut crate_name = String::new();
    let mut crate_idx = None;
    for (idx, part) in components.iter().enumerate() {
        if part == "crates" && idx + 1 < components.len() {
            crate_name = components[idx + 1].clone();
            crate_idx = Some(idx + 1);
            break;
        }
    }

    let mut module = Vec::new();
    if !crate_name.is_empty() {
        module.push(crate_name);
    }

    let Some(crate_idx) = crate_idx else {
        return module;
    };

    let src_idx = components
        .iter()
        .enumerate()
        .skip(crate_idx)
        .find_map(|(idx, part)| if part == "src" { Some(idx) } else { None });

    let tail_start = src_idx.map_or(crate_idx + 1, |idx| idx + 1);
    if tail_start >= components.len() {
        return module;
    }

    let file_name = components.last().cloned().unwrap_or_default();
    let tail = &components[tail_start..components.len() - 1];
    for part in tail {
        module.push(part.clone());
    }

    if file_name == "mod.rs" || file_name == "lib.rs" || file_name == "main.rs" {
        return module;
    }

    if let Some(stem) = Path::new(file_name.as_str())
        .file_stem()
        .and_then(OsStr::to_str)
        .map(str::to_string)
    {
        module.push(stem);
    }

    module
}

fn normalize_type_name(ty: &syn::Type) -> String {
    let raw = ty.to_token_stream().to_string();
    raw.replace(' ', "")
}

fn classify_visibility(vis: &Visibility) -> VisibilityKind {
    match vis {
        Visibility::Public(_) => VisibilityKind::Public,
        Visibility::Inherited => VisibilityKind::Private,
        Visibility::Restricted(restricted) => {
            if restricted.in_token.is_none() && restricted.path.is_ident("crate") {
                VisibilityKind::Crate
            } else {
                VisibilityKind::Restricted
            }
        }
    }
}

fn has_test_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path()
            .segments
            .last()
            .map(|segment| segment.ident == "test")
            .unwrap_or(false)
    })
}

fn attr_meta_contains(attr: &Attribute, attr_name: &str, needle: &str) -> bool {
    attr.path().is_ident(attr_name) && attr.meta.to_token_stream().to_string().contains(needle)
}

fn has_attr_meta_contains(attrs: &[Attribute], attr_name: &str, needle: &str) -> bool {
    attrs
        .iter()
        .any(|attr| attr_meta_contains(attr, attr_name, needle))
}

fn has_cfg_attr(attrs: &[Attribute]) -> bool {
    has_attr_meta_contains(attrs, "cfg", "test")
}

fn allows_dead_code(attrs: &[Attribute]) -> bool {
    has_attr_meta_contains(attrs, "allow", "dead_code")
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_has_tests_segment(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "tests")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
