//! Utilities for identifying runtime entrypoints and external call patterns.
//!
//! Used during filtering to prevent false dead-code positives for functions that
//! are invoked by frameworks, proc-macro infrastructure, or FFI rather than
//! through regular Rust call sites.

use crate::{CallSite, infer_file_crate_key, infer_file_module_segments};
use quote::ToTokens;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use syn::spanned::Spanned;

/// Return `true` if `attr_text` matches a known runtime-entrypoint attribute.
///
/// Recognises `#[no_mangle]`, `#[export_name]`, async-runtime mains,
/// Actix/Rocket route macros, Anchor `#[program]`, wasm-bindgen, napi,
/// pyo3 `#[pyfunction]`, and proc-macro declaration attributes.
/// Comparison is case-insensitive and whitespace-insensitive.
pub fn is_runtime_entrypoint_attr_text(attr_text: &str) -> bool {
    let compact: String = attr_text
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    [
        "no_mangle",
        "export_name",
        "panic_handler",
        "global_allocator",
        "alloc_error_handler",
        "wasm_bindgen",
        "tauri::command",
        "napi",
        "pyfunction",
        "tokio::main",
        "async_std::main",
        "actix_web::main",
        "actix_web::get",
        "actix_web::post",
        "actix_web::put",
        "actix_web::delete",
        "actix_web::patch",
        "actix_web::head",
        "actix_web::options",
        "actix_web::route",
        "rocket::get",
        "rocket::post",
        "rocket::put",
        "rocket::delete",
        "rocket::patch",
        "rocket::head",
        "rocket::options",
        "rocket::route",
        "proc_macro",
        "proc_macro_derive",
        "proc_macro_attribute",
        "proc_macro_hack",
    ]
    .iter()
    .any(|marker| compact.contains(marker))
}

/// Return `true` if `attr` marks an Anchor `#[program]` module.
///
/// Matches both the bare `#[program]` form and the fully-qualified
/// `#[anchor_lang::program]` path.
pub fn is_runtime_program_module_attr(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("program") {
        return true;
    }
    let compact: String = attr
        .to_token_stream()
        .to_string()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    compact.contains("anchor_lang::program")
}

