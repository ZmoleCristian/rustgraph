//! Core callers-report construction: resolves a query via the ensemble pipeline,
//! then groups resulting call sites by caller function and attaches source context.

use super::super::project::ProjectData;
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
/// Call sites whose caller ID cannot be resolved against `project.functions` are counted
/// in `unresolved_call_sites`.
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
    let ensemble = build_function_ensemble(
        &analysis,
        query,
        threshold,
        FunctionEnsembleOptions {
            max_results: 0,
            max_call_sites: 0,
            call_depth: 1,
            max_related: 0,
            max_lifecycle_paths: 0,
            lifecycle_max_functions: 0,
            lifecycle_types: None,
            ensemble_sections: Some(&sections),
            match_signature,
        },
    )?;

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
            let caller_id = match call_site.caller_id.clone() {
                Some(id) if function_by_id.contains_key(&id) => id,
                Some(id) => {
                    // The visitor recorded an enclosing function for this call,
                    // but that id is not present in the project's function index.
                    // Surface the location anyway so the user can navigate.
                    let _unmapped_id = id;
                    unresolved_call_site_locations.push(UnresolvedCallSite {
                        file_path: call_site.file_path.clone(),
                        line: call_site.line,
                        column: call_site.column,
                        line_text: call_site.line_text.clone(),
                        caller_name: call_site.caller_name.clone(),
                        caller_kind: "unmapped",
                    });
                    continue;
                }
                None => {
                    unresolved_call_site_locations.push(UnresolvedCallSite {
                        file_path: call_site.file_path.clone(),
                        line: call_site.line,
                        column: call_site.column,
                        line_text: call_site.line_text.clone(),
                        caller_name: call_site.caller_name.clone(),
                        caller_kind: "module-level",
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
                    line_text: call_site.line_text.clone(),
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
            let Some(caller) = function_by_id.get(&caller_id) else {
                // Should not happen any more thanks to the upfront
                // `function_by_id.contains_key` check above, but defend
                // against future refactors that bypass it.
                let _stale_caller_id = caller_id;
                continue;
            };

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
        unresolved_call_site_locations
            .dedup_by(|left, right| {
                left.file_path == right.file_path
                    && left.line == right.line
                    && left.column == right.column
            });

        matches.push(CallersMatch {
            info: ensemble_match.info,
            call_sites_total: ensemble_match.call_sites_total,
            unresolved_call_sites: unresolved_call_site_locations.len(),
            unresolved_call_site_locations,
            callers,
        });
    }

    Ok(CallersReport {
        query: query.to_string(),
        matches,
    })
}
