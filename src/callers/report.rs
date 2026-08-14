//! Core callers-report construction: resolves a query via the ensemble pipeline,
//! then groups resulting call sites by caller function and attaches source context.

use crate::project::ProjectData;
use crate::{FunctionEnsembleOptions, build_function_ensemble, function_id};
use std::collections::{BTreeMap, HashMap};

use super::context::context_lines_for_call;
use super::types::{CallerFunction, CallerSite, CallersMatch, CallersReport, UnresolvedCallSite};

#[derive(Default)]
struct CallerBucket {
    sites: Vec<CallerSite>,
}

/// Builds a [`CallersReport`] with no source context lines and default path roots.
///
/// Convenience wrapper around [`build_callers_report_with_options`].
pub fn build_callers_report(
    project: &ProjectData,
    query: &str,
    threshold: f64,
    before_context: usize,
    after_context: usize,
) -> Result<CallersReport, Box<dyn std::error::Error>> {
    build_callers_report_with_options(
        project,
        query,
        threshold,
        before_context,
        after_context,
        false,
        None,
        &[],
    )
}

/// Builds a [`CallersReport`] by running the ensemble pipeline on `query`, grouping
/// every call site by its caller function ID, optionally attaching `before_context` /
/// `after_context` source lines read from `primary_root` or `also_roots`.
///
/// Call sites whose caller ID cannot be resolved against `project.functions` land in
/// `unresolved_call_site_locations`, tagged `module-level` or `unmapped`.
pub fn build_callers_report_with_options(
    project: &ProjectData,
    query: &str,
    threshold: f64,
    before_context: usize,
    after_context: usize,
    match_signature: bool,
    primary_root: Option<&std::path::Path>,
    also_roots: &[std::path::PathBuf],
) -> Result<CallersReport, Box<dyn std::error::Error>> {
    let analysis = project.to_analysis();
    let sections = vec!["call-sites".to_string()];
    let options = || FunctionEnsembleOptions {
        max_results: 0,
        max_call_sites: 0,
        call_depth: 1,
        max_related: 0,
        max_lifecycle_paths: 0,
        lifecycle_max_functions: 0,
        lifecycle_types: None,
        ensemble_sections: Some(&sections),
        match_signature,
    };
    let mut ensemble = build_function_ensemble(&analysis, query, threshold, options())?;

    // A query naming a re-export alias (`pub use operator::add as add_operator`)
    // matches no function, because the function's own name is the original —
    // the alias exists only in the use tree. When nothing matched the query
    // exactly and the reexport index maps it to a differently-named target,
    // retry under the original name, narrowed to the module the re-export
    // points into so a common name like `add` does not fan out project-wide.
    let exact = ensemble.matches.iter().any(|m| m.info.name == query);
    if !exact && let Some((target_name, module_hint)) = reexport_target(&analysis.reexports, query)
    {
        let mut retry = build_function_ensemble(&analysis, &target_name, threshold, options())?;
        retry.matches.retain(|m| {
            m.info.name == target_name
                && match &module_hint {
                    Some(hint) => file_in_module(&m.info.file_path, hint),
                    None => true,
                }
        });
        if !retry.matches.is_empty() {
            ensemble = retry;
        }
    }

    let function_by_id = project
        .functions
        .iter()
        .map(|function| (function_id(function), function))
        .collect::<HashMap<_, _>>();
    let mut file_lines_cache = HashMap::<String, Vec<String>>::new();

    let mut matches = Vec::new();

    for ensemble_match in ensemble.matches {
        let mut grouped = BTreeMap::<String, CallerBucket>::new();
        let mut unresolved_call_site_locations: Vec<UnresolvedCallSite> = Vec::new();

        for call_site in ensemble_match.call_sites {
            let caller_id = match call_site.caller_id.as_deref() {
                Some(id) if function_by_id.contains_key(id) => id.to_string(),
                Some(_) | None => {
                    let kind = if call_site.caller_id.is_some() {
                        "unmapped"
                    } else {
                        "module-level"
                    };
                    unresolved_call_site_locations.push(UnresolvedCallSite {
                        file_path: call_site.file_path,
                        line: call_site.line,
                        column: call_site.column,
                        line_text: call_site.line_text,
                        caller_name: call_site.caller_name,
                        caller_kind: kind,
                    });
                    continue;
                }
            };

            grouped
                .entry(caller_id)
                .or_default()
                .sites
                .push(CallerSite {
                    line: call_site.line,
                    column: call_site.column,
                    line_text: call_site.line_text,
                    context_lines: context_lines_for_call(
                        &call_site.file_path,
                        call_site.line,
                        before_context,
                        after_context,
                        &mut file_lines_cache,
                        primary_root,
                        also_roots,
                    ),
                });
        }

        let mut callers = Vec::new();

        for (caller_id, mut bucket) in grouped {
            let caller = function_by_id
                .get(&caller_id)
                .expect("caller_id presence checked above");

            bucket.sites.sort_by(|left, right| {
                left.line
                    .cmp(&right.line)
                    .then(left.column.cmp(&right.column))
            });
            bucket
                .sites
                .dedup_by(|left, right| left.line == right.line && left.column == right.column);

            callers.push(CallerFunction {
                info: (*caller).clone(),
                call_sites_total: bucket.sites.len(),
                call_sites: bucket.sites,
            });
        }

        callers.sort_by(|left, right| {
            left.info
                .file_path
                .cmp(&right.info.file_path)
                .then(left.info.start_line.cmp(&right.info.start_line))
                .then(left.info.name.cmp(&right.info.name))
        });

        unresolved_call_site_locations.sort_by(|left, right| {
            left.file_path
                .cmp(&right.file_path)
                .then(left.line.cmp(&right.line))
                .then(left.column.cmp(&right.column))
        });
        unresolved_call_site_locations.dedup_by(|left, right| {
            left.file_path == right.file_path
                && left.line == right.line
                && left.column == right.column
        });

        matches.push(CallersMatch {
            info: ensemble_match.info,
            call_sites_total: ensemble_match.call_sites_total,
            unresolved_call_site_locations,
            callers,
        });
    }

    Ok(CallersReport {
        query: query.to_string(),
        matches,
    })
}

