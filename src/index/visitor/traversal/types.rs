use super::super::CodeVisitor;
use super::super::metadata::is_public;
use crate::index::visitor::render_cfg_attrs;
use crate::index::{ConstInfo, EnumInfo, EnumVariantInfo, StructInfo, TypeDeclInfo};
use syn::spanned::Spanned;
use syn::{ImplItemConst, ItemConst, ItemEnum, ItemStatic, ItemStruct, ItemTrait, ItemType};

pub fn visit_item_struct(visitor: &mut CodeVisitor, item_struct: &ItemStruct) {
    let name = item_struct.ident.to_string();
    let signature = crate::index::visitor::metadata::normalize_signature(&format!(
        "{}",
        quote::quote! { #item_struct }
    ));
    let is_pub = is_public(&item_struct.vis);

    let generics = item_struct
        .generics
        .params
        .iter()
        .map(|param| {
            crate::index::visitor::metadata::normalize_signature(&format!(
                "{}",
                quote::quote! { #param }
            ))
        })
        .collect();

    let fields = match &item_struct.fields {
        syn::Fields::Named(fields) => fields
            .named
            .iter()
            .map(|field| {
                let name = field
                    .ident
                    .as_ref()
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "_".to_string());
                let ty = &field.ty;
                let ty_str = crate::index::visitor::metadata::normalize_signature(&format!(
                    "{}",
                    quote::quote! { #ty }
                ));
                format!("{}: {}", name, ty_str)
            })
            .collect(),
        syn::Fields::Unnamed(fields) => fields
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let ty = &field.ty;
                let ty_str = crate::index::visitor::metadata::normalize_signature(&format!(
                    "{}",
                    quote::quote! { #ty }
                ));
                format!("{}: {}", i, ty_str)
            })
            .collect(),
        syn::Fields::Unit => Vec::new(),
    };

    let span = item_struct.span();
    let start_line = span.start().line;
    let end_line = span.end().line;

    let cfg_attrs = visitor.effective_cfg_attrs(render_cfg_attrs(&item_struct.attrs));
    visitor.structs.push(StructInfo {
        name,
        signature,
        file_path: visitor.file_path.clone(),
        start_line,
        end_line,
        is_pub,
        generics,
        fields,
        kind: "struct".to_string(),
        cfg_attrs,
    });

    syn::visit::visit_item_struct(visitor, item_struct);
}

pub fn visit_item_enum(visitor: &mut CodeVisitor, item_enum: &ItemEnum) {
    let name = item_enum.ident.to_string();
    let signature = crate::index::visitor::metadata::normalize_signature(&format!(
        "{}",
        quote::quote! { #item_enum }
    ));
    let is_pub = is_public(&item_enum.vis);

    let generics = item_enum
        .generics
        .params
        .iter()
        .map(|param| {
            crate::index::visitor::metadata::normalize_signature(&format!(
                "{}",
                quote::quote! { #param }
            ))
        })
        .collect();

    let variants = item_enum
        .variants
        .iter()
        .map(|variant| {
            let fields = match &variant.fields {
                syn::Fields::Named(fields) => fields
                    .named
                    .iter()
                    .map(|field| {
                        let name = field
                            .ident
                            .as_ref()
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "_".to_string());
                        let ty = &field.ty;
                        let ty_str = crate::index::visitor::metadata::normalize_signature(
                            &format!("{}", quote::quote! { #ty }),
                        );
                        format!("{}: {}", name, ty_str)
                    })
                    .collect(),
                syn::Fields::Unnamed(fields) => fields
                    .unnamed
                    .iter()
                    .enumerate()
                    .map(|(i, field)| {
                        let ty = &field.ty;
                        let ty_str = crate::index::visitor::metadata::normalize_signature(
                            &format!("{}", quote::quote! { #ty }),
                        );
                        format!("{}: {}", i, ty_str)
                    })
                    .collect(),
                syn::Fields::Unit => Vec::new(),
            };
            let discriminant = variant.discriminant.as_ref().map(|(_, expr)| {
                crate::index::visitor::metadata::normalize_signature(&format!(
                    "{}",
                    quote::quote! { #expr }
                ))
            });
            EnumVariantInfo {
                name: variant.ident.to_string(),
                fields,
                discriminant,
            }
        })
        .collect();

    let span = item_enum.span();
    let start_line = span.start().line;
    let end_line = span.end().line;

    let cfg_attrs = visitor.effective_cfg_attrs(render_cfg_attrs(&item_enum.attrs));
    visitor.enums.push(EnumInfo {
        name,
        signature,
        file_path: visitor.file_path.clone(),
        start_line,
        end_line,
        is_pub,
        generics,
        variants,
        kind: "enum".to_string(),
        cfg_attrs,
    });

    syn::visit::visit_item_enum(visitor, item_enum);
}

fn push_const(
    visitor: &mut CodeVisitor,
    name: String,
    ty: &syn::Type,
    is_pub: bool,
    kind: String,
    span: proc_macro2::Span,
    attrs: &[syn::Attribute],
) {
    // quote! renders array types as `[T ; N]`; tighten to `[T; N]` for display.
    let ty_str =
        crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #ty }))
            .replace(" ; ", "; ");
    let keyword = if kind.starts_with("static") {
        kind.clone()
    } else {
        "const".to_string()
    };
    let signature = format!(
        "{}{} {}: {}",
        if is_pub { "pub " } else { "" },
        keyword,
        name,
        ty_str
    );
    let cfg_attrs = visitor.effective_cfg_attrs(render_cfg_attrs(attrs));
    visitor.consts.push(ConstInfo {
        name,
        signature,
        ty: ty_str,
        file_path: visitor.file_path.clone(),
        start_line: span.start().line,
        end_line: span.end().line,
        is_pub,
        kind,
        cfg_attrs,
    });
}

pub fn visit_item_const(visitor: &mut CodeVisitor, item: &ItemConst) {
    push_const(
        visitor,
        item.ident.to_string(),
        &item.ty,
        is_public(&item.vis),
        "const".to_string(),
        item.span(),
        &item.attrs,
    );
    syn::visit::visit_item_const(visitor, item);
}

pub fn visit_item_static(visitor: &mut CodeVisitor, item: &ItemStatic) {
    let kind = match item.mutability {
        syn::StaticMutability::Mut(_) => "static mut",
        _ => "static",
    };
    push_const(
        visitor,
        item.ident.to_string(),
        &item.ty,
        is_public(&item.vis),
        kind.to_string(),
        item.span(),
        &item.attrs,
    );
    syn::visit::visit_item_static(visitor, item);
}

pub fn visit_impl_item_const(visitor: &mut CodeVisitor, item: &ImplItemConst) {
    let container = visitor
        .impl_stack
        .last()
        .cloned()
        .unwrap_or_else(|| "<impl>".to_string());
    push_const(
        visitor,
        item.ident.to_string(),
        &item.ty,
        is_public(&item.vis),
        format!("const({})", container),
        item.span(),
        &item.attrs,
    );
    syn::visit::visit_impl_item_const(visitor, item);
}

/// Render generic params as `<T, U>` or empty when there are none.
fn render_generics(generics: &syn::Generics) -> String {
    if generics.params.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = generics
        .params
        .iter()
        .map(|p| {
            crate::index::visitor::metadata::normalize_signature(&format!(
                "{}",
                quote::quote! { #p }
            ))
        })
        .collect();
    format!("<{}>", parts.join(", "))
}

pub fn visit_item_type(visitor: &mut CodeVisitor, item: &ItemType) {
    let name = item.ident.to_string();
    let is_pub = is_public(&item.vis);
    let ty = &item.ty;
    let rhs =
        crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #ty }))
            .replace(" ; ", "; ");
    let signature = format!(
        "{}type {}{} = {}",
        if is_pub { "pub " } else { "" },
        name,
        render_generics(&item.generics),
        rhs
    );
    let span = item.span();
    let cfg_attrs = visitor.effective_cfg_attrs(render_cfg_attrs(&item.attrs));
    visitor.type_decls.push(TypeDeclInfo {
        name,
        signature,
        rhs: Some(rhs),
        file_path: visitor.file_path.clone(),
        start_line: span.start().line,
        end_line: span.end().line,
        is_pub,
        kind: "alias".to_string(),
        cfg_attrs,
    });
    syn::visit::visit_item_type(visitor, item);
}

/// Record the trait declaration itself as a findable symbol.
///
/// Called from `functions::visit_item_trait`, which owns the trait-stack
/// bookkeeping and the descent into trait items.
pub fn record_item_trait(visitor: &mut CodeVisitor, item: &ItemTrait) {
    let name = item.ident.to_string();
    let is_pub = is_public(&item.vis);
    let supertraits = if item.supertraits.is_empty() {
        String::new()
    } else {
        let bounds: Vec<String> = item
            .supertraits
            .iter()
            .map(|b| {
                crate::index::visitor::metadata::normalize_signature(&format!(
                    "{}",
                    quote::quote! { #b }
                ))
            })
            .collect();
        format!(": {}", bounds.join(" + "))
    };
    let signature = format!(
        "{}trait {}{}{}",
        if is_pub { "pub " } else { "" },
        name,
        render_generics(&item.generics),
        supertraits
    );
    let span = item.span();
    let cfg_attrs = visitor.effective_cfg_attrs(render_cfg_attrs(&item.attrs));
    visitor.type_decls.push(TypeDeclInfo {
        name,
        signature,
        rhs: None,
        file_path: visitor.file_path.clone(),
        start_line: span.start().line,
        end_line: span.end().line,
        is_pub,
        kind: "trait".to_string(),
        cfg_attrs,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn visit_item_struct_records_named_fields() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemStruct = parse_quote! {
            pub struct Foo {
                pub x: u32,
                y: String,
            }
        };
        visit_item_struct(&mut v, &item);
        assert_eq!(v.structs.len(), 1);
        let s = &v.structs[0];
        assert_eq!(s.name, "Foo");
        assert!(s.is_pub);
        assert_eq!(s.fields.len(), 2);
        assert_eq!(s.fields[0], "x: u32");
        assert_eq!(s.fields[1], "y: String");
        assert_eq!(s.kind, "struct");
    }

    #[test]
    fn visit_item_struct_records_tuple_fields_with_index_names() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemStruct = parse_quote! {
            struct Tup(u32, String);
        };
        visit_item_struct(&mut v, &item);
        let s = &v.structs[0];
        assert_eq!(s.fields, vec!["0: u32", "1: String"]);
        assert!(!s.is_pub);
    }

    #[test]
    fn visit_item_struct_unit_struct_has_no_fields() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemStruct = parse_quote! {
            pub struct Unit;
        };
        visit_item_struct(&mut v, &item);
        assert!(v.structs[0].fields.is_empty());
        assert!(v.structs[0].is_pub);
    }

    #[test]
    fn visit_item_struct_collects_generics() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemStruct = parse_quote! {
            pub struct Boxed<T: Clone> { value: T }
        };
        visit_item_struct(&mut v, &item);
        assert_eq!(v.structs[0].generics.len(), 1);
    }

    #[test]
    fn visit_item_enum_collects_variants() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemEnum = parse_quote! {
            pub enum E {
                A,
                B(u32),
                C { x: u8 },
            }
        };
        visit_item_enum(&mut v, &item);
        assert_eq!(v.enums.len(), 1);
        let e = &v.enums[0];
        assert_eq!(e.name, "E");
        assert!(e.is_pub);
        assert_eq!(e.variants.len(), 3);
        assert_eq!(e.variants[0].name, "A");
        assert!(e.variants[0].fields.is_empty());
        assert_eq!(e.variants[1].name, "B");
        assert_eq!(e.variants[1].fields, vec!["0: u32"]);
        assert_eq!(e.variants[2].name, "C");
        assert_eq!(e.variants[2].fields, vec!["x: u8"]);
    }

    #[test]
    fn visit_item_enum_records_discriminant_value() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemEnum = parse_quote! {
            pub enum Code {
                Ok = 0,
                Err = 1,
            }
        };
        visit_item_enum(&mut v, &item);
        let e = &v.enums[0];
        assert_eq!(e.variants[0].discriminant.as_deref(), Some("0"));
        assert_eq!(e.variants[1].discriminant.as_deref(), Some("1"));
    }

    #[test]
    fn visit_item_const_records_signature_without_initializer() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemConst = parse_quote! {
            pub const KIND_META: [KindMeta; 3] = [a, b, c];
        };
        visit_item_const(&mut v, &item);
        assert_eq!(v.consts.len(), 1);
        let c = &v.consts[0];
        assert_eq!(c.name, "KIND_META");
        assert!(c.is_pub);
        assert_eq!(c.kind, "const");
        assert_eq!(c.signature, "pub const KIND_META: [KindMeta; 3]");
        assert!(
            !c.signature.contains('='),
            "initializer must not leak into the signature: {}",
            c.signature
        );
    }

    #[test]
    fn visit_item_static_records_mutability_kind() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let imm: ItemStatic = parse_quote! { static LOG: Logger = Logger::new(); };
        let mu: ItemStatic = parse_quote! { pub static mut COUNTER: u64 = 0; };
        visit_item_static(&mut v, &imm);
        visit_item_static(&mut v, &mu);
        assert_eq!(v.consts[0].kind, "static");
        assert!(!v.consts[0].is_pub);
        assert_eq!(v.consts[1].kind, "static mut");
        assert!(v.consts[1].is_pub);
        assert_eq!(v.consts[1].signature, "pub static mut COUNTER: u64");
    }

    #[test]
    fn visit_impl_item_const_tags_container_type() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        v.impl_stack.push("Widget".to_string());
        let item: ImplItemConst = parse_quote! { pub const MAX: usize = 16; };
        visit_impl_item_const(&mut v, &item);
        assert_eq!(v.consts[0].kind, "const(Widget)");
        assert_eq!(v.consts[0].name, "MAX");
    }

    #[test]
    fn visit_item_type_records_alias_with_rhs() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemType = parse_quote! {
            pub type ChangedRanges = HashMap<String, Vec<(usize, usize)>>;
        };
        visit_item_type(&mut v, &item);
        assert_eq!(v.type_decls.len(), 1);
        let t = &v.type_decls[0];
        assert_eq!(t.name, "ChangedRanges");
        assert_eq!(t.kind, "alias");
        assert!(t.is_pub);
        assert!(
            t.signature.starts_with("pub type ChangedRanges = HashMap"),
            "{}",
            t.signature
        );
        assert!(t.rhs.as_deref().unwrap().starts_with("HashMap"));
    }

    #[test]
    fn visit_item_type_records_generic_alias() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemType = parse_quote! { type Pair<T> = (T, T); };
        visit_item_type(&mut v, &item);
        let t = &v.type_decls[0];
        assert!(!t.is_pub);
        assert_eq!(t.signature, "type Pair<T> = (T, T)");
    }

    #[test]
    fn record_item_trait_captures_supertraits() {
        let mut v = CodeVisitor::new("src/lib.rs".to_string());
        let item: ItemTrait = parse_quote! {
            pub trait Renderer: Send + Sync {
                fn draw(&self);
            }
        };
        record_item_trait(&mut v, &item);
        assert_eq!(v.type_decls.len(), 1);
        let t = &v.type_decls[0];
        assert_eq!(t.name, "Renderer");
        assert_eq!(t.kind, "trait");
        assert_eq!(t.signature, "pub trait Renderer: Send + Sync");
        assert!(t.rhs.is_none());
    }
}
