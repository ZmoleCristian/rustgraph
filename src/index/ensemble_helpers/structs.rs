use crate::index::FunctionInfo;
use std::collections::HashSet;
use std::fs;
use syn::spanned::Spanned;
use syn::visit::Visit;

struct UsedStructsVisitor<'a> {
    known_structs: &'a HashSet<String>,
    used: HashSet<String>,
}

impl<'ast> Visit<'ast> for UsedStructsVisitor<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        for seg in &path.segments {
            let name = seg.ident.to_string();
            if self.known_structs.contains(&name) {
                self.used.insert(name);
            }
        }
        syn::visit::visit_path(self, path);
    }
}

fn visit_items_for_function(
    items: &[syn::Item],
    func: &FunctionInfo,
    visitor: &mut UsedStructsVisitor<'_>,
    found: &mut bool,
) {
    for item in items {
        if *found {
            return;
        }

        match item {
            syn::Item::Fn(item_fn) => {
                if item_fn.sig.ident == func.name && item_fn.span().start().line == func.start_line
                {
                    visitor.visit_item_fn(item_fn);
                    *found = true;
                }
            }
            syn::Item::Impl(item_impl) => {
                for impl_item in &item_impl.items {
                    if let syn::ImplItem::Fn(item_fn) = impl_item
                        && item_fn.sig.ident == func.name
                        && item_fn.span().start().line == func.start_line
                    {
                        visitor.visit_impl_item_fn(item_fn);
                        *found = true;
                        break;
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for trait_item in &item_trait.items {
                    if let syn::TraitItem::Fn(item_fn) = trait_item
                        && item_fn.sig.ident == func.name
                        && item_fn.span().start().line == func.start_line
                    {
                        visitor.visit_trait_item_fn(item_fn);
                        *found = true;
                        break;
                    }
                }
            }
            syn::Item::Mod(item_mod) => {
                if let Some((_, inner_items)) = &item_mod.content {
                    visit_items_for_function(inner_items, func, visitor, found);
                }
            }
            _ => {}
        }
    }
}

/// Parse the source file containing `func` with `syn` and return the subset of
/// `known_structs` names that are referenced inside that function's AST node.
///
/// Returns an error if the file cannot be read or the source fails to parse.
/// Prefer [`collect_used_structs_with_project`] when a [`crate::project::ProjectData`]
/// is available — it reuses the cached `syn::File` and avoids a second parse.
pub fn collect_used_structs(
    func: &FunctionInfo,
    known_structs: &HashSet<String>,
) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(crate::index::resolve_read_path(&func.file_path))?;
    let syntax_tree = syn::parse_file(&content)?;
    Ok(collect_used_structs_in_ast(
        &syntax_tree,
        func,
        known_structs,
    ))
}

/// Cache-aware variant of [`collect_used_structs`]: looks the parsed
/// [`syn::File`] up on `project` and falls back to disk if it isn't cached.
pub fn collect_used_structs_with_project(
    project: &crate::project::ProjectData,
    func: &FunctionInfo,
    known_structs: &HashSet<String>,
) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    if let Some(syntax) = project.parsed_file_by_str(&func.file_path) {
        return Ok(collect_used_structs_in_ast(syntax, func, known_structs));
    }
    collect_used_structs(func, known_structs)
}

fn collect_used_structs_in_ast(
    syntax_tree: &syn::File,
    func: &FunctionInfo,
    known_structs: &HashSet<String>,
) -> HashSet<String> {
    let mut visitor = UsedStructsVisitor {
        known_structs,
        used: HashSet::new(),
    };

    let mut found = false;
    visit_items_for_function(&syntax_tree.items, func, &mut visitor, &mut found);

    visitor.used
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn temp_with(contents: &str) -> NamedTempFile {
        let mut tmp = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
        tmp.write_all(contents.as_bytes()).unwrap();
        tmp.flush().unwrap();
        tmp
    }

    fn function_at(file: &str, name: &str, start_line: usize) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}()"),
            file_path: file.to_string(),
            start_line,
            end_line: start_line + 1,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: Vec::new(),
            parameters: Vec::new(),
            return_type: None,
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    #[test]
    fn collect_used_structs_finds_referenced_struct_names() {
        let file = temp_with("pub fn run() {\n    let _ = User { name: \"x\".to_string() };\n}\n");
        let path = file.path().to_string_lossy().to_string();
        let mut known = HashSet::new();
        known.insert("User".to_string());

        let func = function_at(&path, "run", 1);
        let used = collect_used_structs(&func, &known).expect("ok");
        assert!(used.contains("User"));
    }

    #[test]
    fn collect_used_structs_returns_empty_when_function_unmatched() {
        let file = temp_with("pub fn other() {}\n");
        let path = file.path().to_string_lossy().to_string();
        let mut known = HashSet::new();
        known.insert("User".to_string());

        let func = function_at(&path, "missing", 99);
        let used = collect_used_structs(&func, &known).expect("ok");
        assert!(used.is_empty());
    }

    #[test]
    fn collect_used_structs_skips_unknown_struct_names() {
        let file = temp_with("pub fn run() {\n    let _ = SomeOtherType {};\n}\n");
        let path = file.path().to_string_lossy().to_string();
        let mut known = HashSet::new();
        known.insert("KnownType".to_string());

        let func = function_at(&path, "run", 1);
        let used = collect_used_structs(&func, &known).expect("ok");
        assert!(used.is_empty());
    }

    #[test]
    fn collect_used_structs_returns_err_for_missing_file() {
        let mut known = HashSet::new();
        known.insert("User".to_string());
        let func = function_at("/no/such/file.rs", "run", 1);
        assert!(collect_used_structs(&func, &known).is_err());
    }

    #[test]
    fn collect_used_structs_returns_err_for_invalid_syntax() {
        let file = temp_with("not valid rust ::");
        let path = file.path().to_string_lossy().to_string();
        let mut known = HashSet::new();
        let func = function_at(&path, "run", 1);
        assert!(collect_used_structs(&func, &known).is_err());
    }
}
