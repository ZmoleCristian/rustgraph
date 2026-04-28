use std::collections::HashMap;

use super::super::ensemble_flow::is_low_signal_unresolved_callee;
use super::super::ensemble_helpers::dedup_map_values;
use super::super::parse::ProjectAnalysis;
use super::super::resolve::{build_function_module_lookup, resolve_ambiguous_call_target};
use super::super::{CallSite, FunctionInfo, function_id};

/// Resolved call-graph data derived from a single `ProjectAnalysis` pass.
///
/// All maps are keyed by stable function ID (`file:line:name`).
pub struct ResolvedCalls<'a> {
    /// All indexed functions keyed by their stable ID.
    pub function_by_id: HashMap<String, &'a FunctionInfo>,
    /// Maps a bare function name to the set of IDs that share that name.
    pub name_to_ids: HashMap<String, Vec<String>>,
    /// Module-path segments keyed by function ID, used for disambiguation.
    pub module_lookup: HashMap<String, Vec<String>>,
    /// All call-site records grouped by their caller's ID, in source order.
    pub call_sites_by_caller: HashMap<String, Vec<&'a CallSite>>,
    /// Outgoing edges whose callees were resolved to an internal function ID.
    pub resolved_outgoing: HashMap<String, Vec<String>>,
    /// Outgoing call names that could not be matched to any indexed function.
    pub unresolved_outgoing: HashMap<String, Vec<String>>,
    /// Incoming edges: maps each callee ID to the set of caller IDs.
    pub resolved_incoming: HashMap<String, Vec<String>>,
    /// Framework routing call-sites (e.g. `axum::get(handler)`) keyed by
    /// the resolved handler function ID.
    pub framework_refs_by_target: HashMap<String, Vec<&'a CallSite>>,
}

/// Build a fully resolved call-graph from `analysis`, augmented with
/// framework routing call-sites in `framework_handler_refs`.
pub fn resolve_project_calls<'a>(
    analysis: &'a ProjectAnalysis,
    framework_handler_refs: &'a [CallSite],
) -> ResolvedCalls<'a> {
    let mut function_by_id: HashMap<String, &FunctionInfo> = HashMap::new();
    let mut name_to_ids: HashMap<String, Vec<String>> = HashMap::new();
    for f in &analysis.functions {
        let id = function_id(f);
        function_by_id.insert(id.clone(), f);
        name_to_ids.entry(f.name.clone()).or_default().push(id);
    }
    let module_lookup = build_function_module_lookup(&analysis.functions);

    let mut call_sites_by_caller: HashMap<String, Vec<&CallSite>> = HashMap::new();
    for cs in analysis
        .call_sites
        .iter()
        .filter(|cs| cs.call_kind == "function")
    {
        if let Some(caller_id) = &cs.caller_id {
            call_sites_by_caller
                .entry(caller_id.clone())
                .or_default()
                .push(cs);
        }
    }
    for cs in framework_handler_refs {
        if let Some(caller_id) = &cs.caller_id {
            call_sites_by_caller
                .entry(caller_id.clone())
                .or_default()
                .push(cs);
        }
    }
    for call_sites in call_sites_by_caller.values_mut() {
        call_sites.sort_by(|a, b| a.line.cmp(&b.line).then(a.column.cmp(&b.column)));
    }

    let mut resolved_outgoing: HashMap<String, Vec<String>> = HashMap::new();
    let mut unresolved_outgoing: HashMap<String, Vec<String>> = HashMap::new();
    let mut resolved_incoming: HashMap<String, Vec<String>> = HashMap::new();
    let mut framework_refs_by_target: HashMap<String, Vec<&CallSite>> = HashMap::new();
    for cs in analysis
        .call_sites
        .iter()
        .filter(|cs| cs.call_kind == "function")
    {
        let Some(caller_id) = &cs.caller_id else {
            continue;
        };
        let Some(candidate_ids) = name_to_ids.get(&cs.callee_base) else {
            if !is_low_signal_unresolved_callee(&cs.callee_base) {
                unresolved_outgoing
                    .entry(caller_id.clone())
                    .or_default()
                    .push(cs.callee.clone());
            }
            continue;
        };

        let target_id = if candidate_ids.len() == 1 {
            Some(candidate_ids[0].clone())
        } else {
            resolve_ambiguous_call_target(cs, candidate_ids, &function_by_id, &module_lookup)
        };

        if let Some(target_id) = target_id {
            resolved_outgoing
                .entry(caller_id.clone())
                .or_default()
                .push(target_id.clone());
            resolved_incoming
                .entry(target_id.clone())
                .or_default()
                .push(caller_id.clone());
        } else if !is_low_signal_unresolved_callee(&cs.callee_base) {
            unresolved_outgoing
                .entry(caller_id.clone())
                .or_default()
                .push(cs.callee.clone());
        }
    }
    for cs in framework_handler_refs {
        let Some(caller_id) = &cs.caller_id else {
            continue;
        };
        let Some(candidate_ids) = name_to_ids.get(&cs.callee_base) else {
            if !is_low_signal_unresolved_callee(&cs.callee_base) {
                unresolved_outgoing
                    .entry(caller_id.clone())
                    .or_default()
                    .push(cs.callee.clone());
            }
            continue;
        };
        let target_id = if candidate_ids.len() == 1 {
            Some(candidate_ids[0].clone())
        } else {
            resolve_ambiguous_call_target(cs, candidate_ids, &function_by_id, &module_lookup)
        };

        if let Some(target_id) = target_id {
            resolved_outgoing
                .entry(caller_id.clone())
                .or_default()
                .push(target_id.clone());
            resolved_incoming
                .entry(target_id.clone())
                .or_default()
                .push(caller_id.clone());
            framework_refs_by_target
                .entry(target_id)
                .or_default()
                .push(cs);
        } else if !is_low_signal_unresolved_callee(&cs.callee_base) {
            unresolved_outgoing
                .entry(caller_id.clone())
                .or_default()
                .push(cs.callee.clone());
        }
    }
    dedup_map_values(&mut resolved_outgoing);
    dedup_map_values(&mut unresolved_outgoing);
    dedup_map_values(&mut resolved_incoming);

    ResolvedCalls {
        function_by_id,
        name_to_ids,
        module_lookup,
        call_sites_by_caller,
        resolved_outgoing,
        unresolved_outgoing,
        resolved_incoming,
        framework_refs_by_target,
    }
}
