use super::super::CodeVisitor;
use super::super::metadata::is_public;
use crate::index::visitor::render_cfg_attrs;
use crate::index::{EnumInfo, EnumVariantInfo, StructInfo};
use syn::spanned::Spanned;
use syn::{ItemEnum, ItemStruct};

pub fn visit_item_struct(visitor: &mut CodeVisitor, item_struct: &ItemStruct) {
    let name = item_struct.ident.to_string();
    let signature = crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #item_struct }));
    let is_pub = is_public(&item_struct.vis);

    let generics = item_struct
        .generics
        .params
        .iter()
        .map(|param| crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #param })))
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
                let ty_str = crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #ty }));
                format!("{}: {}", name, ty_str)
            })
            .collect(),
        syn::Fields::Unnamed(fields) => fields
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let ty = &field.ty;
                let ty_str = crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #ty }));
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
    let signature = crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #item_enum }));
    let is_pub = is_public(&item_enum.vis);

    let generics = item_enum
        .generics
        .params
        .iter()
        .map(|param| crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #param })))
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
                        let ty_str = crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #ty }));
                        format!("{}: {}", name, ty_str)
                    })
                    .collect(),
                syn::Fields::Unnamed(fields) => fields
                    .unnamed
                    .iter()
                    .enumerate()
                    .map(|(i, field)| {
                        let ty = &field.ty;
                        let ty_str = crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #ty }));
                        format!("{}: {}", i, ty_str)
                    })
                    .collect(),
                syn::Fields::Unit => Vec::new(),
            };
            let discriminant = variant
                .discriminant
                .as_ref()
                .map(|(_, expr)| crate::index::visitor::metadata::normalize_signature(&format!("{}", quote::quote! { #expr })));
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
}
