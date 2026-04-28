//! Mutable visitor state accumulating all symbols and call sites found during AST traversal.

use super::super::{CallSite, EnumInfo, FunctionInfo, StructInfo};
use super::make_function_id;
use super::metadata::FnMetadata;
use std::collections::HashMap;

/// Accumulator threaded through the `syn::visit::Visit` traversal of one file.
pub(crate) struct CodeVisitor {
    /// Functions collected so far.
    pub functions: Vec<FunctionInfo>,
    /// Structs collected so far.
    pub structs: Vec<StructInfo>,
    /// Enums collected so far.
    pub enums: Vec<EnumInfo>,
    /// Absolute or relative path of the file being visited.
    pub file_path: String,
    /// Stable ID of the function currently being traversed, if any.
    pub current_function_id: Option<String>,
    /// Display name of the function currently being traversed, if any.
    pub current_function_name: Option<String>,
    /// Caller-id → list of raw callee names (built alongside `call_sites`).
    pub function_calls: HashMap<String, Vec<String>>,
    /// All call sites found in the file.
    pub call_sites: Vec<CallSite>,

    /// Type-alias pairs `(alias_name, original_name)` found in the file.
    pub aliases: Vec<(String, String)>,

    /// Re-export triples `(source_path, exported_name, alias)`.
    pub reexports: Vec<(String, String, String)>,
    /// Stack of `impl` type names for the current nesting level.
    pub(super) impl_stack: Vec<String>,
    /// Stack of trait names for the current `impl Trait for Type` nesting level.
    pub(super) trait_stack: Vec<String>,
    /// Nesting depth inside `#[cfg(test)]` modules or `#[test]` items.
    pub(crate) cfg_test_depth: usize,

    /// Module names pushed as the visitor descends into inline `mod` blocks.
    pub(crate) mod_stack: Vec<String>,

    /// `cfg(...)` predicates inherited from enclosing inline modules.
    pub(crate) mod_cfg_stack: Vec<String>,
}

impl CodeVisitor {
    /// Create a new visitor for `file_path` with all collections empty.
    pub fn new(file_path: String) -> Self {
        Self {
            functions: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            file_path,
            current_function_id: None,
            current_function_name: None,
            function_calls: HashMap::new(),
            call_sites: Vec::new(),
            aliases: Vec::new(),
            reexports: Vec::new(),
            impl_stack: Vec::new(),
            trait_stack: Vec::new(),
            cfg_test_depth: 0,
            mod_stack: Vec::new(),
            mod_cfg_stack: Vec::new(),
        }
    }


    /// Combine inherited module-level `cfg` predicates with the item's own predicates.
    pub(crate) fn effective_cfg_attrs(&self, own: Vec<String>) -> Vec<String> {
        let mut combined = self.mod_cfg_stack.clone();
        combined.extend(own);
        combined
    }

    /// Return `true` when the visitor is currently inside a `#[cfg(test)]` context.
    pub(crate) fn in_cfg_test(&self) -> bool {
        self.cfg_test_depth > 0
    }

    pub(super) fn record_call_site(
        &mut self,
        callee: String,
        callee_base: String,
        call_kind: &str,
        span: proc_macro2::Span,
    ) {
        if let Some(caller_id) = &self.current_function_id {
            self.function_calls
                .entry(caller_id.clone())
                .or_default()
                .push(callee.clone());
        }

        let loc = span.start();
        self.call_sites.push(CallSite {
            caller_id: self.current_function_id.clone(),
            caller_name: self.current_function_name.clone(),
            callee,
            callee_base,
            call_kind: call_kind.to_string(),
            file_path: self.file_path.clone(),
            line: loc.line,
            column: loc.column,
        });
    }

    pub(super) fn push_function(
        &mut self,
        meta: &FnMetadata,
        is_pub: bool,
        is_test: bool,
        kind: String,
        cfg_attrs: Vec<String>,
    ) -> String {
        let func_id = make_function_id(&self.file_path, meta.start_line, &meta.name);
        self.functions.push(FunctionInfo {
            name: meta.name.clone(),
            signature: meta.signature.clone(),
            file_path: self.file_path.clone(),
            start_line: meta.start_line,
            end_line: meta.end_line,
            is_async: meta.is_async,
            is_unsafe: meta.is_unsafe,
            is_pub,
            is_const: meta.is_const,
            is_test,
            generics: meta.generics.clone(),
            parameters: meta.parameters.clone(),
            return_type: meta.return_type.clone(),
            kind,
            cfg_attrs,
        });
        func_id
    }

    pub(super) fn set_function_context(&mut self, func_id: String, display_name: String) {
        self.current_function_id = Some(func_id.clone());
        self.current_function_name = Some(display_name);
        self.function_calls.insert(func_id, Vec::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::visitor::metadata::FnMetadata;
    use syn::parse_quote;
    use syn::spanned::Spanned;

    #[test]
    fn new_visitor_starts_with_empty_collections() {
        let v = CodeVisitor::new("src/lib.rs".to_string());
        assert!(v.functions.is_empty());
        assert!(v.structs.is_empty());
        assert!(v.enums.is_empty());
        assert_eq!(v.file_path, "src/lib.rs");
        assert!(v.current_function_id.is_none());
        assert!(v.function_calls.is_empty());
    }

    #[test]
    fn push_function_records_function_info() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: syn::ItemFn = parse_quote! {
            pub fn alpha() {}
        };
        let meta = FnMetadata::from_sig(&item.sig, item.span());
        let id = v.push_function(&meta, true, false, "function".to_string(), Vec::new());
        assert!(id.contains("alpha"));
        assert_eq!(v.functions.len(), 1);
        assert_eq!(v.functions[0].name, "alpha");
        assert!(v.functions[0].is_pub);
    }

    #[test]
    fn record_call_site_associates_with_current_caller() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        v.set_function_context("file:1:caller".to_string(), "caller".to_string());
        let item: syn::ItemFn = parse_quote! { fn placeholder() {} };
        let span = item.span();
        v.record_call_site("foo".to_string(), "foo".to_string(), "function", span);
        assert_eq!(v.call_sites.len(), 1);
        assert_eq!(
            v.call_sites[0].caller_id.as_deref(),
            Some("file:1:caller")
        );
        assert_eq!(v.function_calls["file:1:caller"], vec!["foo"]);
    }

    #[test]
    fn record_call_site_with_no_caller_skips_function_calls_map() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: syn::ItemFn = parse_quote! { fn placeholder() {} };
        let span = item.span();
        v.record_call_site("foo".to_string(), "foo".to_string(), "function", span);
        assert_eq!(v.call_sites.len(), 1);
        assert!(v.call_sites[0].caller_id.is_none());
        assert!(v.function_calls.is_empty());
    }

    #[test]
    fn set_function_context_initializes_empty_call_list() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        v.set_function_context("a".to_string(), "a".to_string());
        assert_eq!(v.current_function_id.as_deref(), Some("a"));
        assert!(v.function_calls["a"].is_empty());
    }
}
