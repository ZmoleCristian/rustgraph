use std::collections::{HashMap, HashSet};

use crate::index::ensemble::resolution::resolve_project_calls;
use crate::index::ensemble_flow::collect_framework_handler_refs;
use crate::index::ensemble_helpers::parse_ensemble_sections;
use crate::index::ensemble_lifecycle::TypeLifecycleContext;
use crate::index::parse::{ProjectAnalysis, function_id};
use crate::index::types::{FunctionEnsemble, FunctionEnsembleResult, StructInfo};

use crate::index::ensemble::candidates::find_candidates_with_options;
use crate::index::ensemble::options::FunctionEnsembleOptions;

use crate::index::ensemble::builder::sections::{
    build_call_site_views, build_code_snippet, build_dataflow_hints, build_io_boundaries,
    build_lifecycles, build_neighborhood, build_struct_ensembles,
};

/// Build a [`FunctionEnsembleResult`] for every function in `analysis` that
/// matches `query` at the given fuzzy `threshold`.
///
/// `options` controls per-section limits and which sections are populated.
pub fn build_function_ensemble(
    analysis: &ProjectAnalysis,
    query: &str,
    threshold: f64,
    options: FunctionEnsembleOptions<'_>,
) -> Result<FunctionEnsembleResult, Box<dyn std::error::Error>> {
    let FunctionEnsembleOptions {
        max_results,
        max_call_sites,
        call_depth,
        max_related,
        max_lifecycle_paths,
        lifecycle_max_functions,
        lifecycle_types,
        ensemble_sections,
        match_signature,
    } = options;

    let known_structs: HashSet<String> = analysis.structs.iter().map(|s| s.name.clone()).collect();
    let mut structs_by_name: HashMap<String, Vec<&StructInfo>> = HashMap::new();
    for s in &analysis.structs {
        structs_by_name.entry(s.name.clone()).or_default().push(s);
    }

    let candidates =
        find_candidates_with_options(analysis, query, threshold, max_results, match_signature);
    let (sections, section_flags) = parse_ensemble_sections(ensemble_sections);

    let mut file_cache: HashMap<String, Vec<String>> = HashMap::new();
    let framework_handler_refs =
        collect_framework_handler_refs(&analysis.call_sites, &mut file_cache);

    let resolved = resolve_project_calls(analysis, &framework_handler_refs);

    let lifecycle_context = TypeLifecycleContext {
        function_by_id: &resolved.function_by_id,
        resolved_outgoing: &resolved.resolved_outgoing,
        call_sites_by_caller: &resolved.call_sites_by_caller,
        name_to_ids: &resolved.name_to_ids,
        module_lookup: &resolved.module_lookup,
    };

    let explicit_lifecycle_types: Vec<String> = lifecycle_types
        .map(|types: &[String]| {
            types
                .iter()
                .map(|t: &String| t.trim())
                .filter(|t: &&str| !t.is_empty())
                .map(|t: &str| t.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    let mut matches = Vec::new();

    let mut callers_by_target: std::collections::HashMap<&str, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    for (caller_id, targets) in &resolved.resolved_outgoing {
        for tid in targets {
            callers_by_target
                .entry(tid.as_str())
                .or_default()
                .insert(caller_id.clone());
        }
    }

    for (func, _score) in candidates {
        let root_id = function_id(func);
        let func_code = build_code_snippet(func, section_flags.code)?;

        let (used_struct_names, struct_ensembles) = build_struct_ensembles(
            func,
            section_flags.structs,
            section_flags.lifecycles,
            explicit_lifecycle_types.is_empty(),
            &known_structs,
            &structs_by_name,
        )?;

        let project_homonym_count = resolved
            .name_to_ids
            .get(func.name.as_str())
            .map(|ids| ids.len())
            .unwrap_or(0);
        let collision = project_homonym_count > 1;

        let has_method_homonym = collision
            && resolved
                .name_to_ids
                .get(func.name.as_str())
                .map(|ids| {
                    ids.iter().any(|id| {
                        resolved.function_by_id.get(id).is_some_and(|f| {
                            f.kind.starts_with("method(") || f.kind.starts_with("trait_fn(")
                        })
                    })
                })
                .unwrap_or(false);
        let resolved_for_target = callers_by_target.get(root_id.as_str());
        let mut claimed_elsewhere: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut other_paths: Vec<String> = Vec::new();
        if collision {
            for (other_id, other_func) in &resolved.function_by_id {
                if other_id.as_str() == root_id.as_str() {
                    continue;
                }
                if other_func.name == func.name {
                    other_paths.push(other_func.file_path.clone());
                    if let Some(callers) = callers_by_target.get(other_id.as_str()) {
                        claimed_elsewhere.extend(callers.iter().cloned());
                    }
                }
            }
        }
        let (total_call_sites, call_sites_truncated, call_site_views) = build_call_site_views(
            &func.name,
            &root_id,
            &func.kind,
            section_flags.call_sites,
            max_call_sites,
            analysis,
            &resolved.framework_refs_by_target,
            &mut file_cache,
            resolved_for_target,
            &claimed_elsewhere,
            &other_paths,
            &func.file_path,
            collision,
            has_method_homonym,
        );

        let neighborhood = build_neighborhood(
            &root_id,
            section_flags.neighborhood,
            call_depth,
            max_related,
            &resolved.resolved_incoming,
            &resolved.resolved_outgoing,
            &resolved.function_by_id,
            &resolved.unresolved_outgoing,
        );

        let mut lifecycle_queries = if explicit_lifecycle_types.is_empty() {
            used_struct_names.clone()
        } else {
            explicit_lifecycle_types.clone()
        };
        lifecycle_queries.sort();
        lifecycle_queries.dedup();

        let lifecycles = build_lifecycles(
            section_flags.lifecycles,
            &lifecycle_queries,
            &lifecycle_context,
            max_related,
            max_lifecycle_paths,
            explicit_lifecycle_types.is_empty(),
            lifecycle_max_functions,
        );

        let dataflow_hints = build_dataflow_hints(
            func,
            &root_id,
            section_flags.dataflow,
            max_related,
            &resolved.call_sites_by_caller,
            &mut file_cache,
        );

        let io_boundaries = build_io_boundaries(
            &root_id,
            section_flags.boundaries,
            call_depth,
            max_related,
            &resolved.resolved_incoming,
            &resolved.resolved_outgoing,
            &resolved.function_by_id,
            &resolved.unresolved_outgoing,
        );

        matches.push(FunctionEnsemble {
            info: func.clone(),
            code: func_code,
            structs_used: struct_ensembles,
            call_sites_total: total_call_sites,
            call_sites_truncated,
            call_sites: call_site_views,
            neighborhood,
            lifecycles,
            dataflow_hints,
            io_boundaries,
        });
    }

    Ok(FunctionEnsembleResult {
        query: query.to_string(),
        threshold,
        max_results,
        max_call_sites,
        call_depth,
        max_related,
        max_lifecycle_paths,
        lifecycle_max_functions,
        lifecycle_types: explicit_lifecycle_types,
        sections,
        matches,
    })
}
