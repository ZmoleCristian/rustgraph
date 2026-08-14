use super::calls;
use super::functions;
use super::types;
use crate::index::infer_file_module_segments;
use crate::index::visitor::{CodeVisitor, attrs_imply_cfg_test, render_cfg_attrs};
use syn::visit::Visit;
use syn::{
    ExprCall, ExprMacro, ExprMethodCall, ImplItemConst, ImplItemFn, ItemConst, ItemEnum, ItemFn,
    ItemImpl, ItemMacro, ItemMod, ItemStatic, ItemStruct, ItemTrait, ItemType, ItemUse,
    TraitItemFn, UseTree,
};

fn collect_use_aliases(tree: &UseTree, out: &mut Vec<(String, String)>) {
    match tree {
        UseTree::Path(p) => collect_use_aliases(&p.tree, out),
        UseTree::Group(g) => {
            for item in &g.items {
                collect_use_aliases(item, out);
            }
        }
        UseTree::Rename(r) => {
            out.push((r.rename.to_string(), r.ident.to_string()));
        }
        UseTree::Name(_) | UseTree::Glob(_) => {}
    }
}

fn collect_pub_use_reexports(
    tree: &UseTree,
    prefix: &mut Vec<String>,
    out: &mut Vec<(String, String)>,
) {
    match tree {
        UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            collect_pub_use_reexports(&p.tree, prefix, out);
            prefix.pop();
        }
        UseTree::Group(g) => {
            for item in &g.items {
                collect_pub_use_reexports(item, prefix, out);
            }
        }
        UseTree::Name(name) => {
            let alias_local = name.ident.to_string();
            let mut full = prefix.clone();
            full.push(alias_local.clone());
            out.push((alias_local, full.join("::")));
        }
        UseTree::Rename(r) => {
            let mut full = prefix.clone();
            full.push(r.ident.to_string());
            out.push((r.rename.to_string(), full.join("::")));
        }
        UseTree::Glob(_) => {}
    }
}

impl<'ast> Visit<'ast> for CodeVisitor {
    fn visit_item_fn(&mut self, item_fn: &'ast ItemFn) {
        functions::visit_item_fn(self, item_fn);
    }

    fn visit_item_impl(&mut self, item_impl: &'ast ItemImpl) {
        functions::visit_item_impl(self, item_impl);
    }

    fn visit_item_trait(&mut self, item_trait: &'ast ItemTrait) {
        functions::visit_item_trait(self, item_trait);
    }

    fn visit_impl_item_fn(&mut self, item_fn: &'ast ImplItemFn) {
        functions::visit_impl_item_fn(self, item_fn);
    }

    fn visit_trait_item_fn(&mut self, item_fn: &'ast TraitItemFn) {
        functions::visit_trait_item_fn(self, item_fn);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        calls::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        calls::visit_expr_method_call(self, call);
    }

    fn visit_expr_macro(&mut self, macro_call: &'ast ExprMacro) {
        calls::visit_expr_macro(self, macro_call);
    }

    fn visit_item_macro(&mut self, macro_call: &'ast ItemMacro) {
        calls::visit_item_macro(self, macro_call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        calls::record_macro_token_calls(self, mac);
        syn::visit::visit_macro(self, mac);
    }

    fn visit_item_struct(&mut self, item_struct: &'ast ItemStruct) {
        types::visit_item_struct(self, item_struct);
    }

    fn visit_item_enum(&mut self, item_enum: &'ast ItemEnum) {
        types::visit_item_enum(self, item_enum);
    }

    fn visit_item_const(&mut self, item_const: &'ast ItemConst) {
        types::visit_item_const(self, item_const);
    }

    fn visit_item_static(&mut self, item_static: &'ast ItemStatic) {
        types::visit_item_static(self, item_static);
    }

    fn visit_impl_item_const(&mut self, item_const: &'ast ImplItemConst) {
        types::visit_impl_item_const(self, item_const);
    }

    fn visit_item_type(&mut self, item_type: &'ast ItemType) {
        types::visit_item_type(self, item_type);
    }

    fn visit_item_use(&mut self, item_use: &'ast ItemUse) {
        collect_use_aliases(&item_use.tree, &mut self.aliases);

        if matches!(item_use.vis, syn::Visibility::Public(_)) {
            let mut module_segs = infer_file_module_segments(&self.file_path);
            module_segs.extend(self.mod_stack.iter().cloned());
            let containing_module = module_segs.join("::");

            let mut edges: Vec<(String, String)> = Vec::new();
            let mut prefix: Vec<String> = Vec::new();
            collect_pub_use_reexports(&item_use.tree, &mut prefix, &mut edges);
            for (alias_local, target_full_path) in edges {
                self.reexports
                    .push((containing_module.clone(), alias_local, target_full_path));
            }
        }
        syn::visit::visit_item_use(self, item_use);
    }

    fn visit_item_mod(&mut self, item_mod: &'ast ItemMod) {
        let bumped = attrs_imply_cfg_test(&item_mod.attrs);
        if bumped {
            self.cfg_test_depth += 1;
        }

        let pushed_mod = if item_mod.content.is_some() {
            self.mod_stack.push(item_mod.ident.to_string());
            true
        } else {
            false
        };

        let mod_cfgs = if item_mod.content.is_some() {
            render_cfg_attrs(&item_mod.attrs)
        } else {
            Vec::new()
        };
        let mod_cfg_pushed = mod_cfgs.len();
        for c in mod_cfgs {
            self.mod_cfg_stack.push(c);
        }
        syn::visit::visit_item_mod(self, item_mod);
        for _ in 0..mod_cfg_pushed {
            self.mod_cfg_stack.pop();
        }
        if pushed_mod {
            self.mod_stack.pop();
        }
        if bumped {
            self.cfg_test_depth -= 1;
        }
    }
}
