//! AST-based source-range collectors for `#[cfg(test)]` blocks and `macro_rules!` definitions.

use quote::ToTokens;
use std::fs;
use syn::{
    Attribute, ImplItemFn, ItemFn, ItemMacro, ItemMod, TraitItemFn,
    spanned::Spanned,
    visit::{self, Visit},
};

fn has_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .to_token_stream()
                .to_string()
                .to_lowercase()
                .contains("test")
    })
}

#[derive(Default)]
struct CfgTestRangeVisitor {
    ranges: Vec<(usize, usize)>,
}

impl CfgTestRangeVisitor {
    fn push_span<T: Spanned>(&mut self, node: &T) {
        let span = node.span();
        self.ranges.push((span.start().line, span.end().line));
    }
}

impl<'ast> Visit<'ast> for CfgTestRangeVisitor {
    fn visit_item_mod(&mut self, item_mod: &'ast ItemMod) {
        if has_cfg_test(&item_mod.attrs) {
            self.push_span(item_mod);
        }
        visit::visit_item_mod(self, item_mod);
    }

    fn visit_item_fn(&mut self, item_fn: &'ast ItemFn) {
        if has_cfg_test(&item_fn.attrs) {
            self.push_span(item_fn);
        }
        visit::visit_item_fn(self, item_fn);
    }

    fn visit_impl_item_fn(&mut self, item_fn: &'ast ImplItemFn) {
        if has_cfg_test(&item_fn.attrs) {
            self.push_span(item_fn);
        }
        visit::visit_impl_item_fn(self, item_fn);
    }

    fn visit_trait_item_fn(&mut self, item_fn: &'ast TraitItemFn) {
        if has_cfg_test(&item_fn.attrs) {
            self.push_span(item_fn);
        }
        visit::visit_trait_item_fn(self, item_fn);
    }
}

/// Return the line ranges `(start, end)` of all `#[cfg(test)]`-gated items in `file_path`.
///
/// Parses the file with `syn`; returns an empty `Vec` if the file cannot be read or parsed.
/// Prefer [`collect_cfg_test_ranges_from_ast`] when a parsed `syn::File` is
/// already available (e.g. via [`crate::project::ProjectData::parsed_file`])
/// to avoid a second parse of the same source file.
pub(crate) fn collect_cfg_test_ranges(file_path: &str) -> Vec<(usize, usize)> {
    let Ok(content) = fs::read_to_string(file_path) else {
        return Vec::new();
    };
    let Ok(syntax) = syn::parse_file(&content) else {
        return Vec::new();
    };
    collect_cfg_test_ranges_from_ast(&syntax)
}

/// AST-driven core of [`collect_cfg_test_ranges`] — operates on an
/// already-parsed [`syn::File`] from the project cache.
pub(crate) fn collect_cfg_test_ranges_from_ast(syntax: &syn::File) -> Vec<(usize, usize)> {
    let mut v = CfgTestRangeVisitor::default();
    v.visit_file(syntax);
    v.ranges
}

#[derive(Default)]
struct MacroRulesRangeVisitor {
    ranges: Vec<(usize, usize)>,
}

impl MacroRulesRangeVisitor {
    fn push_span<T: Spanned>(&mut self, node: &T) {
        let span = node.span();
        self.ranges.push((span.start().line, span.end().line));
    }
}

impl<'ast> Visit<'ast> for MacroRulesRangeVisitor {
    fn visit_item_macro(&mut self, item_macro: &'ast ItemMacro) {
        if item_macro.mac.path.is_ident("macro_rules") {
            self.push_span(item_macro);
        }
        visit::visit_item_macro(self, item_macro);
    }
}

/// Return the line ranges `(start, end)` of all `macro_rules!` definitions in `file_path`.
///
/// Parses the file with `syn`; returns an empty `Vec` if the file cannot be read or parsed.
/// Prefer [`collect_macro_rules_ranges_from_ast`] when a parsed `syn::File` is
/// already available.
pub(crate) fn collect_macro_rules_ranges(file_path: &str) -> Vec<(usize, usize)> {
    let Ok(content) = fs::read_to_string(file_path) else {
        return Vec::new();
    };
    let Ok(syntax) = syn::parse_file(&content) else {
        return Vec::new();
    };
    collect_macro_rules_ranges_from_ast(&syntax)
}

/// AST-driven core of [`collect_macro_rules_ranges`] — operates on an
/// already-parsed [`syn::File`] from the project cache.
pub(crate) fn collect_macro_rules_ranges_from_ast(syntax: &syn::File) -> Vec<(usize, usize)> {
    let mut v = MacroRulesRangeVisitor::default();
    v.visit_file(syntax);
    v.ranges
}

/// Return `true` if `line` (1-based) falls within any of the given inclusive `ranges`.
pub(crate) fn line_in_ranges(line: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| line >= *start && line <= *end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_in_ranges_handles_inclusive_endpoints() {
        let ranges = vec![(5, 10)];
        assert!(line_in_ranges(5, &ranges));
        assert!(line_in_ranges(10, &ranges));
        assert!(line_in_ranges(7, &ranges));
        assert!(!line_in_ranges(4, &ranges));
        assert!(!line_in_ranges(11, &ranges));
    }

    #[test]
    fn line_in_ranges_handles_multiple_ranges() {
        let ranges = vec![(1, 3), (10, 15)];
        assert!(line_in_ranges(2, &ranges));
        assert!(line_in_ranges(13, &ranges));
        assert!(!line_in_ranges(7, &ranges));
    }

    #[test]
    fn line_in_ranges_returns_false_for_empty_ranges() {
        assert!(!line_in_ranges(1, &[]));
    }

    #[test]
    fn collect_cfg_test_ranges_finds_cfg_test_module() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        std::fs::write(
            &path,
            "fn live() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n",
        )
        .expect("write");

        let ranges = collect_cfg_test_ranges(path.to_str().unwrap());
        assert!(!ranges.is_empty());
    }

    #[test]
    fn collect_cfg_test_ranges_returns_empty_when_no_cfg_test() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("b.rs");
        std::fs::write(&path, "fn live() {}\n").expect("write");
        let ranges = collect_cfg_test_ranges(path.to_str().unwrap());
        assert!(ranges.is_empty());
    }

    #[test]
    fn collect_cfg_test_ranges_returns_empty_for_unparseable_file() {
        let ranges = collect_cfg_test_ranges("/nonexistent/foo.rs");
        assert!(ranges.is_empty());
    }

    #[test]
    fn collect_macro_rules_ranges_finds_macro_rules_definitions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("c.rs");
        std::fs::write(
            &path,
            "macro_rules! my_macro { () => {}; }\nfn other() {}\n",
        )
        .expect("write");

        let ranges = collect_macro_rules_ranges(path.to_str().unwrap());
        assert!(!ranges.is_empty());
    }

    #[test]
    fn collect_macro_rules_ranges_ignores_other_macros() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("d.rs");
        std::fs::write(&path, "fn foo() { vec![1,2,3]; }\n").expect("write");
        let ranges = collect_macro_rules_ranges(path.to_str().unwrap());
        assert!(ranges.is_empty());
    }

    #[test]
    fn collect_cfg_test_ranges_marks_cfg_test_function_attr() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("e.rs");
        std::fs::write(
            &path,
            "fn live() {}\n#[cfg(test)]\nfn helper() {}\n",
        )
        .expect("write");
        let ranges = collect_cfg_test_ranges(path.to_str().unwrap());
        assert!(!ranges.is_empty());
    }
}
