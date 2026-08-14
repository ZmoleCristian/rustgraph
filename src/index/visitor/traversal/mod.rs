mod calls;
mod functions;
mod types;
mod visitor;

#[cfg(test)]
mod tests {
    use crate::index::visitor::CodeVisitor;
    use syn::visit::Visit;

    #[test]
    fn visit_file_records_function_struct_and_enum() {
        let content = r#"
            pub fn alpha() {}
            pub struct Foo { x: u32 }
            pub enum Bar { A, B }
        "#;
        let file = syn::parse_file(content).expect("parse");
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.visit_file(&file);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "alpha");
        assert_eq!(visitor.structs.len(), 1);
        assert_eq!(visitor.structs[0].name, "Foo");
        assert_eq!(visitor.enums.len(), 1);
        assert_eq!(visitor.enums[0].name, "Bar");
    }

    #[test]
    fn visit_file_records_calls_within_function_body() {
        let content = r#"
            pub fn helper() {}
            pub fn caller() {
                helper();
                helper();
            }
        "#;
        let file = syn::parse_file(content).expect("parse");
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.visit_file(&file);

        let caller = visitor
            .functions
            .iter()
            .find(|f| f.name == "caller")
            .expect("caller");
        let caller_id = format!("file.rs:{}:caller", caller.start_line);
        let helper_calls = visitor
            .call_sites
            .iter()
            .filter(|cs| cs.callee_base == "helper" && cs.caller_id.as_deref() == Some(&caller_id))
            .count();
        assert_eq!(helper_calls, 2);
    }

    #[test]
    fn visit_file_records_method_call_sites() {
        let content = r#"
            pub fn caller() {
                let v = vec![1, 2, 3];
                v.iter().count();
            }
        "#;
        let file = syn::parse_file(content).expect("parse");
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.visit_file(&file);
        let method_calls: Vec<_> = visitor
            .call_sites
            .iter()
            .filter(|cs| cs.call_kind == "method")
            .map(|cs| cs.callee_base.clone())
            .collect();
        assert!(method_calls.iter().any(|n| n == "iter"));
        assert!(method_calls.iter().any(|n| n == "count"));
    }

    #[test]
    fn visit_file_records_impl_methods_with_owner() {
        let content = r#"
            pub struct Foo;
            impl Foo {
                pub fn new() -> Self { Foo }
                pub fn run(&self) {}
            }
        "#;
        let file = syn::parse_file(content).expect("parse");
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.visit_file(&file);

        let new_fn = visitor
            .functions
            .iter()
            .find(|f| f.name == "new")
            .expect("new");
        let run_fn = visitor
            .functions
            .iter()
            .find(|f| f.name == "run")
            .expect("run");
        assert_eq!(new_fn.kind, "method(Foo)");
        assert_eq!(run_fn.kind, "method(Foo)");
    }

    #[test]
    fn visit_file_records_trait_methods_with_trait_kind() {
        let content = r#"
            pub trait Worker {
                fn work(&self);
            }
        "#;
        let file = syn::parse_file(content).expect("parse");
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.visit_file(&file);

        let work_fn = visitor
            .functions
            .iter()
            .find(|f| f.name == "work")
            .expect("work");
        assert_eq!(work_fn.kind, "trait_fn(Worker)");
    }

    #[test]
    fn visit_file_marks_test_attribute_functions() {
        let content = r#"
            #[test]
            fn it_works() {}
            fn not_a_test() {}
        "#;
        let file = syn::parse_file(content).expect("parse");
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.visit_file(&file);
        let it_works = visitor
            .functions
            .iter()
            .find(|f| f.name == "it_works")
            .expect("it_works");
        let not_a_test = visitor
            .functions
            .iter()
            .find(|f| f.name == "not_a_test")
            .expect("not_a_test");
        assert!(it_works.is_test);
        assert!(!not_a_test.is_test);
    }

    #[test]
    fn visit_file_marks_cfg_test_module_functions_as_test() {
        let content = r#"
            #[cfg(test)]
            mod tests {
                fn helper() {}
            }
        "#;
        let file = syn::parse_file(content).expect("parse");
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.visit_file(&file);
        let helper = visitor
            .functions
            .iter()
            .find(|f| f.name == "helper")
            .expect("helper");
        assert!(helper.is_test);
    }

    #[test]
    fn visit_file_keeps_cfg_not_test_module_functions_production() {
        let content = r#"
            #[cfg(not(test))]
            mod wiring {
                fn wire_up() {}
            }
        "#;
        let file = syn::parse_file(content).expect("parse");
        let mut visitor = CodeVisitor::new("file.rs".to_string());
        visitor.visit_file(&file);
        let wire_up = visitor
            .functions
            .iter()
            .find(|f| f.name == "wire_up")
            .expect("wire_up");
        assert!(!wire_up.is_test);
    }
}
