//! Utilities for extracting the callee target from `syn` call expressions.

use quote::ToTokens;
use syn::{Expr, ExprCall};

/// The resolved target of a function call expression.
#[derive(Debug, Clone)]
pub(crate) struct CallTarget {
    /// Full qualified name (e.g. `std::mem::take`) or a synthetic marker for complex calls.
    pub full: String,
    /// Bare name — last path segment or the synthetic marker (e.g. `take`, `complex_call`).
    pub base: String,
}

pub(super) fn extract_macro_path(macro_call: &syn::Macro) -> (String, String) {
    let full = macro_call
        .path
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    let full = format!("{}!", full);
    let base = macro_call
        .path
        .segments
        .last()
        .map(|seg| format!("{}!", seg.ident))
        .unwrap_or_else(|| full.clone());
    (full, base)
}

/// Extract a [`CallTarget`] from a `syn` function-call expression.
///
/// Returns `None` only when the function expression is structurally unrecognised
/// (currently never happens — the fallback branch always produces a synthetic target).
pub(crate) fn extract_call_target_from_call(call: &ExprCall) -> Option<CallTarget> {
    match &*call.func {
        Expr::Path(path) => {
            let full = path
                .path
                .segments
                .iter()
                .map(|seg| seg.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            let base = path
                .path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
                .unwrap_or_else(|| full.clone());
            Some(CallTarget { full, base })
        }
        Expr::Field(field) => {
            let member = field.member.to_token_stream().to_string();
            Some(CallTarget {
                full: format!("field::{}", member),
                base: member,
            })
        }
        _ => {
            let full = format!("complex_call::{}", quote::quote! { #call.func });
            Some(CallTarget {
                full,
                base: "complex_call".to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn extract_macro_path_returns_full_and_base() {
        let m: syn::Macro = parse_quote! { tokio::join!(a, b) };
        let (full, base) = extract_macro_path(&m);
        assert_eq!(full, "tokio::join!");
        assert_eq!(base, "join!");
    }

    #[test]
    fn extract_macro_path_handles_single_segment() {
        let m: syn::Macro = parse_quote! { vec!(1, 2, 3) };
        let (full, base) = extract_macro_path(&m);
        assert_eq!(full, "vec!");
        assert_eq!(base, "vec!");
    }

    #[test]
    fn extract_call_target_from_path_call_uses_last_segment_as_base() {
        let call: ExprCall = parse_quote! { foo() };
        let t = extract_call_target_from_call(&call).expect("target");
        assert_eq!(t.full, "foo");
        assert_eq!(t.base, "foo");
    }

    #[test]
    fn extract_call_target_from_qualified_path_keeps_full_path() {
        let call: ExprCall = parse_quote! { std::mem::take(x) };
        let t = extract_call_target_from_call(&call).expect("target");
        assert_eq!(t.full, "std::mem::take");
        assert_eq!(t.base, "take");
    }

    #[test]
    fn extract_call_target_from_field_call_uses_field_marker() {


        let field: syn::ExprField = parse_quote! { data.handler };
        let call = ExprCall {
            attrs: vec![],
            func: Box::new(Expr::Field(field)),
            paren_token: Default::default(),
            args: syn::punctuated::Punctuated::new(),
        };
        let t = extract_call_target_from_call(&call).expect("target");
        assert!(t.full.starts_with("field::"));
        assert_eq!(t.base, "handler");
    }

    #[test]
    fn extract_call_target_from_complex_call_uses_complex_marker() {


        let call: ExprCall = parse_quote! { (some.field)() };
        let t = extract_call_target_from_call(&call).expect("target");
        assert_eq!(t.base, "complex_call");
        assert!(t.full.starts_with("complex_call::"));
    }
}