/// Return `true` if `callee` looks like an associated-function call on a type.
///
/// Heuristic: the path segment immediately before the final `::` segment must
/// start with an uppercase letter (e.g. `Item::from`, `crate::Foo::new`).
/// Requires at least two `::` segments; bare names return `false`.
pub fn is_likely_associated_function_callee(callee: &str) -> bool {
    let segments = callee
        .split("::")
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 2 {
        return false;
    }
    let owner = segments[segments.len() - 2].trim();
    owner
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

/// Return `true` if `call_site` is a qualified call into an external crate.
///
/// A call is considered external when its first `::` segment is not `crate`,
/// `self`, or `super`, and is not among the known top-level module names of
/// the caller's crate as recorded in `local_module_roots`.
pub fn is_likely_external_qualified_call(
    call_site: &CallSite,
    local_module_roots: &HashMap<String, HashSet<String>>,
) -> bool {
    let segments = call_site
        .callee
        .split("::")
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 2 {
        return false;
    }
    let first = segments[0].trim();
    if matches!(first, "crate" | "self" | "super") {
        return false;
    }

    let caller_crate = infer_file_crate_key(&call_site.file_path);
    if caller_crate.is_empty() {
        return false;
    }
    let Some(roots) = local_module_roots.get(&caller_crate) else {
        return false;
    };
    !roots.contains(first)
}

/// Collect the 1-based start lines of all runtime-entrypoint functions in `file_path`.
///
/// A function is considered an entrypoint if it carries a recognised entrypoint
/// attribute or if it is defined inside a module annotated with `#[program]` /
/// `#[anchor_lang::program]`.  Returns an empty set when the file cannot be read
/// or parsed.
pub fn collect_runtime_entrypoint_lines(file_path: &str) -> HashSet<usize> {
    fn visit_items(items: &[syn::Item], in_program_module: bool, out: &mut HashSet<usize>) {
        for item in items {
            match item {
                syn::Item::Fn(item_fn) => {
                    if in_program_module
                        || item_fn.attrs.iter().any(|a| {
                            is_runtime_entrypoint_attr_text(&a.to_token_stream().to_string())
                        })
                    {
                        out.insert(item_fn.span().start().line);
                    }
                }
                syn::Item::Mod(item_mod) => {
                    let module_is_program = in_program_module
                        || item_mod.attrs.iter().any(is_runtime_program_module_attr);
                    if let Some((_, inner_items)) = &item_mod.content {
                        visit_items(inner_items, module_is_program, out);
                    }
                }
                _ => {}
            }
        }
    }

    let Ok(content) = fs::read_to_string(file_path) else {
        return HashSet::new();
    };
    let Ok(syntax) = syn::parse_file(&content) else {
        return HashSet::new();
    };

    let mut out = HashSet::new();
    visit_items(&syntax.items, false, &mut out);
    out
}

/// Build a map from crate key to the set of top-level module names in that crate.
///
/// Iterates `rust_files`, infers each file's crate key and module segments, and
/// accumulates the segment names.  Used to distinguish local qualified calls from
/// external dependency calls.
pub fn collect_local_module_roots(rust_files: &[PathBuf]) -> HashMap<String, HashSet<String>> {
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for file_path in rust_files {
        let file = file_path.to_string_lossy().to_string();
        let crate_key = infer_file_crate_key(&file);
        if crate_key.is_empty() {
            continue;
        }
        let segments = infer_file_module_segments(&file);
        if !segments.is_empty() {
            let names = out.entry(crate_key).or_default();
            for seg in segments {
                names.insert(seg);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(callee: &str, file_path: &str) -> CallSite {
        CallSite {
            caller_id: None,
            caller_name: None,
            callee: callee.to_string(),
            callee_base: callee.split("::").last().unwrap_or(callee).to_string(),
            call_kind: "function".to_string(),
            file_path: file_path.to_string(),
            line: 1,
            column: 0,
        }
    }

    #[test]
    fn is_runtime_entrypoint_attr_text_recognises_no_mangle_and_export_name() {
        assert!(is_runtime_entrypoint_attr_text("#[no_mangle]"));
        assert!(is_runtime_entrypoint_attr_text("#[export_name = \"foo\"]"));
    }

    #[test]
    fn is_runtime_entrypoint_attr_text_recognises_proc_macro_attrs() {
        assert!(is_runtime_entrypoint_attr_text("#[proc_macro]"));
        assert!(is_runtime_entrypoint_attr_text("#[proc_macro_derive(D)]"));
        assert!(is_runtime_entrypoint_attr_text("#[proc_macro_attribute]"));
    }

    #[test]
    fn is_runtime_entrypoint_attr_text_recognises_async_main_attrs() {
        assert!(is_runtime_entrypoint_attr_text("#[tokio::main]"));
        assert!(is_runtime_entrypoint_attr_text("#[async_std::main]"));
        assert!(is_runtime_entrypoint_attr_text("#[actix_web::main]"));
    }

    #[test]
    fn is_runtime_entrypoint_attr_text_returns_false_for_normal_attrs() {
        assert!(!is_runtime_entrypoint_attr_text("#[derive(Clone)]"));
        assert!(!is_runtime_entrypoint_attr_text("#[serde(skip)]"));
    }

    #[test]
    fn is_runtime_program_module_attr_recognises_program() {
        let attr: syn::Attribute = syn::parse_quote! { #[program] };
        assert!(is_runtime_program_module_attr(&attr));
    }

    #[test]
    fn is_runtime_program_module_attr_recognises_anchor_program_path() {
        let attr: syn::Attribute = syn::parse_quote! { #[anchor_lang::program] };
        assert!(is_runtime_program_module_attr(&attr));
    }

    #[test]
    fn is_runtime_program_module_attr_returns_false_for_other_attrs() {
        let attr: syn::Attribute = syn::parse_quote! { #[derive(Debug)] };
        assert!(!is_runtime_program_module_attr(&attr));
    }

    #[test]
    fn is_likely_associated_function_callee_detects_uppercase_owner() {
        assert!(is_likely_associated_function_callee("Item::from"));
        assert!(is_likely_associated_function_callee("crate::Foo::new"));
    }

    #[test]
    fn is_likely_associated_function_callee_returns_false_for_lowercase_module() {
        assert!(!is_likely_associated_function_callee("module::function"));
    }

    #[test]
    fn is_likely_associated_function_callee_returns_false_for_unqualified() {
        assert!(!is_likely_associated_function_callee("plain_call"));
    }

    #[test]
    fn is_likely_external_qualified_call_returns_false_for_crate_relative() {
        let c = cs("crate::foo::bar", "/proj/mycrate/src/lib.rs");
        let roots: HashMap<String, HashSet<String>> = HashMap::new();
        assert!(!is_likely_external_qualified_call(&c, &roots));
    }

    #[test]
    fn is_likely_external_qualified_call_returns_false_for_unqualified() {
        let c = cs("foo", "/proj/mycrate/src/lib.rs");
        let roots: HashMap<String, HashSet<String>> = HashMap::new();
        assert!(!is_likely_external_qualified_call(&c, &roots));
    }

    #[test]
    fn is_likely_external_qualified_call_uses_module_root_lookup() {
        let mut roots: HashMap<String, HashSet<String>> = HashMap::new();
        let mut set = HashSet::new();
        set.insert("api".to_string());
        roots.insert("mycrate".to_string(), set);

        let local = cs("api::foo", "/proj/mycrate/src/lib.rs");
        assert!(!is_likely_external_qualified_call(&local, &roots));

        let external = cs("serde_json::from_str", "/proj/mycrate/src/lib.rs");
        assert!(is_likely_external_qualified_call(&external, &roots));
    }

    #[test]
    fn collect_local_module_roots_aggregates_segments_per_crate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lib = dir.path().join("mycrate/src/lib.rs");
        let api = dir.path().join("mycrate/src/api/users.rs");
        std::fs::create_dir_all(api.parent().unwrap()).expect("mkdirs");
        std::fs::write(&lib, "").expect("w");
        std::fs::write(&api, "").expect("w");

        let roots = collect_local_module_roots(&[lib, api]);
        let names = roots.get("mycrate").expect("crate key");
        assert!(names.contains("api"));
        assert!(names.contains("users"));
    }

    #[test]
    fn collect_runtime_entrypoint_lines_finds_no_mangle_function() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        std::fs::write(
            &path,
            "fn live() {}\n#[no_mangle]\npub extern \"C\" fn ffi() {}\n",
        )
        .expect("write");

        let lines = collect_runtime_entrypoint_lines(path.to_str().unwrap());
        assert!(!lines.is_empty());
    }

    #[test]
    fn collect_runtime_entrypoint_lines_finds_program_module_functions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("b.rs");
        std::fs::write(
            &path,
            "#[program]\npub mod prog {\n    pub fn supply() {}\n    pub fn unstake() {}\n}\n",
        )
        .expect("write");

        let lines = collect_runtime_entrypoint_lines(path.to_str().unwrap());
        assert!(!lines.is_empty());
    }

    #[test]
    fn collect_runtime_entrypoint_lines_returns_empty_for_normal_code() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("c.rs");
        std::fs::write(&path, "fn live() {}\n").expect("write");
        let lines = collect_runtime_entrypoint_lines(path.to_str().unwrap());
        assert!(lines.is_empty());
    }

    #[test]
    fn collect_runtime_entrypoint_lines_returns_empty_for_unparseable() {
        let lines = collect_runtime_entrypoint_lines("/nonexistent/foo.rs");
        assert!(lines.is_empty());
    }
}
