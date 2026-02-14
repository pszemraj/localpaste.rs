//! AST node normalization and call-reference extraction helpers.

use super::CallRef;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::HashMap;
use syn::visit::Visit;
use syn::{Attribute, Expr, ExprCall, ExprMethodCall, Lit};

#[derive(Default)]
pub(super) struct AstNormalizer {
    pub(super) nodes: Vec<String>,
    ident_map: HashMap<String, usize>,
}

impl<'ast> Visit<'ast> for AstNormalizer {
    fn visit_attribute(&mut self, _i: &'ast Attribute) {
        // Attributes are intentionally ignored.
    }

    fn visit_expr(&mut self, node: &'ast Expr) {
        self.nodes.push(expr_kind(node));
        syn::visit::visit_expr(self, node);
    }

    fn visit_lit(&mut self, node: &'ast Lit) {
        self.nodes.push(lit_kind(node).to_string());
        syn::visit::visit_lit(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        self.nodes.push(path_kind("path", &node.path));
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        self.nodes.push(path_kind("type", &node.path));
        syn::visit::visit_type_path(self, node);
    }

    fn visit_pat_ident(&mut self, node: &'ast syn::PatIdent) {
        let token = self.ident_token_for(&node.ident);
        self.nodes.push(format!("pat_{}", token));
        if let Some((_at, subpat)) = &node.subpat {
            self.visit_pat(subpat);
        }
    }

    fn visit_ident(&mut self, node: &'ast syn::Ident) {
        let token = self.ident_token_for(node);
        self.nodes.push(token);
    }
}

impl AstNormalizer {
    fn ident_token_for(&mut self, ident: &syn::Ident) -> String {
        let key = ident.to_string();
        let next = self.ident_map.len();
        let idx = *self.ident_map.entry(key).or_insert(next);
        format!("id_{}", idx)
    }
}

fn expr_kind(expr: &Expr) -> String {
    match expr {
        Expr::Binary(binary) => format!("expr_binary_{}", binary_op_kind(&binary.op)),
        Expr::Unary(unary) => format!("expr_unary_{}", unary_op_kind(&unary.op)),
        _ => expr_kind_base(expr).to_string(),
    }
}

fn expr_kind_base(expr: &Expr) -> &'static str {
    match expr {
        Expr::Array(_) => "expr_array",
        Expr::Assign(_) => "expr_assign",
        Expr::Async(_) => "expr_async",
        Expr::Await(_) => "expr_await",
        Expr::Binary(_) => "expr_binary",
        Expr::Block(_) => "expr_block",
        Expr::Break(_) => "expr_break",
        Expr::Call(_) => "expr_call",
        Expr::Cast(_) => "expr_cast",
        Expr::Closure(_) => "expr_closure",
        Expr::Const(_) => "expr_const",
        Expr::Continue(_) => "expr_continue",
        Expr::Field(_) => "expr_field",
        Expr::ForLoop(_) => "expr_for",
        Expr::Group(_) => "expr_group",
        Expr::If(_) => "expr_if",
        Expr::Index(_) => "expr_index",
        Expr::Infer(_) => "expr_infer",
        Expr::Let(_) => "expr_let",
        Expr::Lit(_) => "expr_lit",
        Expr::Loop(_) => "expr_loop",
        Expr::Macro(_) => "expr_macro",
        Expr::Match(_) => "expr_match",
        Expr::MethodCall(_) => "expr_method_call",
        Expr::Paren(_) => "expr_paren",
        Expr::Path(_) => "expr_path",
        Expr::Range(_) => "expr_range",
        Expr::Reference(_) => "expr_ref",
        Expr::Repeat(_) => "expr_repeat",
        Expr::Return(_) => "expr_return",
        Expr::Struct(_) => "expr_struct",
        Expr::Try(_) => "expr_try",
        Expr::TryBlock(_) => "expr_try_block",
        Expr::Tuple(_) => "expr_tuple",
        Expr::Unary(_) => "expr_unary",
        Expr::Unsafe(_) => "expr_unsafe",
        Expr::Verbatim(_) => "expr_verbatim",
        Expr::While(_) => "expr_while",
        Expr::Yield(_) => "expr_yield",
        _ => "expr_other",
    }
}

fn binary_op_kind(op: &syn::BinOp) -> &'static str {
    match op {
        syn::BinOp::Add(_) => "add",
        syn::BinOp::Sub(_) => "sub",
        syn::BinOp::Mul(_) => "mul",
        syn::BinOp::Div(_) => "div",
        syn::BinOp::Rem(_) => "rem",
        syn::BinOp::And(_) => "and",
        syn::BinOp::Or(_) => "or",
        syn::BinOp::BitXor(_) => "bit_xor",
        syn::BinOp::BitAnd(_) => "bit_and",
        syn::BinOp::BitOr(_) => "bit_or",
        syn::BinOp::Shl(_) => "shl",
        syn::BinOp::Shr(_) => "shr",
        syn::BinOp::Eq(_) => "eq",
        syn::BinOp::Lt(_) => "lt",
        syn::BinOp::Le(_) => "le",
        syn::BinOp::Ne(_) => "ne",
        syn::BinOp::Ge(_) => "ge",
        syn::BinOp::Gt(_) => "gt",
        syn::BinOp::AddAssign(_) => "add_assign",
        syn::BinOp::SubAssign(_) => "sub_assign",
        syn::BinOp::MulAssign(_) => "mul_assign",
        syn::BinOp::DivAssign(_) => "div_assign",
        syn::BinOp::RemAssign(_) => "rem_assign",
        syn::BinOp::BitXorAssign(_) => "bit_xor_assign",
        syn::BinOp::BitAndAssign(_) => "bit_and_assign",
        syn::BinOp::BitOrAssign(_) => "bit_or_assign",
        syn::BinOp::ShlAssign(_) => "shl_assign",
        syn::BinOp::ShrAssign(_) => "shr_assign",
        _ => "other",
    }
}

