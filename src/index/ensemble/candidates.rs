use super::super::ensemble_helpers::{parse_file_line_query, parse_symbol_id_query};
use super::super::parse::ProjectAnalysis;
use super::super::{FunctionInfo, fuzzy_similarity};

/// Search for candidate functions matching `query` using exact-name, fuzzy, or
/// file:line lookup, sorted by descending score and capped at `max_results`
/// (0 = unlimited).
pub fn find_candidates<'a>(
    analysis: &'a ProjectAnalysis,
    query: &str,
    threshold: f64,
    max_results: usize,
) -> Vec<(&'a FunctionInfo, f64)> {
    find_candidates_with_options(analysis, query, threshold, max_results, false)
}


/// Like [`find_candidates`] but optionally falls back to matching against the
/// full function signature when no name-based candidate meets the threshold.
pub fn find_candidates_with_options<'a>(
    analysis: &'a ProjectAnalysis,
    query: &str,
    threshold: f64,
    max_results: usize,
    match_signature: bool,
) -> Vec<(&'a FunctionInfo, f64)> {
    let mut candidates: Vec<(&FunctionInfo, f64)> = Vec::new();

    // Symbol-id triple (`path:LINE:name`, as emitted by ambiguity errors) is
    // authoritative: when it names an existing function exactly, return only
    // that function and skip span-containment / fuzzy fallbacks.
    if let Some((path_query, line_query, name_query)) = parse_symbol_id_query(query) {
        for func in &analysis.functions {
            if func.name == name_query
                && func.start_line == line_query
                && func.file_path.ends_with(&path_query)
            {
                candidates.push((func, 1.0));
            }
        }
        if !candidates.is_empty() {
            return candidates;
        }
    }

    if let Some((path_query, line_query)) = parse_file_line_query(query) {
        for func in &analysis.functions {
            if func.file_path.ends_with(&path_query)
                && func.start_line <= line_query
                && line_query <= func.end_line
            {
                let span_len = (func.end_line.saturating_sub(func.start_line) + 1) as f64;
                let score = 1.0 / span_len;
                candidates.push((func, score));
            }
        }
    } else {
        let trimmed = query.trim();
        let exact_matches = analysis
            .functions
            .iter()
            .filter(|func| func.name == trimmed)
            .collect::<Vec<_>>();
        if !exact_matches.is_empty() {
            for func in exact_matches {
                candidates.push((func, 1.0));
            }
        } else {

            for func in &analysis.functions {
                let score = fuzzy_similarity(query, &func.name);
                if score >= threshold {
                    candidates.push((func, score));
                }
            }

            if candidates.is_empty() && match_signature {
                for func in &analysis.functions {
                    let score = fuzzy_similarity(query, &func.signature);
                    if score >= threshold {
                        candidates.push((func, score));
                    }
                }
            }
        }
    }

    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if max_results != 0 {
        candidates.truncate(max_results);
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn fn_(name: &str, file_path: &str, start: usize, end: usize) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}"),
            file_path: file_path.to_string(),
            start_line: start,
            end_line: end,
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

    fn analysis_from(functions: Vec<FunctionInfo>) -> ProjectAnalysis {
        ProjectAnalysis {
            functions,
            structs: vec![],
            enums: vec![],
            call_map: HashMap::new(),
            call_sites: vec![],
            files_analyzed: 0,
            parse_errors: vec![],
            reexports: vec![],
        }
    }

    #[test]
    fn find_candidates_returns_exact_name_match() {
        let analysis = analysis_from(vec![fn_("foo", "src/a.rs", 1, 1), fn_("bar", "src/b.rs", 1, 1)]);
        let cands = find_candidates(&analysis, "foo", 0.7, 0);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].0.name, "foo");
        assert_eq!(cands[0].1, 1.0);
    }

    #[test]
    fn find_candidates_falls_back_to_fuzzy_when_no_exact() {
        let analysis = analysis_from(vec![fn_("compute_value", "src/a.rs", 1, 1)]);
        let cands = find_candidates(&analysis, "compute_v", 0.6, 0);
        assert_eq!(cands.len(), 1);
        assert!(cands[0].1 >= 0.6);
    }

    #[test]
    fn find_candidates_respects_threshold() {
        let analysis = analysis_from(vec![fn_("compute_value", "src/a.rs", 1, 1)]);
        let cands = find_candidates(&analysis, "xyz", 0.99, 0);
        assert!(cands.is_empty());
    }

    #[test]
    fn find_candidates_resolves_file_line_query() {
        let analysis = analysis_from(vec![
            fn_("outer", "src/a.rs", 1, 100),
            fn_("inner", "src/a.rs", 10, 12),
            fn_("after", "src/a.rs", 50, 60),
        ]);
        let cands = find_candidates(&analysis, "src/a.rs:11", 0.7, 0);

        assert!(cands.len() >= 2);
        assert_eq!(cands[0].0.name, "inner");
    }

    #[test]
    fn find_candidates_returns_empty_for_unmatched_file_line() {
        let analysis = analysis_from(vec![fn_("a", "src/a.rs", 1, 5)]);
        let cands = find_candidates(&analysis, "src/a.rs:99", 0.7, 0);
        assert!(cands.is_empty());
    }

    #[test]
    fn find_candidates_truncates_results() {
        let analysis = analysis_from(vec![
            fn_("compute_a", "src/a.rs", 1, 1),
            fn_("compute_b", "src/b.rs", 1, 1),
            fn_("compute_c", "src/c.rs", 1, 1),
        ]);
        let cands = find_candidates(&analysis, "compute", 0.5, 2);
        assert_eq!(cands.len(), 2);
    }

    #[test]
    fn find_candidates_symbol_id_is_authoritative_against_homonyms() {
        let analysis = analysis_from(vec![
            fn_("run_cli", "src/daemon/cli.rs", 11, 41),
            fn_("run_cli", "src/session/cli.rs", 90, 139),
        ]);
        let cands = find_candidates(&analysis, "src/session/cli.rs:90:run_cli", 0.7, 0);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].0.file_path, "src/session/cli.rs");
        assert_eq!(cands[0].1, 1.0);
    }

    #[test]
    fn find_candidates_symbol_id_excludes_enclosing_span() {
        // span-containment would match `outer` too; the name+start_line pin must not
        let analysis = analysis_from(vec![
            fn_("outer", "src/a.rs", 1, 100),
            fn_("inner", "src/a.rs", 10, 12),
        ]);
        let cands = find_candidates(&analysis, "src/a.rs:10:inner", 0.7, 0);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].0.name, "inner");
    }

    #[test]
    fn find_candidates_stale_symbol_id_falls_back_to_span_containment() {
        let analysis = analysis_from(vec![fn_("inner", "src/a.rs", 10, 12)]);
        // line 11 is inside the span but is not the start line; name lookup misses,
        // span-containment via parse_file_line_query recovers it
        let cands = find_candidates(&analysis, "src/a.rs:11:inner", 0.7, 0);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].0.name, "inner");
    }

    #[test]
    fn find_candidates_zero_max_results_returns_all() {
        let analysis = analysis_from(vec![
            fn_("compute_a", "src/a.rs", 1, 1),
            fn_("compute_b", "src/b.rs", 1, 1),
        ]);
        let cands = find_candidates(&analysis, "compute", 0.5, 0);
        assert_eq!(cands.len(), 2);
    }
}
