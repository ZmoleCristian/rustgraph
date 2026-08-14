use std::collections::HashMap;

use super::super::super::model::{
    AmbiguousNameDetail, DeadCodeReport, DeadCodeSkippedTotals, DeadFunction,
};
use super::candidates::CandidateContext;

pub fn build_ambiguous_details(
    ctx: &CandidateContext,
    unresolved_duplicate_non_test: &HashMap<String, usize>,
) -> Vec<AmbiguousNameDetail> {
    let mut ambiguous_name_details = unresolved_duplicate_non_test
        .iter()
        .map(|(name, unresolved_count)| {
            let mut locations = ctx
                .candidates_by_name
                .get(name)
                .map(|items| {
                    let mut locs = items
                        .iter()
                        .map(|f| format!("{}:{}", f.file_path, f.start_line))
                        .collect::<Vec<_>>();
                    locs.sort();
                    locs
                })
                .unwrap_or_default();
            if locations.len() > 12 {
                locations.truncate(12);
            }
            AmbiguousNameDetail {
                symbol: name.clone(),
                unresolved_non_test_call_sites_total: *unresolved_count,
                candidates_total: ctx.candidate_name_counts.get(name).copied().unwrap_or(0),
                candidate_locations: locations,
            }
        })
        .collect::<Vec<_>>();

    ambiguous_name_details.sort_by(|a, b| {
        b.unresolved_non_test_call_sites_total
            .cmp(&a.unresolved_non_test_call_sites_total)
            .then(a.symbol.cmp(&b.symbol))
    });

    ambiguous_name_details
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_report(
    candidates_total: usize,
    dead: Vec<DeadFunction>,
    skipped_reexported: usize,
    skipped_cfg_test: usize,
    skipped_runtime_entrypoints: usize,
    skipped_ambiguous_name: usize,
    skipped_framework_handler_refs: usize,
    ambiguous_name_details: Vec<AmbiguousNameDetail>,
) -> DeadCodeReport {
    DeadCodeReport {
        candidates_total,
        dead_functions_total: dead.len(),
        skipped: DeadCodeSkippedTotals {
            reexported_total: skipped_reexported,
            cfg_test_total: skipped_cfg_test,
            runtime_entrypoints_total: skipped_runtime_entrypoints,
            ambiguous_name_total: skipped_ambiguous_name,
            framework_handler_refs_total: skipped_framework_handler_refs,
        },
        ambiguous_name_details,
        dead_functions: dead,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FunctionInfo;

    fn func(name: &str, file_path: &str, line: usize) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}"),
            file_path: file_path.to_string(),
            start_line: line,
            end_line: line,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: vec![],
            parameters: vec![],
            return_type: None,
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    fn ctx_with(funcs: Vec<FunctionInfo>) -> (CandidateContext<'static>, Vec<FunctionInfo>) {
        let leaked: &'static Vec<FunctionInfo> = Box::leak(Box::new(funcs.clone()));
        let ctx = super::super::candidates::build_candidate_context(leaked);
        (ctx, funcs)
    }

    #[test]
    fn assemble_report_populates_totals_and_skipped() {
        let dead = vec![DeadFunction {
            info: func("foo", "src/lib.rs", 1),
            call_sites_non_test_total: 0,
            call_sites_test_only_total: 0,
        }];
        let report = assemble_report(10, dead, 1, 2, 3, 4, 5, vec![]);
        assert_eq!(report.candidates_total, 10);
        assert_eq!(report.dead_functions_total, 1);
        assert_eq!(report.skipped.reexported_total, 1);
        assert_eq!(report.skipped.cfg_test_total, 2);
        assert_eq!(report.skipped.runtime_entrypoints_total, 3);
        assert_eq!(report.skipped.ambiguous_name_total, 4);
        assert_eq!(report.skipped.framework_handler_refs_total, 5);
    }

    #[test]
    fn build_ambiguous_details_sorts_by_count_desc_then_symbol() {
        let funcs = vec![
            func("dup", "src/a.rs", 1),
            func("dup", "src/b.rs", 1),
            func("alpha", "src/c.rs", 1),
            func("alpha", "src/d.rs", 1),
        ];
        let (ctx, _funcs) = ctx_with(funcs);
        let mut counts = HashMap::new();
        counts.insert("dup".to_string(), 3usize);
        counts.insert("alpha".to_string(), 5usize);

        let details = build_ambiguous_details(&ctx, &counts);
        assert_eq!(details.len(), 2);
        assert_eq!(details[0].symbol, "alpha");
        assert_eq!(details[0].unresolved_non_test_call_sites_total, 5);
        assert_eq!(details[1].symbol, "dup");
    }

    #[test]
    fn build_ambiguous_details_includes_candidate_locations() {
        let funcs = vec![func("dup", "src/a.rs", 5), func("dup", "src/b.rs", 7)];
        let (ctx, _funcs) = ctx_with(funcs);
        let mut counts = HashMap::new();
        counts.insert("dup".to_string(), 2usize);

        let details = build_ambiguous_details(&ctx, &counts);
        let dup = &details[0];
        assert_eq!(dup.candidates_total, 2);
        assert_eq!(dup.candidate_locations.len(), 2);
        assert!(
            dup.candidate_locations
                .iter()
                .any(|loc| loc.contains("src/a.rs:5"))
        );
    }

    #[test]
    fn build_ambiguous_details_truncates_locations_above_limit() {
        let mut funcs = Vec::new();
        for i in 0..20 {
            funcs.push(func("dup", &format!("src/{i}.rs"), 1));
        }
        let (ctx, _funcs) = ctx_with(funcs);
        let mut counts = HashMap::new();
        counts.insert("dup".to_string(), 1usize);
        let details = build_ambiguous_details(&ctx, &counts);
        assert!(details[0].candidate_locations.len() <= 12);
    }

    #[test]
    fn build_ambiguous_details_returns_empty_for_no_unresolved() {
        let funcs = vec![func("foo", "src/a.rs", 1)];
        let (ctx, _funcs) = ctx_with(funcs);
        let counts = HashMap::new();
        assert!(build_ambiguous_details(&ctx, &counts).is_empty());
    }
}
