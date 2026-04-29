//! Thin wrappers that translate high-level preset/view parameters into the
//! lower-level ensemble, callers, and dead-code builders, adding structured
//! tracing around each call.

use crate::api::{EnsemblePreset, EnsembleView};
use crate::app::callers::{CallersReport, build_callers_report};
use crate::dead_code::{DeadCodeReport, build_dead_code_report};
use crate::project::ProjectData;
use crate::{FunctionEnsembleOptions, FunctionEnsembleResult, build_function_ensemble};
use std::path::Path;
use std::time::Instant;
use tracing::info;

/// Build a function ensemble for `query` against `project`.
///
/// The `preset` controls result depth and breadth; `view` selects which
/// ensemble sections are included. `lifecycle_types` is a `|`-separated list
/// of type names to scope the lifecycle section to.
pub fn build_ensemble(
    project: &ProjectData,
    query: &str,
    threshold: f64,
    preset: Option<EnsemblePreset>,
    view: Option<EnsembleView>,
    lifecycle_types: Option<&str>,
) -> Result<FunctionEnsembleResult, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let lifecycle_types = lifecycle_types.map(|raw| {
        raw.split('|')
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>()
    });
    let (max_results, max_call_sites, call_depth, max_related, max_paths, lifecycle_max_funcs) =
        match preset.unwrap_or(EnsemblePreset::Balanced) {
            EnsemblePreset::Quick => (3, 80, 1, 25, 8, 80),
            EnsemblePreset::Balanced => (5, 200, 2, 60, 20, 120),
            EnsemblePreset::Deep => (10, 0, 3, 120, 40, 200),
        };

    let sections = match view.unwrap_or(EnsembleView::Summary) {
        EnsembleView::Summary => vec!["code".to_string(), "structs".to_string()],
        EnsembleView::Usage => vec!["call-sites".to_string(), "neighborhood".to_string()],
        EnsembleView::Flow => vec![
            "neighborhood".to_string(),
            "lifecycle".to_string(),
            "dataflow".to_string(),
            "boundaries".to_string(),
        ],
        EnsembleView::Full => vec!["all".to_string()],
    };

    let result = build_function_ensemble(
        &project.to_analysis(),
        query,
        threshold,
        FunctionEnsembleOptions {
            max_results,
            max_call_sites,
            call_depth,
            max_related,
            max_lifecycle_paths: max_paths,
            lifecycle_max_functions: lifecycle_max_funcs,
            lifecycle_types: lifecycle_types.as_deref(),
            ensemble_sections: Some(&sections),
            match_signature: false,
        },
    );
    if let Ok(ref ensemble) = result {
        info!(
            target: "query.ensemble",
            query,
            threshold,
            functions = project.functions.len(),
            structs = project.structs.len(),
            enums = project.enums.len(),
            matches = ensemble.matches.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "built ensemble"
        );
    }
    result
}

/// Build a callers report for all functions in `project` matching `query`.
///
/// `before_context` and `after_context` control how many surrounding source
/// lines are included with each call site.
pub fn build_callers(
    project: &ProjectData,
    query: &str,
    threshold: f64,
    before_context: usize,
    after_context: usize,
) -> Result<CallersReport, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let result = build_callers_report(project, query, threshold, before_context, after_context);
    if let Ok(ref report) = result {
        info!(
            target: "query.callers",
            query,
            threshold,
            matches = report.matches.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "built callers report"
        );
    }
    result
}

/// Build a dead-code report for `project`, anchored at `workspace_root`.
///
/// Functions that are never referenced by any call site within the project are
/// reported as dead. `workspace_root` is used to exclude non-project files.
pub fn build_dead_code(
    project: &ProjectData,
    workspace_root: &Path,
) -> Result<DeadCodeReport, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let result = build_dead_code_report(
        &project.functions,
        &project.call_sites,
        &project.rust_files,
        workspace_root,
        Some(project),
    );
    if let Ok(ref report) = result {
        info!(
            target: "query.dead_code",
            rust_files = project.rust_files.len(),
            functions = project.functions.len(),
            dead_functions = report.dead_functions.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "built dead-code report"
        );
    }
    result
}
