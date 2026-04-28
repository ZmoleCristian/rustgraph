//! Collects function names that are publicly re-exported from `lib.rs` or `mod.rs` files.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn collect_use_names(tree: &syn::UseTree, out: &mut HashSet<String>) {
    match tree {
        syn::UseTree::Name(name) => {
            out.insert(name.ident.to_string());
        }
        syn::UseTree::Rename(rename) => {
            out.insert(rename.rename.to_string());
        }
        syn::UseTree::Path(path) => {
            collect_use_names(&path.tree, out);
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_use_names(item, out);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

/// Collect all names exported via `pub use` statements in `lib.rs` and `mod.rs` files.
///
/// Only files whose name is exactly `lib.rs` or `mod.rs` are scanned.  For each
/// such file, the function parses the AST and records every identifier introduced
/// into the public namespace by a `pub use` item, including rename targets.
/// Glob re-exports (`pub use foo::*`) are skipped.
pub(crate) fn collect_public_reexport_names(rust_files: &[PathBuf]) -> HashSet<String> {
    let mut names = HashSet::new();
    for file_path in rust_files {
        let Some(fname) = file_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if fname != "mod.rs" && fname != "lib.rs" {
            continue;
        }
        let Ok(content) = fs::read_to_string(file_path) else {
            continue;
        };
        let Ok(syntax) = syn::parse_file(&content) else {
            continue;
        };
        for item in syntax.items {
            if let syn::Item::Use(item_use) = item
                && matches!(item_use.vis, syn::Visibility::Public(_))
            {
                collect_use_names(&item_use.tree, &mut names);
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("mkdirs");
        }
        std::fs::write(&p, content).expect("write");
        p
    }

    #[test]
    fn collect_use_names_handles_simple_name() {
        let tree: syn::UseTree = syn::parse_str("foo").unwrap();
        let mut out = HashSet::new();
        collect_use_names(&tree, &mut out);
        assert!(out.contains("foo"));
    }

    #[test]
    fn collect_use_names_handles_rename() {
        let tree: syn::UseTree = syn::parse_str("foo as bar").unwrap();
        let mut out = HashSet::new();
        collect_use_names(&tree, &mut out);
        assert!(out.contains("bar"));
        assert!(!out.contains("foo"));
    }

    #[test]
    fn collect_use_names_handles_path_and_group() {
        let tree: syn::UseTree = syn::parse_str("crate::foo::{a, b}").unwrap();
        let mut out = HashSet::new();
        collect_use_names(&tree, &mut out);
        assert!(out.contains("a"));
        assert!(out.contains("b"));
    }

    #[test]
    fn collect_use_names_skips_glob() {
        let tree: syn::UseTree = syn::parse_str("crate::foo::*").unwrap();
        let mut out = HashSet::new();
        collect_use_names(&tree, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn collect_public_reexport_names_extracts_names_from_lib_rs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lib = write(
            dir.path(),
            "src/lib.rs",
            "pub use foo::{bar, baz as qux};\nuse other::ignored;\n",
        );
        let names = collect_public_reexport_names(&[lib]);
        assert!(names.contains("bar"));
        assert!(names.contains("qux"));
        assert!(!names.contains("ignored"));
        assert!(!names.contains("baz"));
    }

    #[test]
    fn collect_public_reexport_names_extracts_names_from_mod_rs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let modrs = write(
            dir.path(),
            "src/api/mod.rs",
            "pub use crate::api::types::Foo;\n",
        );
        let names = collect_public_reexport_names(&[modrs]);
        assert!(names.contains("Foo"));
    }

    #[test]
    fn collect_public_reexport_names_skips_random_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let other = write(dir.path(), "src/other.rs", "pub use crate::Foo;\n");
        let names = collect_public_reexport_names(&[other]);
        assert!(names.is_empty());
    }
}
