use super::super::calls::extract_macro_path;
use super::super::{CodeVisitor, extract_call_target_from_call};
use syn::spanned::Spanned;
use syn::{Expr, ExprCall, ExprMacro, ExprMethodCall, ItemMacro};

pub fn visit_expr_call(visitor: &mut CodeVisitor, call: &ExprCall) {
    if let Some(target) = extract_call_target_from_call(call) {
        visitor.record_call_site(target.full, target.base, "function", call.span());
    }
    syn::visit::visit_expr_call(visitor, call);
}

pub fn visit_expr_method_call(visitor: &mut CodeVisitor, call: &ExprMethodCall) {
    let receiver_info = match &*call.receiver {
        Expr::Path(path) => {
            let receiver = path
                .path
                .segments
                .iter()
                .map(|seg| seg.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            format!("{}.", receiver)
        }
        _ => "receiver.".to_string(),
    };

    let full = format!("{}{}", receiver_info, call.method);
    let base = call.method.to_string();
    visitor.record_call_site(full, base, "method", call.span());
    syn::visit::visit_expr_method_call(visitor, call);
}

pub fn visit_expr_macro(visitor: &mut CodeVisitor, macro_call: &ExprMacro) {
    let (full, base) = extract_macro_path(&macro_call.mac);
    visitor.record_call_site(full, base, "macro", macro_call.span());
    syn::visit::visit_expr_macro(visitor, macro_call);
}

pub fn visit_item_macro(visitor: &mut CodeVisitor, macro_call: &ItemMacro) {
    let (full, base) = extract_macro_path(&macro_call.mac);
    visitor.record_call_site(full, base, "macro", macro_call.span());
    syn::visit::visit_item_macro(visitor, macro_call);
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn visit_expr_method_call_records_method_call_site_with_receiver_path() {
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.set_function_context("id".to_string(), "name".to_string());

        let call: ExprMethodCall = parse_quote! { router.method() };
        visit_expr_method_call(&mut visitor, &call);

        let recorded = visitor
            .call_sites
            .iter()
            .find(|cs| cs.callee_base == "method")
            .expect("found method call");
        assert_eq!(recorded.call_kind, "method");
        assert_eq!(recorded.callee, "router.method");
    }

    #[test]
    fn visit_expr_method_call_records_unknown_receiver_as_receiver_dot() {
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.set_function_context("id".to_string(), "name".to_string());

        let call: ExprMethodCall = parse_quote! { (1 + 2).abs() };
        visit_expr_method_call(&mut visitor, &call);

        let recorded = visitor
            .call_sites
            .iter()
            .find(|cs| cs.callee_base == "abs")
            .expect("found abs call");
        assert!(recorded.callee.starts_with("receiver."));
    }

    #[test]
    fn visit_expr_macro_records_macro_call_site() {
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.set_function_context("id".to_string(), "name".to_string());

        let m: ExprMacro = parse_quote! { println!("hi") };
        visit_expr_macro(&mut visitor, &m);

        let cs = visitor.call_sites.iter().find(|cs| cs.callee_base == "println!");
        assert!(cs.is_some());
        assert_eq!(cs.unwrap().call_kind, "macro");
    }

    #[test]
    fn visit_item_macro_records_macro_at_item_position() {
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        let im: ItemMacro = parse_quote! { entrypoint!(process_instruction); };
        visit_item_macro(&mut visitor, &im);
        let cs = visitor.call_sites.iter().find(|cs| cs.callee_base == "entrypoint!");
        assert!(cs.is_some());
    }

    #[test]
    fn visit_expr_call_records_function_call() {
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.set_function_context("id".to_string(), "name".to_string());

        let call: ExprCall = parse_quote! { foo() };
        visit_expr_call(&mut visitor, &call);

        let cs = visitor.call_sites.iter().find(|cs| cs.callee_base == "foo");
        assert!(cs.is_some());
        assert_eq!(cs.unwrap().call_kind, "function");
    }
}
