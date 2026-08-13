use super::super::CodeVisitor;
use super::super::metadata::{FnMetadata, is_public};
use crate::index::visitor::{attrs_imply_cfg_test, render_cfg_attrs};
use syn::spanned::Spanned;
use syn::{ImplItemFn, ItemFn, ItemImpl, ItemTrait, TraitItemFn};

fn has_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.meta.path();
        if path.is_ident("test") {
            return true;
        }
        if let Some(last) = path.segments.last()
            && last.ident == "test"
        {
            return true;
        }
        false
    })
}

pub fn visit_item_fn(visitor: &mut CodeVisitor, item_fn: &ItemFn) {
    let bumped = attrs_imply_cfg_test(&item_fn.attrs);
    if bumped {
        visitor.cfg_test_depth += 1;
    }
    let meta = FnMetadata::from_sig(&item_fn.sig, item_fn.span());
    let is_test = has_test_attr(&item_fn.attrs) || visitor.in_cfg_test();
    let cfg_attrs = visitor.effective_cfg_attrs(render_cfg_attrs(&item_fn.attrs));
    let func_id = visitor.push_function(
        &meta,
        is_public(&item_fn.vis),
        is_test,
        "function".to_string(),
        cfg_attrs,
    );

    let prev_id = visitor.current_function_id.take();
    let prev_name = visitor.current_function_name.take();
    visitor.set_function_context(func_id, meta.name.clone());

    syn::visit::visit_item_fn(visitor, item_fn);

    visitor.current_function_id = prev_id;
    visitor.current_function_name = prev_name;
    if bumped {
        visitor.cfg_test_depth -= 1;
    }
}

