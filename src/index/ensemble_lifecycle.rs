//! Type lifecycle analysis: classifies functions as entrypoints, transit nodes,
//! endpoints, or isolated with respect to a named type, and samples data-flow
//! paths through the call graph.

use super::ensemble_helpers::{
    dedup_map_values, function_consumes_type, function_mentions_type, function_produces_type,
    lifecycle_hop_from_function, normalize_type_query, related_from_function, sort_related,
};
use super::resolve::resolve_ambiguous_call_target;
use super::{CallSite, FunctionInfo, LifecyclePath, RelatedFunction, TypeLifecycle};
use std::collections::{HashMap, HashSet};

/// Borrowed context bundles the index tables needed by [`build_type_lifecycle`].
pub(super) struct TypeLifecycleContext<'a> {
    /// All known functions keyed by their stable ID.
    pub(super) function_by_id: &'a HashMap<String, &'a FunctionInfo>,
    /// Resolved outgoing call edges (caller-id → [callee-id]).
    pub(super) resolved_outgoing: &'a HashMap<String, Vec<String>>,
    /// All call sites grouped by caller ID.
    pub(super) call_sites_by_caller: &'a HashMap<String, Vec<&'a CallSite>>,
    /// Function name → list of all stable IDs with that name.
    pub(super) name_to_ids: &'a HashMap<String, Vec<String>>,
    /// Stable ID → module path segments (used for ambiguity resolution).
    pub(super) module_lookup: &'a HashMap<String, Vec<String>>,
}

struct LifecyclePathSearch<'a> {
    outgoing: &'a HashMap<String, Vec<String>>,
    end_set: &'a HashSet<String>,
    function_by_id: &'a HashMap<String, &'a FunctionInfo>,
    max_depth: usize,
    max_paths: usize,
}

impl<'a> LifecyclePathSearch<'a> {
    fn dfs(
        &self,
        node: &str,
        current: &mut Vec<String>,
        paths: &mut Vec<LifecyclePath>,
        truncated: &mut bool,
    ) {
        if paths.len() >= self.max_paths {
            *truncated = true;
            return;
        }

        current.push(node.to_string());

        if self.end_set.contains(node) && current.len() > 1 {
            let hops = current
                .iter()
                .filter_map(|id| {
                    self.function_by_id
                        .get(id)
                        .map(|func| lifecycle_hop_from_function(func, id))
                })
                .collect::<Vec<_>>();
            paths.push(LifecyclePath { hops });
            if paths.len() >= self.max_paths {
                *truncated = true;
            }
        }

        if !*truncated
            && current.len() < self.max_depth
            && let Some(next_nodes) = self.outgoing.get(node)
        {
            for next in next_nodes {
                if current.iter().any(|id| id == next) {
                    continue;
                }
                self.dfs(next, current, paths, truncated);
                if *truncated {
                    break;
                }
            }
        }

        current.pop();
    }
}

