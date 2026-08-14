//! Module-path inference from file system paths and inline `mod` blocks.

use super::super::{FunctionInfo, function_id};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use syn::spanned::Spanned;

/// Infer the Rust module path segments for a file from its path.
///
/// Strips well-known prefixes (`src/`, `tests/`, `benches/`, `examples/`) and
/// converts directory components plus the file stem into module segments.
/// `lib.rs`, `main.rs`, and `mod.rs` contribute only their parent directories.
pub(crate) fn infer_file_module_segments(file_path: &str) -> Vec<String> {
    let normalized = file_path.replace('\\', "/");
    let relative = if let Some(i) = normalized.find("/src/") {
        &normalized[(i + 5)..]
    } else if let Some(i) = normalized.find("/tests/") {
        &normalized[(i + 7)..]
    } else if let Some(i) = normalized.find("/benches/") {
        &normalized[(i + 9)..]
    } else if let Some(i) = normalized.find("/examples/") {
        &normalized[(i + 10)..]
    } else if let Some(stripped) = normalized.strip_prefix("./src/") {
        stripped
    } else if let Some(stripped) = normalized.strip_prefix("./tests/") {
        stripped
    } else if let Some(stripped) = normalized.strip_prefix("./benches/") {
        stripped
    } else if let Some(stripped) = normalized.strip_prefix("./examples/") {
        stripped
    } else if let Some(stripped) = normalized.strip_prefix("src/") {
        stripped
    } else if let Some(stripped) = normalized.strip_prefix("tests/") {
        stripped
    } else if let Some(stripped) = normalized.strip_prefix("benches/") {
        stripped
    } else if let Some(stripped) = normalized.strip_prefix("examples/") {
        stripped
    } else {
        normalized.as_str()
    };

    let path = Path::new(relative);
    let mut dirs: Vec<String> = path
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter_map(|component| component.as_os_str().to_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return dirs;
    };

    match stem {
        "lib" | "main" | "mod" => dirs,
        other => {
            dirs.push(other.to_string());
            dirs
        }
    }
}

