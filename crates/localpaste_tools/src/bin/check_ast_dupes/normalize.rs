//! AST node normalization and call-reference extraction helpers.

use super::CallRef;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::visit::Visit;
use syn::{Attribute, Expr, ExprCall, ExprMethodCall, Lit};

#[derive(Default)]
pub(super) struct AstNormalizer {
    pub(super) nodes: Vec<String>,
}

impl<'ast> Visit<'ast> for AstNormalizer {
    fn visit_attribute(&mut self, _i: &'ast Attribute) {
        // Attributes are intentionally ignored.
    }

    fn visit_expr(&mut self, node: &'ast Expr) {
        self.nodes.push(expr_kind(node).to_string());
        syn::visit::visit_expr(self, node);
    }

    fn visit_lit(&mut self, node: &'ast Lit) {
        self.nodes.push(lit_kind(node).to_string());
        syn::visit::visit_lit(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        self.nodes
            .push(format!("path({})", node.path.segments.len()));
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        self.nodes
            .push(format!("type({})", node.path.segments.len()));
        syn::visit::visit_type_path(self, node);
    }

    fn visit_pat_ident(&mut self, _node: &'ast syn::PatIdent) {
        self.nodes.push("pat_id".to_string());
    }

    fn visit_ident(&mut self, _node: &'ast syn::Ident) {
        self.nodes.push("id".to_string());
    }
}

fn expr_kind(expr: &Expr) -> &'static str {
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
