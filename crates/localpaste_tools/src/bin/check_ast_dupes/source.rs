//! Source scanning with inherited test context and attribute references.

use super::normalize::{collect_call_refs, AstNormalizer};
use super::*;
use syn::visit::Visit;
use syn::{Field, File, ImplItem, ItemEnum, ItemFn, ItemImpl, ItemMod, ItemStruct, Meta, Variant};

/// Collected audit inputs, including references from unexpanded attributes.
pub(super) struct SourceScan {
    pub(super) functions: Vec<FunctionInfo>,
    pub(super) parse_errors: Vec<String>,
    pub(super) attribute_refs: HashSet<String>,
}

struct SourceFile {
    path: PathBuf,
    ast: Result<File, String>,
}

/// Parses each source once and propagates test context through module declarations.
///
/// # Arguments
/// - `cwd`: Workspace directory used for relative report paths.
/// - `files`: Rust source paths, including test modules needed for context discovery.
/// - `k`: Shingle width for normalized function bodies.
/// - `include_tests`: Whether test-only bodies participate in similarity checks.
///
/// # Returns
/// Function metadata, parse diagnostics, and conservative attribute references.
pub(super) fn scan_sources(
    cwd: &Path,
    files: Vec<PathBuf>,
    k: usize,
    include_tests: bool,
) -> SourceScan {
    let sources: Vec<SourceFile> = files
        .into_iter()
        .map(|file| {
            let path = file.canonicalize().unwrap_or(file);
            let ast = fs::read_to_string(&path)
                .map_err(|err| format!("{}: {}", normalize_path(&path), err))
                .and_then(|src| {
                    syn::parse_file(&src).map_err(|err| {
                        format!("{}: failed to parse: {}", normalize_path(&path), err)
                    })
                });
            SourceFile { path, ast }
        })
        .collect();
    let test_only = test_only_files(&sources);
    let mut scan = SourceScan {
        functions: Vec::new(),
        parse_errors: Vec::new(),
        attribute_refs: HashSet::new(),
    };
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    for (source, test_only) in sources.into_iter().zip(test_only) {
        if test_only && !include_tests {
            continue;
        }
        let ast = match source.ast {
            Ok(ast) => ast,
            Err(err) => {
                scan.parse_errors.push(err);
                continue;
            }
        };
        let rel = source
            .path
            .strip_prefix(&cwd)
            .unwrap_or(&source.path)
            .to_path_buf();
        let base_module = infer_base_module(&rel);
        let mut collector = AstCollector::new(rel, base_module, k, include_tests, test_only);
        collector.visit_file(&ast);
        scan.functions.extend(collector.functions);
        scan.attribute_refs.extend(collector.attribute_refs);
    }
    scan
}

/// A file shared by test and production declarations stays in the production audit.
fn test_only_files(sources: &[SourceFile]) -> Vec<bool> {
    let by_path: HashMap<&Path, usize> = sources
        .iter()
        .enumerate()
        .map(|(id, source)| (source.path.as_path(), id))
        .collect();
    let mut referenced = HashSet::new();
    let mut contexts = vec![[false; 2]; sources.len()];
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();

    // Crate roots establish module-resolution context. Remaining files are
    // roots too, but are queued after declared modules so path-loaded children
    // do not become spurious production roots.
    for (id, source) in sources.iter().enumerate() {
        if matches!(
            source.path.file_name().and_then(|name| name.to_str()),
            Some("lib.rs" | "main.rs" | "mod.rs")
        ) {
            queue.push_back((id, false, ModuleLinks::new(&source.path).module_dir));
        }
    }
    loop {
        if let Some((id, inherited_test, module_dir)) = queue.pop_front() {
            if !seen.insert((id, inherited_test, module_dir.clone())) {
                continue;
            }
            visit_module_context(
                id,
                inherited_test,
                module_dir,
                sources,
                &by_path,
                &mut referenced,
                &mut contexts,
                &mut queue,
            );
            continue;
        }
        if let Some((id, source)) = sources
            .iter()
            .enumerate()
            .find(|(id, _)| !referenced.contains(id) && contexts[*id] == [false; 2])
        {
            queue.push_back((id, false, ModuleLinks::new(&source.path).module_dir));
        } else {
            break;
        }
    }
    contexts
        .into_iter()
        .map(|[production, test]| test && !production)
        .collect()
}