pub fn visit_item_impl(visitor: &mut CodeVisitor, item_impl: &ItemImpl) {
    let bumped = attrs_imply_cfg_test(&item_impl.attrs);
    if bumped {
        visitor.cfg_test_depth += 1;
    }
    let self_ty = &item_impl.self_ty;
    let ty = format!("{}", quote::quote! { #self_ty });
    visitor.impl_stack.push(ty);
    syn::visit::visit_item_impl(visitor, item_impl);
    visitor.impl_stack.pop();
    if bumped {
        visitor.cfg_test_depth -= 1;
    }
}

pub fn visit_item_trait(visitor: &mut CodeVisitor, item_trait: &ItemTrait) {
    let bumped = attrs_imply_cfg_test(&item_trait.attrs);
    if bumped {
        visitor.cfg_test_depth += 1;
    }
    super::types::record_item_trait(visitor, item_trait);
    visitor.trait_stack.push(item_trait.ident.to_string());
    syn::visit::visit_item_trait(visitor, item_trait);
    visitor.trait_stack.pop();
    if bumped {
        visitor.cfg_test_depth -= 1;
    }
}

pub fn visit_impl_item_fn(visitor: &mut CodeVisitor, item_fn: &ImplItemFn) {
    let meta = FnMetadata::from_sig(&item_fn.sig, item_fn.span());
    let container = visitor
        .impl_stack
        .last()
        .cloned()
        .unwrap_or_else(|| "<impl>".to_string());
    let is_test = has_test_attr(&item_fn.attrs)
        || attrs_imply_cfg_test(&item_fn.attrs)
        || visitor.in_cfg_test();
    let cfg_attrs = visitor.effective_cfg_attrs(render_cfg_attrs(&item_fn.attrs));
    let func_id = visitor.push_function(
        &meta,
        is_public(&item_fn.vis),
        is_test,
        format!("method({})", container),
        cfg_attrs,
    );

    let prev_id = visitor.current_function_id.take();
    let prev_name = visitor.current_function_name.take();
    let display_name = visitor
        .impl_stack
        .last()
        .map(|c| format!("{}::{}", c, meta.name))
        .unwrap_or_else(|| meta.name.clone());
    visitor.set_function_context(func_id, display_name);

    syn::visit::visit_impl_item_fn(visitor, item_fn);

    visitor.current_function_id = prev_id;
    visitor.current_function_name = prev_name;
}

pub fn visit_trait_item_fn(visitor: &mut CodeVisitor, item_fn: &TraitItemFn) {
    let meta = FnMetadata::from_sig(&item_fn.sig, item_fn.span());
    let trait_name = visitor
        .trait_stack
        .last()
        .cloned()
        .unwrap_or_else(|| "<trait>".to_string());
    let is_test = has_test_attr(&item_fn.attrs)
        || attrs_imply_cfg_test(&item_fn.attrs)
        || visitor.in_cfg_test();
    let cfg_attrs = visitor.effective_cfg_attrs(render_cfg_attrs(&item_fn.attrs));
    let func_id = visitor.push_function(
        &meta,
        false,
        is_test,
        format!("trait_fn({})", trait_name),
        cfg_attrs,
    );

    let prev_id = visitor.current_function_id.take();
    let prev_name = visitor.current_function_name.take();
    let display_name = visitor
        .trait_stack
        .last()
        .map(|t| format!("{}::{}", t, meta.name))
        .unwrap_or_else(|| meta.name.clone());
    visitor.set_function_context(func_id, display_name);

    syn::visit::visit_trait_item_fn(visitor, item_fn);

    visitor.current_function_id = prev_id;
    visitor.current_function_name = prev_name;
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::visitor::state::CodeVisitor;
    use syn::parse_quote;

    #[test]
    fn visit_item_fn_preserves_cargo_test_attr_in_signature() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());

        let item_fn: ItemFn = parse_quote! {
            #[test]
            fn my_test() {}
        };
        visit_item_fn(&mut visitor, &item_fn);

        let item_fn2: ItemFn = parse_quote! {
            #[tokio::test]
            async fn my_async_test() {}
        };
        visit_item_fn(&mut visitor, &item_fn2);

        let item_fn3: ItemFn = parse_quote! {
            fn not_a_test() {}
        };
        visit_item_fn(&mut visitor, &item_fn3);

        assert_eq!(visitor.functions.len(), 3);
        assert!(visitor.functions[0].is_test);
        assert!(visitor.functions[1].is_test);
        assert!(!visitor.functions[2].is_test);
    }

    #[test]
    fn visit_item_impl_records_clean_owner_type_for_methods() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_impl: ItemImpl = parse_quote! {
            impl ComputeInput {
                pub fn new(a: i32, b: i32) -> Self {
                    Self { a, b }
                }
            }
        };

        visit_item_impl(&mut visitor, &item_impl);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].kind, "method(ComputeInput)");
    }

    #[test]
    fn visit_item_trait_records_trait_kind_with_trait_name() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_trait: ItemTrait = parse_quote! {
            pub trait MyTrait {
                fn method(&self);
            }
        };
        visit_item_trait(&mut visitor, &item_trait);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].kind, "trait_fn(MyTrait)");
        assert!(!visitor.functions[0].is_pub, "trait fn is_pub is always false in this code");
    }

    #[test]
    fn has_test_attr_recognises_path_segment_test() {
        let attrs: syn::ItemFn = parse_quote! { #[some::path::test] fn x() {} };
        assert!(has_test_attr(&attrs.attrs));
    }

    #[test]
    fn has_test_attr_returns_false_for_unrelated_attrs() {
        let attrs: syn::ItemFn = parse_quote! { #[derive(Debug)] fn x() {} };
        assert!(!has_test_attr(&attrs.attrs));
    }

    #[test]
    fn has_test_attr_handles_no_attributes() {
        let attrs: syn::ItemFn = parse_quote! { fn x() {} };
        assert!(!has_test_attr(&attrs.attrs));
    }

    #[test]
    fn visit_impl_item_fn_uses_impl_stack_for_owner() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_impl: ItemImpl = parse_quote! {
            impl<'a, T: Display> Wrapper<'a, T> {
                pub fn show(&self) {}
            }
        };
        visit_item_impl(&mut visitor, &item_impl);
        assert_eq!(visitor.functions.len(), 1);

        assert!(visitor.functions[0].kind.starts_with("method("));
        assert!(visitor.functions[0].kind.contains("Wrapper"));
    }

    #[test]
    fn visit_trait_item_fn_records_test_attr() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_trait: ItemTrait = parse_quote! {
            trait T {
                #[test]
                fn t(&self);
            }
        };
        visit_item_trait(&mut visitor, &item_trait);
        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn visit_item_fn_marks_cfg_test_fn_as_test() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_fn: ItemFn = parse_quote! {
            #[cfg(test)]
            fn helper() {}
        };
        visit_item_fn(&mut visitor, &item_fn);
        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn visit_item_impl_marks_methods_of_cfg_test_impl_as_test() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_impl: ItemImpl = parse_quote! {
            #[cfg(test)]
            impl Config {
                pub fn test_default() -> Self {
                    Self::default()
                }
            }
        };
        visit_item_impl(&mut visitor, &item_impl);
        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_test);

        // The cfg-test scope must not leak past the impl block.
        let item_fn: ItemFn = parse_quote! {
            fn production() {}
        };
        visit_item_fn(&mut visitor, &item_fn);
        assert_eq!(visitor.functions.len(), 2);
        assert!(!visitor.functions[1].is_test);
    }

    #[test]
    fn visit_item_impl_without_cfg_test_keeps_methods_production() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_impl: ItemImpl = parse_quote! {
            impl Config {
                pub fn new() -> Self {
                    Self::default()
                }
            }
        };
        visit_item_impl(&mut visitor, &item_impl);
        assert_eq!(visitor.functions.len(), 1);
        assert!(!visitor.functions[0].is_test);
    }

    #[test]
    fn visit_item_trait_marks_methods_of_cfg_test_trait_as_test() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_trait: ItemTrait = parse_quote! {
            #[cfg(test)]
            trait Fixture {
                fn build(&self);
            }
        };
        visit_item_trait(&mut visitor, &item_trait);
        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn visit_item_fn_keeps_cfg_not_test_fn_production() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_fn: ItemFn = parse_quote! {
            #[cfg(not(test))]
            fn wire_up() {}
        };
        visit_item_fn(&mut visitor, &item_fn);
        assert_eq!(visitor.functions.len(), 1);
        assert!(!visitor.functions[0].is_test);
    }

    #[test]
    fn visit_item_impl_keeps_methods_of_cfg_not_test_impl_production() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_impl: ItemImpl = parse_quote! {
            #[cfg(not(test))]
            impl Config {
                pub fn wire_up() -> Self {
                    Self::default()
                }
            }
        };
        visit_item_impl(&mut visitor, &item_impl);
        assert_eq!(visitor.functions.len(), 1);
        assert!(!visitor.functions[0].is_test);
    }

    #[test]
    fn visit_item_fn_keeps_test_feature_fn_production() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_fn: ItemFn = parse_quote! {
            #[cfg(feature = "test-utils")]
            fn helper() {}
        };
        visit_item_fn(&mut visitor, &item_fn);
        assert_eq!(visitor.functions.len(), 1);
        assert!(!visitor.functions[0].is_test);
    }

    #[test]
    fn visit_item_fn_marks_cfg_all_test_fn_as_test() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_fn: ItemFn = parse_quote! {
            #[cfg(all(test, unix))]
            fn helper() {}
        };
        visit_item_fn(&mut visitor, &item_fn);
        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].is_test);
    }

    #[test]
    fn visit_item_fn_marks_fn_nested_in_cfg_test_fn_body_as_test() {
        let mut visitor = CodeVisitor::new("test.rs".to_string());
        let item_fn: ItemFn = parse_quote! {
            #[cfg(test)]
            fn outer() {
                fn inner() {}
            }
        };
        visit_item_fn(&mut visitor, &item_fn);
        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[1].name, "inner");
        assert!(visitor.functions[0].is_test);
        assert!(visitor.functions[1].is_test);

        // The cfg-test scope must not leak past the outer fn.
        let item_fn2: ItemFn = parse_quote! {
            fn production() {}
        };
        visit_item_fn(&mut visitor, &item_fn2);
        assert_eq!(visitor.functions.len(), 3);
        assert!(!visitor.functions[2].is_test);
    }
}