/// Infer the crate name for a file by looking for standard layout markers.
///
/// Returns the directory name immediately before `/src/`, `/tests/`, `/benches/`,
/// or `/examples/`.  Falls back to the immediate parent directory name.
pub(crate) fn infer_file_crate_key(file_path: &str) -> String {
    let normalized = file_path.replace('\\', "/");
    for marker in ["/src/", "/tests/", "/benches/", "/examples/"] {
        if let Some(i) = normalized.find(marker) {
            let prefix = &normalized[..i];
            if let Some(segment) = prefix.rsplit('/').find(|s| !s.is_empty()) {
                return segment.to_string();
            }
        }
    }
    Path::new(&normalized)
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn collect_inline_function_module_paths(file_path: &str) -> HashMap<usize, Vec<String>> {
    fn visit_items(
        items: &[syn::Item],
        stack: &mut Vec<String>,
        out: &mut HashMap<usize, Vec<String>>,
    ) {
        for item in items {
            match item {
                syn::Item::Fn(item_fn) => {
                    out.insert(item_fn.span().start().line, stack.clone());
                }
                syn::Item::Impl(item_impl) => {
                    for impl_item in &item_impl.items {
                        if let syn::ImplItem::Fn(item_fn) = impl_item {
                            out.insert(item_fn.span().start().line, stack.clone());
                        }
                    }
                }
                syn::Item::Trait(item_trait) => {
                    for trait_item in &item_trait.items {
                        if let syn::TraitItem::Fn(item_fn) = trait_item {
                            out.insert(item_fn.span().start().line, stack.clone());
                        }
                    }
                }
                syn::Item::Mod(item_mod) => {
                    if let Some((_, inner_items)) = &item_mod.content {
                        stack.push(item_mod.ident.to_string());
                        visit_items(inner_items, stack, out);
                        stack.pop();
                    }
                }
                _ => {}
            }
        }
    }

    let Ok(content) = fs::read_to_string(crate::index::resolve_read_path(file_path)) else {
        return HashMap::new();
    };
    let Ok(syntax) = syn::parse_file(&content) else {
        return HashMap::new();
    };

    let mut out = HashMap::new();
    let mut stack = Vec::new();
    visit_items(&syntax.items, &mut stack, &mut out);
    out
}

/// Build a map from each function's stable ID to its full module path segment list.
///
/// Combines file-path derived segments with inline module nesting detected by a
/// lightweight re-parse of each source file.
pub(crate) fn build_function_module_lookup(
    functions: &[FunctionInfo],
) -> HashMap<String, Vec<String>> {
    let mut per_file_inline: HashMap<String, HashMap<usize, Vec<String>>> = HashMap::new();
    let mut out = HashMap::new();

    for function in functions {
        let id = function_id(function);
        let inline = per_file_inline
            .entry(function.file_path.clone())
            .or_insert_with(|| collect_inline_function_module_paths(&function.file_path));
        let inline_stack = inline
            .get(&function.start_line)
            .cloned()
            .unwrap_or_default();
        let mut module_path = infer_file_module_segments(&function.file_path);
        module_path.extend(inline_stack);
        out.insert(id, module_path);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_file_module_segments_returns_empty_for_lib_rs() {
        let segments = infer_file_module_segments("/work/proj/src/lib.rs");
        assert!(segments.is_empty());
    }

    #[test]
    fn infer_file_module_segments_returns_empty_for_main_rs() {
        let segments = infer_file_module_segments("/proj/src/main.rs");
        assert!(segments.is_empty());
    }

    #[test]
    fn infer_file_module_segments_uses_file_stem() {
        let segments = infer_file_module_segments("/proj/src/foo.rs");
        assert_eq!(segments, vec!["foo".to_string()]);
    }

    #[test]
    fn infer_file_module_segments_includes_directory() {
        let segments = infer_file_module_segments("/proj/src/api/users.rs");
        assert_eq!(segments, vec!["api".to_string(), "users".to_string()]);
    }

    #[test]
    fn infer_file_module_segments_strips_mod_rs() {
        let segments = infer_file_module_segments("/proj/src/api/mod.rs");
        assert_eq!(segments, vec!["api".to_string()]);
    }

    #[test]
    fn infer_file_module_segments_handles_relative_src_prefix() {
        let segments = infer_file_module_segments("./src/foo.rs");
        assert_eq!(segments, vec!["foo".to_string()]);
        let segments_no_dot = infer_file_module_segments("src/bar.rs");
        assert_eq!(segments_no_dot, vec!["bar".to_string()]);
    }

    #[test]
    fn infer_file_module_segments_handles_tests_and_benches() {
        let s = infer_file_module_segments("/proj/tests/integration.rs");
        assert_eq!(s, vec!["integration".to_string()]);
        let b = infer_file_module_segments("/proj/benches/bench.rs");
        assert_eq!(b, vec!["bench".to_string()]);
        let e = infer_file_module_segments("/proj/examples/demo.rs");
        assert_eq!(e, vec!["demo".to_string()]);
    }

    #[test]
    fn infer_file_module_segments_handles_windows_separators() {
        let segments = infer_file_module_segments(r"C:\proj\src\foo.rs");
        assert_eq!(segments, vec!["foo".to_string()]);
    }

    #[test]
    fn infer_file_crate_key_extracts_name_for_src_layout() {
        let key = infer_file_crate_key("/work/proj/mycrate/src/lib.rs");
        assert_eq!(key, "mycrate");
    }

    #[test]
    fn infer_file_crate_key_handles_tests() {
        let key = infer_file_crate_key("/work/proj/mycrate/tests/integration.rs");
        assert_eq!(key, "mycrate");
    }

    #[test]
    fn infer_file_crate_key_returns_parent_when_no_marker() {
        let key = infer_file_crate_key("/some/dir/file.rs");
        assert_eq!(key, "dir");
    }

    #[test]
    fn build_function_module_lookup_assigns_module_paths() {
        let func = FunctionInfo {
            name: "foo".to_string(),
            signature: "fn foo".to_string(),
            file_path: "src/api/users.rs".to_string(),
            start_line: 1,
            end_line: 1,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: vec![],
            parameters: vec![],
            return_type: None,
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        };
        let lookup = build_function_module_lookup(&[func.clone()]);
        let id = function_id(&func);
        let path = lookup.get(&id).expect("path entry");
        assert_eq!(*path, vec!["api".to_string(), "users".to_string()]);
    }
}
