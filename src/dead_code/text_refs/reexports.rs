//! Collects function names that are publicly re-exported via `pub use`
//! anywhere in the crate.
//!
//! Every `.rs` file is a Rust module and `pub use` in any module file places
//! a name into that module's public namespace, so dead-code analysis must
//! consider all of them — not just `lib.rs` and `mod.rs`.  In Rust 2018+
//! the parent module file for a `foo/` directory is `foo.rs`, not
//! `foo/mod.rs`, and `pub use foo::Bar;` in that parent file was
//! previously silently dropped, producing false-positive dead-code reports
//! for re-exported public items.

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

/// Collect all names exported via `pub use` anywhere in the crate.
///
/// Every `.rs` file in `rust_files` is parsed and every `pub use` item is
/// recorded.  This covers both the classic `foo/mod.rs` parent-module form
/// and the Rust 2018+ `foo.rs` parent form.  Glob re-exports (`pub use
/// foo::*`) are skipped because the names they expose cannot be enumerated
/// without resolving the source module.
pub(crate) fn collect_public_reexport_names(rust_files: &[PathBuf]) -> HashSet<String> {
    let mut names = HashSet::new();
    for file_path in rust_files {
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
    fn collect_public_reexport_names_picks_up_pub_use_in_2018_parent_module_file() {
        // Rust 2018+ layout: `foo/`'s parent module file is `foo.rs`, not
        // `foo/mod.rs`. The previous filename filter only scanned `mod.rs`/
        // `lib.rs` so `pub use` here was silently dropped and the
        // re-exported name was reported as dead code.
        let dir = tempfile::tempdir().expect("tempdir");
        let parent = write(
            dir.path(),
            "src/foo.rs",
            "pub mod bar;\npub use crate::foo::bar::Quux;\n",
        );
        let names = collect_public_reexport_names(&[parent]);
        assert!(
            names.contains("Quux"),
            "expected `Quux` from a 2018+ parent-module file (foo.rs), got {names:?}"
        );
    }

    #[test]
    fn collect_public_reexport_names_handles_arbitrary_module_files() {
        // Previously these were skipped; now every .rs file is scanned so
        // `pub use` declared in regular module files is honoured.
        let dir = tempfile::tempdir().expect("tempdir");
        let other = write(dir.path(), "src/other.rs", "pub use crate::Foo;\n");
        let names = collect_public_reexport_names(&[other]);
        assert!(names.contains("Foo"));
    }

    #[test]
    fn collect_public_reexport_names_still_skips_glob_reexports() {
        let dir = tempfile::tempdir().expect("tempdir");
        let lib = write(dir.path(), "src/lib.rs", "pub use crate::foo::*;\n");
        let names = collect_public_reexport_names(&[lib]);
        // Glob re-exports cannot be enumerated statically.
        assert!(names.is_empty());
    }
}