// Keep traversal state explicit so module links are resolved in their declaration context.
#[allow(clippy::too_many_arguments)]
fn visit_module_context(
    id: usize,
    inherited_test: bool,
    module_dir: PathBuf,
    sources: &[SourceFile],
    by_path: &HashMap<&Path, usize>,
    referenced: &mut HashSet<usize>,
    contexts: &mut [[bool; 2]],
    queue: &mut VecDeque<(usize, bool, PathBuf)>,
) {
    let source = &sources[id];
    let test_only = inherited_test
        || path_has_tests_segment(&source.path)
        || source
            .ast
            .as_ref()
            .is_ok_and(|ast| has_cfg_attr(&ast.attrs));
    contexts[id][usize::from(test_only)] = true;
    let Ok(ast) = &source.ast else { return };
    let mut collector = ModuleLinks::with_module_dir(&source.path, module_dir);
    collector.visit_file(ast);
    for link in collector.links {
        if let Some(&target) = by_path.get(link.path.as_path()) {
            referenced.insert(target);
            queue.push_back((target, test_only || link.test_only, link.module_dir));
        }
    }
}

struct ModuleLink {
    path: PathBuf,
    test_only: bool,
    module_dir: PathBuf,
}

struct ModuleLinks {
    module_dir: PathBuf,
    path_attr_dir: PathBuf,
    test_only: bool,
    links: Vec<ModuleLink>,
}

impl ModuleLinks {
    fn new(file: &Path) -> Self {
        let parent = file.parent().unwrap_or(Path::new(""));
        let stem = file.file_stem().unwrap_or_default();
        let module_dir = if matches!(stem.to_str(), Some("lib" | "main" | "mod")) {
            parent.to_path_buf()
        } else {
            parent.join(stem)
        };
        Self::with_module_dir(file, module_dir)
    }

    fn with_module_dir(file: &Path, module_dir: PathBuf) -> Self {
        Self {
            module_dir,
            path_attr_dir: file.parent().unwrap_or(Path::new("")).to_path_buf(),
            test_only: false,
            links: Vec::new(),
        }
    }
}

impl<'ast> Visit<'ast> for ModuleLinks {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let test_only = self.test_only || has_cfg_attr(&node.attrs);
        if let Some((_, items)) = &node.content {
            let mut nested = Self {
                module_dir: self.module_dir.join(node.ident.to_string()),
                path_attr_dir: self.module_dir.join(node.ident.to_string()),
                test_only,
                links: Vec::new(),
            };
            for item in items {
                nested.visit_item(item);
            }
            self.links.extend(nested.links);
        } else {
            let explicit_path = node.attrs.iter().find_map(|attr| {
                if !attr.path().is_ident("path") {
                    return None;
                }
                let syn::Meta::NameValue(meta) = &attr.meta else {
                    return None;
                };
                let syn::Expr::Lit(expr) = &meta.value else {
                    return None;
                };
                let syn::Lit::Str(path) = &expr.lit else {
                    return None;
                };
                Some(self.path_attr_dir.join(path.value()))
            });
            let candidates = explicit_path.map_or_else(
                || {
                    vec![
                        (
                            self.module_dir.join(format!("{}.rs", node.ident)),
                            self.module_dir.join(node.ident.to_string()),
                        ),
                        (
                            self.module_dir.join(node.ident.to_string()).join("mod.rs"),
                            self.module_dir.join(node.ident.to_string()),
                        ),
                    ]
                },
                |path| {
                    let module_dir = path.parent().unwrap_or(&self.path_attr_dir).to_path_buf();
                    vec![(path, module_dir)]
                },
            );
            for (path, module_dir) in candidates {
                if let Ok(path) = path.canonicalize() {
                    self.links.push(ModuleLink {
                        path,
                        test_only,
                        module_dir,
                    });
                }
            }
        }
    }
}

struct AstCollector {
    functions: Vec<FunctionInfo>,
    module_path: Vec<String>,
    file: PathBuf,
    include_tests: bool,
    k: usize,
    test_context: bool,
    attribute_refs: HashSet<String>,
}

impl AstCollector {
    fn new(
        file: PathBuf,
        base_module: Vec<String>,
        k: usize,
        include_tests: bool,
        test_context: bool,
    ) -> Self {
        Self {
            functions: Vec::new(),
            module_path: base_module,
            file,
            include_tests,
            k,
            test_context,
            attribute_refs: HashSet::new(),
        }
    }