fn unary_op_kind(op: &syn::UnOp) -> &'static str {
    match op {
        syn::UnOp::Deref(_) => "deref",
        syn::UnOp::Not(_) => "not",
        syn::UnOp::Neg(_) => "neg",
        _ => "other",
    }
}

fn lit_kind(lit: &Lit) -> &'static str {
    match lit {
        Lit::Str(_) => "lit_str",
        Lit::ByteStr(_) => "lit_bytes",
        Lit::Byte(_) => "lit_byte",
        Lit::Char(_) => "lit_char",
        Lit::Int(_) => "lit_int",
        Lit::Float(_) => "lit_float",
        Lit::Bool(_) => "lit_bool",
        Lit::Verbatim(_) => "lit_verbatim",
        _ => "lit_other",
    }
}

fn path_kind(prefix: &str, path: &syn::Path) -> String {
    let depth = path.segments.len();
    if let Some(first) = path
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
    {
        if is_well_known_path_root(first.as_str()) {
            return format!("{}({}/{})", prefix, first, depth);
        }
    }
    format!("{}({})", prefix, depth)
}

fn is_well_known_path_root(root: &str) -> bool {
    matches!(
        root,
        "std"
            | "core"
            | "alloc"
            | "crate"
            | "self"
            | "super"
            | "Self"
            | "Option"
            | "Result"
            | "Some"
            | "None"
            | "Ok"
            | "Err"
    )
}

pub(super) fn collect_call_refs(block: &syn::Block) -> Vec<CallRef> {
    let mut collector = CallRefCollector::default();
    collector.visit_block(block);
    collector.calls
}

#[derive(Default)]
struct CallRefCollector {
    calls: Vec<CallRef>,
}

impl<'ast> Visit<'ast> for CallRefCollector {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Expr::Path(path_expr) = node.func.as_ref() {
            let segments: Vec<String> = path_expr
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            if !segments.is_empty() {
                self.calls.push(CallRef {
                    segments,
                    is_method: false,
                });
            }
        }
        for arg in &node.args {
            if let Expr::Path(path_expr) = arg {
                let segments: Vec<String> = path_expr
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect();
                if !segments.is_empty() {
                    self.calls.push(CallRef {
                        segments,
                        is_method: false,
                    });
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        self.calls.push(CallRef {
            segments: vec![node.method.to_string()],
            is_method: true,
        });
        for arg in &node.args {
            if let Expr::Path(path_expr) = arg {
                let segments: Vec<String> = path_expr
                    .path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect();
                if !segments.is_empty() {
                    self.calls.push(CallRef {
                        segments,
                        is_method: false,
                    });
                }
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        let mut names = Vec::new();
        collect_macro_call_like_idents(node.tokens.clone(), &mut names);
        for name in names {
            self.calls.push(CallRef {
                segments: vec![name],
                is_method: false,
            });
        }
        syn::visit::visit_macro(self, node);
    }
}

fn collect_macro_call_like_idents(tokens: TokenStream, out: &mut Vec<String>) {
    let mut iter = tokens.into_iter().peekable();
    while let Some(tree) = iter.next() {
        match tree {
            TokenTree::Ident(ident) => {
                if let Some(TokenTree::Group(group)) = iter.peek() {
                    if group.delimiter() == Delimiter::Parenthesis {
                        out.push(ident.to_string());
                    }
                }
            }
            TokenTree::Group(group) => {
                collect_macro_call_like_idents(group.stream(), out);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize_block(src: &str) -> Vec<String> {
        let block: syn::Block = syn::parse_str(src).expect("block should parse");
        let mut normalizer = AstNormalizer::default();
        normalizer.visit_block(&block);
        normalizer.nodes
    }

    #[test]
    fn distinguishes_binary_and_unary_ops() {
        let nodes = normalize_block("{ let _ = a + b; let _ = a && b; let _ = !a; }");
        assert!(nodes.contains(&"expr_binary_add".to_string()));
        assert!(nodes.contains(&"expr_binary_and".to_string()));
        assert!(nodes.contains(&"expr_unary_not".to_string()));
    }

    #[test]
    fn reuses_identifier_placeholders_within_function() {
        let nodes = normalize_block("{ let x = 1; let y = x + x; }");
        let x_refs = nodes
            .iter()
            .filter(|token| token.as_str() == "id_0")
            .count();
        let y_refs = nodes
            .iter()
            .filter(|token| token.as_str() == "id_1")
            .count();
        assert_eq!(x_refs, 2);
        assert_eq!(y_refs, 0);
        assert!(nodes.contains(&"pat_id_0".to_string()));
        assert!(nodes.contains(&"pat_id_1".to_string()));
    }

    #[test]
    fn preserves_well_known_path_roots() {
        let nodes = normalize_block("{ std::mem::swap(&mut a, &mut b); helper::call(); }");
        assert!(nodes.contains(&"path(std/3)".to_string()));
        assert!(nodes.contains(&"path(2)".to_string()));
    }
}
