use crate::{FunctionInfo, build_function_module_lookup, function_id};
use std::collections::HashMap;

use super::super::super::model::{is_generated_path, is_test_helper_name, is_test_path};

pub struct CandidateContext<'a> {
    pub candidates: Vec<&'a FunctionInfo>,
    pub candidate_name_counts: HashMap<String, usize>,
    pub candidates_by_name: HashMap<String, Vec<&'a FunctionInfo>>,
    pub function_by_id: HashMap<String, &'a FunctionInfo>,
    pub module_lookup: HashMap<String, Vec<String>>,
}

pub fn build_candidate_context(functions: &[FunctionInfo]) -> CandidateContext<'_> {
    let mut candidates: Vec<&FunctionInfo> = functions
        .iter()
        .filter(|f| f.is_pub)
        .filter(|f| f.kind == "function" || f.kind.starts_with("method("))
        .filter(|f| f.name != "main")
        .filter(|f| !is_test_helper_name(&f.name))
        .filter(|f| !is_test_path(&f.file_path))
        .filter(|f| !is_generated_path(&f.file_path))
        .collect();

    candidates.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.start_line.cmp(&b.start_line))
            .then(a.name.cmp(&b.name))
    });

    let mut candidate_name_counts: HashMap<String, usize> = HashMap::new();
    for func in &candidates {
        *candidate_name_counts.entry(func.name.clone()).or_default() += 1;
    }

    let mut candidates_by_name: HashMap<String, Vec<&FunctionInfo>> = HashMap::new();
    for func in &candidates {
        candidates_by_name
            .entry(func.name.clone())
            .or_default()
            .push(*func);
    }

    let function_by_id = functions
        .iter()
        .map(|function| (function_id(function), function))
        .collect::<HashMap<_, _>>();

    let module_lookup = build_function_module_lookup(functions);

    CandidateContext {
        candidates,
        candidate_name_counts,
        candidates_by_name,
        function_by_id,
        module_lookup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn func(name: &str, file_path: &str, kind: &str, is_pub: bool) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}"),
            file_path: file_path.to_string(),
            start_line: 1,
            end_line: 1,
            is_async: false,
            is_unsafe: false,
            is_pub,
            is_const: false,
            is_test: false,
            generics: vec![],
            parameters: vec![],
            return_type: None,
            kind: kind.to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    #[test]
    fn build_candidate_context_filters_non_pub_functions() {
        let funcs = vec![
            func("priv_one", "src/a.rs", "function", false),
            func("pub_one", "src/a.rs", "function", true),
        ];
        let ctx = build_candidate_context(&funcs);
        assert_eq!(ctx.candidates.len(), 1);
        assert_eq!(ctx.candidates[0].name, "pub_one");
    }

    #[test]
    fn build_candidate_context_filters_main_function() {
        let funcs = vec![
            func("main", "src/main.rs", "function", true),
            func("util", "src/lib.rs", "function", true),
        ];
        let ctx = build_candidate_context(&funcs);
        assert_eq!(ctx.candidates.len(), 1);
        assert_eq!(ctx.candidates[0].name, "util");
    }

    #[test]
    fn build_candidate_context_filters_test_helpers_and_test_paths() {
        let funcs = vec![
            func("test_helper", "src/lib.rs", "function", true),
            func("setup_for_test", "src/lib.rs", "function", true),
            func("normal", "src/tests/foo.rs", "function", true),
            func("kept", "src/lib.rs", "function", true),
        ];
        let ctx = build_candidate_context(&funcs);
        assert_eq!(ctx.candidates.len(), 1);
        assert_eq!(ctx.candidates[0].name, "kept");
    }

    #[test]
    fn build_candidate_context_filters_generated_files() {
        let funcs = vec![
            func("api_a", "src/protocol_generated.rs", "function", true),
            func("kept", "src/lib.rs", "function", true),
        ];
        let ctx = build_candidate_context(&funcs);
        assert_eq!(ctx.candidates.len(), 1);
        assert_eq!(ctx.candidates[0].name, "kept");
    }

    #[test]
    fn build_candidate_context_keeps_method_kinds() {
        let funcs = vec![func("foo", "src/lib.rs", "method(Owner)", true)];
        let ctx = build_candidate_context(&funcs);
        assert_eq!(ctx.candidates.len(), 1);
    }

    #[test]
    fn build_candidate_context_skips_trait_fn_kind() {
        let funcs = vec![func("foo", "src/lib.rs", "trait_fn(Trait)", true)];
        let ctx = build_candidate_context(&funcs);
        assert_eq!(ctx.candidates.len(), 0);
    }

    #[test]
    fn build_candidate_context_counts_duplicate_names() {
        let funcs = vec![
            func("dup", "src/a.rs", "function", true),
            func("dup", "src/b.rs", "function", true),
            func("uniq", "src/c.rs", "function", true),
        ];
        let ctx = build_candidate_context(&funcs);
        assert_eq!(ctx.candidate_name_counts.get("dup"), Some(&2));
        assert_eq!(ctx.candidate_name_counts.get("uniq"), Some(&1));
        assert_eq!(ctx.candidates_by_name.get("dup").map(|v| v.len()), Some(2));
    }

    #[test]
    fn build_candidate_context_function_by_id_maps_all_functions() {
        let funcs = vec![
            func("priv_fn", "src/a.rs", "function", false),
            func("pub_fn", "src/b.rs", "function", true),
        ];
        let ctx = build_candidate_context(&funcs);

        assert_eq!(ctx.function_by_id.len(), 2);
    }

    #[test]
    fn build_candidate_context_sorts_candidates_by_path_line_name() {
        let funcs = vec![
            func("z", "src/c.rs", "function", true),
            func("a", "src/a.rs", "function", true),
            func("m", "src/b.rs", "function", true),
        ];
        let ctx = build_candidate_context(&funcs);
        let names: Vec<_> = ctx.candidates.iter().map(|f| f.name.clone()).collect();
        assert_eq!(
            names,
            vec!["a".to_string(), "m".to_string(), "z".to_string()]
        );
    }
}
