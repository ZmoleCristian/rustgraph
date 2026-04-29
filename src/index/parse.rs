//! Per-file parse entrypoint and symbol-ID helpers.

use super::visitor::{CodeVisitor, make_function_id};
use super::{CallSite, EnumInfo, FunctionInfo, StructInfo};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use syn::visit::Visit;

pub use super::scan::*;

/// Return type of [`parse_rust_file`]: a tuple of all symbol collections extracted from one file.
///
/// Fields in order: functions, structs, enums, call map (caller-id → callee names),
/// call sites, type aliases, and re-exports.
pub type ParsedRustFile = (
    Vec<FunctionInfo>,
    Vec<StructInfo>,
    Vec<EnumInfo>,
    HashMap<String, Vec<String>>,
    Vec<CallSite>,
    Vec<(String, String)>,
    Vec<(String, String, String)>,
);

/// Parse a single `.rs` file and return all extracted symbols and call sites.
///
/// Files located under a `tests/` directory are treated as `#[cfg(test)]` by default.
pub fn parse_rust_file(file_path: &Path) -> Result<ParsedRustFile, Box<dyn std::error::Error>> {
    let (parsed, _ast) = parse_rust_file_with_ast(file_path)?;
    Ok(parsed)
}

/// Parse a single `.rs` file and return both the extracted symbols and the
/// parsed [`syn::File`] AST.
///
/// The caller can stash the returned [`syn::File`] in a cache (see
/// [`crate::project::ProjectData::parsed_file`]) and reuse it instead of
/// calling `syn::parse_file` again for the same source.
///
/// Files located under a `tests/` directory are treated as `#[cfg(test)]` by default.
pub fn parse_rust_file_with_ast(
    file_path: &Path,
) -> Result<(ParsedRustFile, syn::File), Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file_path)?;
    let syntax_tree = syn::parse_file(&content)?;

    let mut visitor = CodeVisitor::new(file_path.to_string_lossy().to_string());
    if path_is_under_tests_dir(file_path) {
        visitor.cfg_test_depth = 1;
    }
    visitor.visit_file(&syntax_tree);

    let parsed = (
        visitor.functions,
        visitor.structs,
        visitor.enums,
        visitor.function_calls,
        visitor.call_sites,
        visitor.aliases,
        visitor.reexports,
    );
    Ok((parsed, syntax_tree))
}

/// Compute the stable ID for a [`FunctionInfo`] in the form `"file:line:name"`.
pub fn function_id(func: &FunctionInfo) -> String {
    make_function_id(&func.file_path, func.start_line, &func.name)
}


/// Alias for [`function_id`]; prefer [`function_id`] in new code.
pub fn function_symbol_id(func: &FunctionInfo) -> String {
    function_id(func)
}

/// Compute the stable ID for a [`StructInfo`] in the form `"struct:file:line:name"`.
pub fn struct_symbol_id(s: &StructInfo) -> String {
    format!("struct:{}:{}:{}", s.file_path, s.start_line, s.name)
}

/// Compute the stable ID for an [`EnumInfo`] in the form `"enum:file:line:name"`.
pub fn enum_symbol_id(e: &EnumInfo) -> String {
    format!("enum:{}:{}:{}", e.file_path, e.start_line, e.name)
}

fn path_is_under_tests_dir(file_path: &Path) -> bool {
    file_path.components().any(|c| {
        matches!(c, std::path::Component::Normal(s) if s == "tests")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_id_combines_path_line_and_name() {
        let f = FunctionInfo {
            name: "demo".to_string(),
            signature: "fn demo".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 42,
            end_line: 44,
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
        assert_eq!(function_id(&f), "src/lib.rs:42:demo");
    }

    #[test]
    fn parse_rust_file_returns_functions_and_structs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        std::fs::write(
            &path,
            "pub struct S { pub a: u32 }\npub fn foo(x: u32) -> u32 { x }\n",
        )
        .expect("write");

        let (funcs, structs, enums, _calls, _sites, _aliases, _reexports) =
            parse_rust_file(&path).expect("parse ok");
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "foo");
        assert!(funcs[0].is_pub);
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name, "S");
        assert!(enums.is_empty());
    }

    #[test]
    fn parse_rust_file_records_function_calls() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("calls.rs");
        std::fs::write(
            &path,
            "fn helper() {}\nfn driver() { helper(); helper(); }\n",
        )
        .expect("write");

        let (funcs, _structs, _enums, _calls, sites, _aliases, _reexports) =
            parse_rust_file(&path).expect("parse ok");
        let driver = funcs.iter().find(|f| f.name == "driver").expect("driver fn");
        let driver_id = function_id(driver);
        let driver_calls = sites
            .iter()
            .filter(|s| s.caller_id.as_deref() == Some(&driver_id))
            .count();
        assert!(driver_calls >= 2);
    }

    #[test]
    fn parse_rust_file_returns_err_for_invalid_source() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.rs");
        std::fs::write(&path, "fn () { invalid syntax").expect("write");
        assert!(parse_rust_file(&path).is_err());
    }
}