/// Build a complete [`TypeLifecycle`] for the named type using the provided index context.
///
/// `max_related` caps the number of functions shown per role; `max_paths` caps the
/// number of sampled data-flow paths.  Returns an empty lifecycle when the type
/// token normalises to the empty string or when no functions mention the type.
pub(super) fn build_type_lifecycle(
    type_name: &str,
    context: &TypeLifecycleContext<'_>,
    max_related: usize,
    max_paths: usize,
) -> TypeLifecycle {
    let function_by_id = context.function_by_id;
    let resolved_outgoing = context.resolved_outgoing;
    let call_sites_by_caller = context.call_sites_by_caller;
    let name_to_ids = context.name_to_ids;
    let module_lookup = context.module_lookup;
    let type_token = normalize_type_query(type_name);
    if type_token.is_empty() {
        return TypeLifecycle {
            type_name: type_name.to_string(),
            functions_total: 0,
            entrypoints_total: 0,
            transit_total: 0,
            endpoints_total: 0,
            isolated_total: 0,
            functions_truncated: false,
            entrypoints: Vec::new(),
            transit: Vec::new(),
            endpoints: Vec::new(),
            isolated: Vec::new(),
            sample_paths: Vec::new(),
            paths_truncated: false,
        };
    }

    let mentioning_ids: HashSet<String> = function_by_id
        .iter()
        .filter_map(|(id, func)| {
            if function_mentions_type(func, &type_token) {
                Some(id.clone())
            } else {
                None
            }
        })
        .collect();

    if mentioning_ids.is_empty() {
        return TypeLifecycle {
            type_name: type_name.to_string(),
            functions_total: 0,
            entrypoints_total: 0,
            transit_total: 0,
            endpoints_total: 0,
            isolated_total: 0,
            functions_truncated: false,
            entrypoints: Vec::new(),
            transit: Vec::new(),
            endpoints: Vec::new(),
            isolated: Vec::new(),
            sample_paths: Vec::new(),
            paths_truncated: false,
        };
    }

    let producers: HashSet<String> = mentioning_ids
        .iter()
        .filter_map(|id| {
            function_by_id.get(id).and_then(|func| {
                if function_produces_type(func, &type_token) {
                    Some(id.clone())
                } else {
                    None
                }
            })
        })
        .collect();

    let consumers: HashSet<String> = mentioning_ids
        .iter()
        .filter_map(|id| {
            function_by_id.get(id).and_then(|func| {
                if function_consumes_type(func, &type_token) {
                    Some(id.clone())
                } else {
                    None
                }
            })
        })
        .collect();

    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();

    for (caller_id, callee_ids) in resolved_outgoing {
        if !mentioning_ids.contains(caller_id) {
            continue;
        }
        for callee_id in callee_ids {
            if mentioning_ids.contains(callee_id) && caller_id != callee_id {
                edges
                    .entry(caller_id.clone())
                    .or_default()
                    .insert(callee_id.clone());
            }
        }
    }

    for call_sites in call_sites_by_caller.values() {
        let mut resolved_calls: Vec<(usize, usize, String)> = Vec::new();

        for cs in call_sites {
            let Some(candidate_ids) = name_to_ids.get(&cs.callee_base) else {
                continue;
            };
            let target_id = if candidate_ids.len() == 1 {
                Some(candidate_ids[0].clone())
            } else {
                resolve_ambiguous_call_target(cs, candidate_ids, function_by_id, module_lookup)
            };

            if let Some(target_id) = target_id
                && mentioning_ids.contains(&target_id)
            {
                resolved_calls.push((cs.line, cs.column, target_id));
            }
        }

        resolved_calls.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));

        for i in 0..resolved_calls.len() {
            let from_id = &resolved_calls[i].2;
            if !producers.contains(from_id) {
                continue;
            }

            for (_, _, to_id) in resolved_calls.iter().skip(i + 1) {
                if from_id != to_id && consumers.contains(to_id) {
                    edges
                        .entry(from_id.clone())
                        .or_default()
                        .insert(to_id.clone());
                }
            }
        }
    }

    let mut outgoing: HashMap<String, Vec<String>> = HashMap::new();
    let mut incoming: HashMap<String, Vec<String>> = HashMap::new();
    for (from, to_set) in edges {
        let mut targets: Vec<String> = to_set.into_iter().collect();
        targets.sort();
        targets.dedup();
        for to in &targets {
            incoming.entry(to.clone()).or_default().push(from.clone());
        }
        outgoing.insert(from, targets);
    }
    dedup_map_values(&mut incoming);

    let mut entry_ids = Vec::new();
    let mut transit_ids = Vec::new();
    let mut endpoint_ids = Vec::new();
    let mut isolated_ids = Vec::new();

    for id in &mentioning_ids {
        let in_degree = incoming.get(id).map_or(0, |v| v.len());
        let out_degree = outgoing.get(id).map_or(0, |v| v.len());
        match (in_degree, out_degree) {
            (0, 0) => isolated_ids.push(id.clone()),
            (0, _) => entry_ids.push(id.clone()),
            (_, 0) => endpoint_ids.push(id.clone()),
            _ => transit_ids.push(id.clone()),
        }
    }

    let to_related = |ids: &[String]| -> Vec<RelatedFunction> {
        let mut out = ids
            .iter()
            .filter_map(|id| {
                function_by_id
                    .get(id)
                    .map(|func| related_from_function(func, id, 0))
            })
            .collect::<Vec<_>>();
        sort_related(&mut out);
        out
    };

    let mut entrypoints = to_related(&entry_ids);
    let mut transit = to_related(&transit_ids);
    let mut endpoints = to_related(&endpoint_ids);
    let mut isolated = to_related(&isolated_ids);

    let entrypoints_total = entrypoints.len();
    let transit_total = transit.len();
    let endpoints_total = endpoints.len();
    let isolated_total = isolated.len();

    if max_related != 0 {
        if entrypoints.len() > max_related {
            entrypoints.truncate(max_related);
        }
        if transit.len() > max_related {
            transit.truncate(max_related);
        }
        if endpoints.len() > max_related {
            endpoints.truncate(max_related);
        }
        if isolated.len() > max_related {
            isolated.truncate(max_related);
        }
    }

    let functions_total = mentioning_ids.len();
    let functions_shown = entrypoints.len() + transit.len() + endpoints.len() + isolated.len();
    let functions_truncated = functions_shown < functions_total;

    let mut sample_paths: Vec<LifecyclePath> = Vec::new();
    let mut paths_truncated = false;

    if max_paths > 0 {
        let mut start_ids = if entry_ids.is_empty() {
            mentioning_ids.iter().cloned().collect::<Vec<_>>()
        } else {
            entry_ids.clone()
        };
        start_ids.sort();
        start_ids.dedup();

        let end_set: HashSet<String> = if endpoint_ids.is_empty() {
            mentioning_ids.clone()
        } else {
            endpoint_ids.iter().cloned().collect()
        };

        let path_search = LifecyclePathSearch {
            outgoing: &outgoing,
            end_set: &end_set,
            function_by_id,
            max_depth: 14,
            max_paths,
        };

        let mut current = Vec::new();
        for start in start_ids {
            path_search.dfs(
                &start,
                &mut current,
                &mut sample_paths,
                &mut paths_truncated,
            );
            if paths_truncated {
                break;
            }
        }

        if sample_paths.is_empty() {
            let mut isolated_sorted = isolated_ids;
            isolated_sorted.sort();
            for id in isolated_sorted.into_iter().take(max_paths) {
                if let Some(func) = function_by_id.get(&id) {
                    sample_paths.push(LifecyclePath {
                        hops: vec![lifecycle_hop_from_function(func, &id)],
                    });
                }
            }
        }
    }

    TypeLifecycle {
        type_name: type_name.to_string(),
        functions_total,
        entrypoints_total,
        transit_total,
        endpoints_total,
        isolated_total,
        functions_truncated,
        entrypoints,
        transit,
        endpoints,
        isolated,
        sample_paths,
        paths_truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_type_lifecycle_returns_empty_struct_for_empty_token() {
        let function_by_id = HashMap::new();
        let resolved_outgoing = HashMap::new();
        let call_sites_by_caller = HashMap::new();
        let name_to_ids = HashMap::new();
        let module_lookup = HashMap::new();

        let ctx = TypeLifecycleContext {
            function_by_id: &function_by_id,
            resolved_outgoing: &resolved_outgoing,
            call_sites_by_caller: &call_sites_by_caller,
            name_to_ids: &name_to_ids,
            module_lookup: &module_lookup,
        };

        let lc = build_type_lifecycle("   ", &ctx, 5, 5);
        assert_eq!(lc.functions_total, 0);
        assert!(lc.entrypoints.is_empty());
        assert!(lc.transit.is_empty());
        assert!(lc.endpoints.is_empty());
        assert!(lc.isolated.is_empty());
    }

    #[test]
    fn build_type_lifecycle_returns_empty_when_no_functions_mention_type() {
        let function_by_id = HashMap::new();
        let resolved_outgoing = HashMap::new();
        let call_sites_by_caller = HashMap::new();
        let name_to_ids = HashMap::new();
        let module_lookup = HashMap::new();

        let ctx = TypeLifecycleContext {
            function_by_id: &function_by_id,
            resolved_outgoing: &resolved_outgoing,
            call_sites_by_caller: &call_sites_by_caller,
            name_to_ids: &name_to_ids,
            module_lookup: &module_lookup,
        };
        let lc = build_type_lifecycle("Foo", &ctx, 5, 5);
        assert_eq!(lc.functions_total, 0);
        assert_eq!(lc.type_name, "Foo");
    }

    #[test]
    fn build_type_lifecycle_classifies_isolated_function() {
        let f = FunctionInfo {
            name: "loner".to_string(),
            signature: "fn loner(x: Foo) -> Foo".to_string(),
            file_path: "src/a.rs".to_string(),
            start_line: 1,
            end_line: 1,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: vec![],
            parameters: vec!["x: Foo".to_string()],
            return_type: Some("Foo".to_string()),
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        };
        let id = "src/a.rs:1:loner".to_string();
        let mut function_by_id = HashMap::new();
        function_by_id.insert(id.clone(), &f);
        let resolved_outgoing = HashMap::new();
        let call_sites_by_caller = HashMap::new();
        let name_to_ids = HashMap::new();
        let module_lookup = HashMap::new();

        let ctx = TypeLifecycleContext {
            function_by_id: &function_by_id,
            resolved_outgoing: &resolved_outgoing,
            call_sites_by_caller: &call_sites_by_caller,
            name_to_ids: &name_to_ids,
            module_lookup: &module_lookup,
        };
        let lc = build_type_lifecycle("Foo", &ctx, 5, 5);
        assert_eq!(lc.functions_total, 1);
        assert_eq!(lc.isolated_total, 1);
        assert_eq!(lc.isolated[0].name, "loner");
    }
}