/// Resolve `query` through the re-export index: returns the target's own name
/// and the innermost real module segment of its path (skipping `crate`, `self`
/// and `super`) when some `pub use path::original as query` renames it.
///
/// A plain re-export (name unchanged) returns `None` — the normal name match
/// already covers it.
fn reexport_target(
    reexports: &[(String, String, String)],
    query: &str,
) -> Option<(String, Option<String>)> {
    for (_module, exported, target) in reexports {
        if exported != query {
            continue;
        }
        let segments: Vec<&str> = target.split("::").collect();
        let (last, prefix) = segments.split_last()?;
        if *last == query {
            continue;
        }
        let hint = prefix
            .iter()
            .rev()
            .find(|seg| !matches!(**seg, "crate" | "self" | "super"))
            .map(|seg| (*seg).to_string());
        return Some(((*last).to_string(), hint));
    }
    None
}

fn file_in_module(file_path: &str, module: &str) -> bool {
    file_path.ends_with(&format!("/{module}.rs"))
        || file_path.contains(&format!("/{module}/"))
        || file_path.ends_with(&format!("{module}.rs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reexport_target_resolves_renamed_alias_with_module_hint() {
        let reexports = vec![(
            "store".to_string(),
            "add_operator".to_string(),
            "operator::add".to_string(),
        )];
        let resolved = reexport_target(&reexports, "add_operator").expect("alias resolves");
        assert_eq!(resolved.0, "add");
        assert_eq!(resolved.1.as_deref(), Some("operator"));
    }

    #[test]
    fn reexport_target_skips_plain_reexports_and_crate_prefix() {
        let reexports = vec![
            (
                "session".to_string(),
                "target".to_string(),
                "crate::shared::target".to_string(),
            ),
            (
                "api".to_string(),
                "renamed".to_string(),
                "crate::inner".to_string(),
            ),
        ];
        assert!(reexport_target(&reexports, "target").is_none());
        let resolved = reexport_target(&reexports, "renamed").expect("renamed resolves");
        assert_eq!(resolved.0, "inner");
        assert_eq!(resolved.1, None);
    }

    #[test]
    fn file_in_module_matches_file_and_directory_layouts() {
        assert!(file_in_module("./src/store/operator.rs", "operator"));
        assert!(file_in_module("./src/store/operator/mod.rs", "operator"));
        assert!(!file_in_module("./src/store/org.rs", "operator"));
    }
}
