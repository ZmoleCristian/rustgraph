//! Helpers for extracting and normalising function metadata from `syn` AST nodes.

use syn::{Meta, Signature, Token, Visibility, punctuated::Punctuated};

pub(super) struct FnMetadata {
    pub name: String,
    pub signature: String,
    pub is_async: bool,
    pub is_unsafe: bool,
    pub is_const: bool,
    pub generics: Vec<String>,
    pub parameters: Vec<String>,
    pub return_type: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
}

/// Normalise whitespace in a token-stream-rendered signature string.
///
/// Removes spurious spaces inserted by `quote!` around angle brackets, arrows, commas,
/// and reference `&` tokens so signatures are consistent across different Rust editions
/// and `quote` versions.
pub(crate) fn normalize_signature(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;

        if c == ' '
            && i + 2 < bytes.len()
            && bytes[i + 1] == b'<'
            && (bytes[i + 2] == b' ' || bytes[i + 2] == b'\'')
        {
            let prev = out.chars().last().unwrap_or(' ');
            if prev.is_alphanumeric() || prev == '>' || prev == '_' || prev == ')' {
                out.push('<');
                i += if bytes[i + 2] == b' ' { 3 } else { 2 };
                continue;
            }
        }

        if c == ' ' && i + 1 < bytes.len() && bytes[i + 1] == b'>' {
            let prev = out.chars().last().unwrap_or(' ');

            if prev != '-' && prev != '=' && prev != '<' {
                out.push('>');
                i += 2;

                if i < bytes.len() && bytes[i] == b' ' {
                    let after = if i + 1 < bytes.len() {
                        bytes[i + 1] as char
                    } else {
                        ' '
                    };
                    if after == ',' || after == ')' || after == ';' || after == ' ' || after == '>'
                    {
                        i += 1;
                    }
                }
                continue;
            }
        }

        if c == ' ' && i + 2 < bytes.len() && bytes[i + 1] == b',' && bytes[i + 2] == b' ' {
            out.push_str(", ");
            i += 3;
            continue;
        }

        if c == '&' && i + 1 < bytes.len() && bytes[i + 1] == b' ' && i + 2 < bytes.len() {
            let after = bytes[i + 2] as char;
            if after.is_alphanumeric() || after == '\'' || after == '_' || after == 'm' {
                out.push('&');
                i += 2;
                continue;
            }
        }

        if c == '-' && i + 2 < bytes.len() && bytes[i + 1] == b' ' && bytes[i + 2] == b'>' {
            out.push_str("->");
            i += 3;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

impl FnMetadata {
    pub(super) fn from_sig(sig: &Signature, span: proc_macro2::Span) -> Self {
        Self {
            name: sig.ident.to_string(),
            signature: normalize_signature(&format!("{}", quote::quote! { #sig })),
            is_async: sig.asyncness.is_some(),
            is_unsafe: sig.unsafety.is_some(),
            is_const: sig.constness.is_some(),
            generics: sig
                .generics
                .params
                .iter()
                .map(|param| normalize_signature(&format!("{}", quote::quote! { #param })))
                .collect(),
            parameters: sig
                .inputs
                .iter()
                .map(|input| normalize_signature(&format!("{}", quote::quote! { #input })))
                .collect(),
            return_type: match &sig.output {
                syn::ReturnType::Default => None,
                syn::ReturnType::Type(_, ty) => {
                    Some(normalize_signature(&format!("{}", quote::quote! { #ty })))
                }
            },
            start_line: span.start().line,
            end_line: span.end().line,
        }
    }
}

/// Build the stable function ID string `"file_path:start_line:name"`.
pub(crate) fn make_function_id(file_path: &str, start_line: usize, name: &str) -> String {
    format!("{}:{}:{}", file_path, start_line, name)
}

pub(super) fn is_public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// Return `true` if any attribute in `attrs` is a `#[cfg(...)]` whose predicate
/// compiles the item only under `test` (see [`cfg_predicate_enables_test`]).
pub(crate) fn attrs_imply_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.meta.path().is_ident("cfg")
            && attr
                .parse_args::<Meta>()
                .is_ok_and(|meta| cfg_predicate_enables_test(&meta))
    })
}

/// Return `true` if the cfg predicate compiles the item only under `test`:
/// the bare `test` path, or `test` nested inside `all(...)` / `any(...)`.
/// `not(test)` and strings that merely contain "test" (e.g.
/// `feature = "test-utils"`) do not count.
fn cfg_predicate_enables_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .is_ok_and(|nested| nested.iter().any(cfg_predicate_enables_test)),
        _ => false,
    }
}

/// Collect and normalise all `#[cfg(...)]` predicates from `attrs` into a string vector.
pub(crate) fn render_cfg_attrs(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut out = Vec::new();
    for attr in attrs {
        let path = attr.meta.path();
        if !path.is_ident("cfg") {
            continue;
        }
        if let syn::Meta::List(list) = &attr.meta {
            let raw = list.tokens.to_string();
            out.push(normalize_cfg_predicate(&raw));
        }
    }
    out
}

fn normalize_cfg_predicate(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch == ' ' {
            if prev_space {
                continue;
            }
            prev_space = true;
            out.push(ch);
        } else {
            prev_space = false;
            out.push(ch);
        }
    }
    out.replace(" (", "(")
        .replace("( ", "(")
        .replace(" )", ")")
        .replace(" ,", ",")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn make_function_id_uses_path_line_name() {
        assert_eq!(
            make_function_id("src/lib.rs", 12, "foo"),
            "src/lib.rs:12:foo"
        );
    }

    #[test]
    fn is_public_returns_true_for_pub_vis() {
        let vis: Visibility = parse_quote! { pub };
        assert!(is_public(&vis));
    }

    #[test]
    fn is_public_returns_false_for_inherited_vis() {
        let vis: Visibility = Visibility::Inherited;
        assert!(!is_public(&vis));
    }

    #[test]
    fn is_public_returns_false_for_pub_in_crate() {
        let vis: Visibility = parse_quote! { pub(crate) };
        assert!(!is_public(&vis));
    }

    #[test]
    fn fn_metadata_extracts_basic_signature_fields() {
        use syn::ItemFn;
        let item: ItemFn = parse_quote! {
            pub async unsafe fn demo<T>(x: u32, y: &str) -> Result<T, ()> { unimplemented!() }
        };
        let meta = FnMetadata::from_sig(&item.sig, syn::spanned::Spanned::span(&item));
        assert_eq!(meta.name, "demo");
        assert!(meta.is_async);
        assert!(meta.is_unsafe);
        assert!(!meta.is_const);
        assert_eq!(meta.parameters.len(), 2);
        assert_eq!(meta.generics.len(), 1);
        assert!(meta.return_type.is_some());
    }

    #[test]
    fn fn_metadata_handles_no_return_type() {
        use syn::ItemFn;
        let item: ItemFn = parse_quote! { fn noop() {} };
        let meta = FnMetadata::from_sig(&item.sig, syn::spanned::Spanned::span(&item));
        assert_eq!(meta.return_type, None);
        assert!(!meta.is_async);
        assert!(!meta.is_unsafe);
    }
}
