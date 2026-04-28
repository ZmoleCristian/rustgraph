//! Core callers-report construction: resolves a query via the ensemble pipeline,
//! then groups resulting call sites by caller function and attaches source context.

use super::super::project::ProjectData;
use crate::{FunctionEnsembleOptions, build_function_ensemble, function_id};
use std::collections::{BTreeMap, HashMap};

use super::context::context_lines_for_call;
use super::types::{CallerFunction, CallerSite, CallersMatch, CallersReport};

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
        let mut unresolved_call_sites = 0usize;

        for call_site in ensemble_match.call_sites {
            let Some(caller_id) = call_site.caller_id else {
                unresolved_call_sites += 1;
                continue;
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
            let Some(caller) = function_by_id.get(&caller_id) else {
                unresolved_call_sites += bucket.sites.len();
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

        matches.push(CallersMatch {
            info: ensemble_match.info,
            call_sites_total: ensemble_match.call_sites_total,
            unresolved_call_sites,
            callers,
        });
    }

    Ok(CallersReport {
        query: query.to_string(),
        matches,
    })
}