    fn enter_test_context(&mut self, attrs: &[Attribute]) -> bool {
        let previous = self.test_context;
        self.test_context |= has_cfg_attr(attrs);
        previous
    }

    fn visit_in_test_context<'node, T>(
        &mut self,
        attrs: &[Attribute],
        node: &'node T,
        visit: impl FnOnce(&mut Self, &'node T),
    ) {
        let previous = self.enter_test_context(attrs);
        if !self.test_context || self.include_tests {
            visit(self, node);
        }
        self.test_context = previous;
    }

    // Keep the syntax-specific call sites explicit about method/trait metadata.
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
        let has_cfg = self.test_context || has_cfg_attr(attrs);
        let is_test = has_test_attr(attrs);
        if !self.include_tests && (has_cfg || is_test) {
            return;
        }
        for attr in attrs {
            self.visit_attribute(attr);
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
        let previous = self.enter_test_context(&node.attrs);
        if self.test_context && !self.include_tests {
            self.test_context = previous;
            return;
        }
        if let Some((_, items)) = node.content.as_ref() {
            self.module_path.push(node.ident.to_string());
            for item in items {
                self.visit_item(item);
            }
            self.module_path.pop();
        }
        self.test_context = previous;
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let previous = self.enter_test_context(&node.attrs);
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
        self.test_context = previous;
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let previous = self.enter_test_context(&node.attrs);
        if self.test_context && !self.include_tests {
            self.test_context = previous;
            return;
        }
        let owner = normalize_type_name(node.self_ty.as_ref());
        let trait_impl = node.trait_.is_some();
        for item in &node.items {
            if let ImplItem::Fn(method) = item {
                let previous = self.enter_test_context(&method.attrs);
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
                self.test_context = previous;
            }
        }
        self.test_context = previous;
    }

    fn visit_file(&mut self, node: &'ast File) {
        let previous = self.enter_test_context(&node.attrs);
        if self.test_context && !self.include_tests {
            self.test_context = previous;
            return;
        }
        for item in &node.items {
            self.visit_item(item);
        }
        self.test_context = previous;
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        self.visit_in_test_context(&node.attrs, node, syn::visit::visit_item_struct);
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        self.visit_in_test_context(&node.attrs, node, syn::visit::visit_item_enum);
    }

    fn visit_field(&mut self, node: &'ast Field) {
        self.visit_in_test_context(&node.attrs, node, syn::visit::visit_field);
    }

    fn visit_variant(&mut self, node: &'ast Variant) {
        self.visit_in_test_context(&node.attrs, node, syn::visit::visit_variant);
    }

    fn visit_attribute(&mut self, node: &'ast Attribute) {
        if self.test_context {
            return;
        }
        let Meta::List(meta) = &node.meta else {
            return;
        };
        let Ok(items) = meta
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            return;
        };
        for item in items {
            collect_attribute_callback_refs(&item, &mut self.attribute_refs);
        }
    }
}

fn collect_attribute_callback_refs(meta: &Meta, out: &mut HashSet<String>) {
    match meta {
        Meta::NameValue(value) if is_callback_key(&value.path) => {
            collect_callback_expr(&value.value, out);
        }
        Meta::List(list) => {
            let Ok(items) = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return;
            };
            let callback_items = items
                .into_iter()
                .skip(usize::from(list.path.is_ident("cfg_attr")));
            for item in callback_items {
                collect_attribute_callback_refs(&item, out);
            }
        }
        _ => {}
    }
}

fn is_callback_key(path: &syn::Path) -> bool {
    matches!(
        path.segments
            .last()
            .map(|segment| segment.ident.to_string())
            .as_deref(),
        Some(
            "value_parser"
                | "default"
                | "deserialize_with"
                | "serialize_with"
                | "skip_serializing_if"
        )
    )
}

fn collect_callback_expr(expr: &syn::Expr, out: &mut HashSet<String>) {
    let path = match expr {
        syn::Expr::Path(path) => Some(path.path.clone()),
        syn::Expr::Lit(expr) => match &expr.lit {
            syn::Lit::Str(value) => syn::parse_str::<syn::Path>(&value.value()).ok(),
            _ => None,
        },
        _ => None,
    };
    if let Some(path) = path {
        if let Some(segment) = path.segments.last() {
            out.insert(segment.ident.to_string());
        }
    }
}
