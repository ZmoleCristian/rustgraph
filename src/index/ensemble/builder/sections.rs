use std::collections::{HashMap, HashSet};
use std::fs;

use crate::index::ensemble_flow::{build_value_flow_hints, classify_io_boundaries_for_function};
use crate::index::ensemble_helpers::{
    collect_related_functions, collect_used_structs, extract_code_snippet,
};
use crate::index::ensemble_lifecycle::{TypeLifecycleContext, build_type_lifecycle};
use crate::index::parse::ProjectAnalysis;
use crate::index::types::{
    CallSite, CallSiteWithLine, CodeSnippet, FunctionInfo, FunctionNeighborhood, IoBoundarySignal,
    StructEnsemble, StructInfo, TypeLifecycle, ValueFlowHint,
};

pub fn build_code_snippet(
    func: &FunctionInfo,
    section_flags_code: bool,
) -> Result<CodeSnippet, Box<dyn std::error::Error>> {
    if section_flags_code {
        extract_code_snippet(&func.file_path, func.start_line, func.end_line)
    } else {
        Ok(CodeSnippet {
            file_path: func.file_path.clone(),
            start_line: func.start_line,
            end_line: func.end_line,
            lines: Vec::new(),
        })
    }
}

pub fn build_struct_ensembles(
    func: &FunctionInfo,
    section_flags_structs: bool,
    section_flags_lifecycles: bool,
    explicit_lifecycle_types_is_empty: bool,
    known_structs: &HashSet<String>,
    structs_by_name: &HashMap<String, Vec<&StructInfo>>,
) -> Result<(Vec<String>, Vec<StructEnsemble>), Box<dyn std::error::Error>> {
    let should_collect_struct_names =
        section_flags_structs || (section_flags_lifecycles && explicit_lifecycle_types_is_empty);

    let mut used_struct_names: Vec<String> = if should_collect_struct_names {
        collect_used_structs(func, known_structs)
            .unwrap_or_default()
            .into_iter()
            .collect()
    } else {
        Vec::new()
    };
    used_struct_names.sort();

    let mut struct_ensembles = Vec::new();
    if section_flags_structs {
        for name in &used_struct_names {
            if let Some(infos) = structs_by_name.get(name) {
                for info in infos {
                    let code =
                        extract_code_snippet(&info.file_path, info.start_line, info.end_line)?;
                    struct_ensembles.push(StructEnsemble {
                        info: (*info).clone(),
                        code,
                    });
                }
            }
        }
    }

    struct_ensembles.sort_by(|a, b| {
        a.info
            .file_path
            .cmp(&b.info.file_path)
            .then(a.info.start_line.cmp(&b.info.start_line))
    });

    Ok((used_struct_names, struct_ensembles))
}

use crate::index::resolve_read_path;


fn path_match_score(file_path: &str, qualifier_segs: &[&str]) -> usize {
    let path_segs: Vec<&str> = file_path
        .trim_start_matches("./")
        .trim_end_matches(".rs")
        .split('/')
        .filter(|s| !s.is_empty() && !matches!(*s, "src" | "lib" | "main" | "mod"))
        .collect();
    qualifier_segs
        .iter()
        .filter(|q| path_segs.contains(q))
        .count()
}


fn file_path_to_module_segs(file_path: &str) -> Vec<&str> {
    file_path
        .trim_start_matches("./")
        .trim_end_matches(".rs")
        .split('/')
        .filter(|s| !s.is_empty() && !matches!(*s, "src" | "lib" | "main" | "mod"))
        .collect()
}


#[allow(clippy::too_many_arguments)]
fn reexport_chain_matches(
    func_name: &str,
    qualifier_segs: &[&str],
    target_path: &str,
    other_candidate_paths: &[String],
    reexports: &[(String, String, String)],
) -> bool {
    let target_segs = file_path_to_module_segs(target_path);
    for (containing_module, alias_local, target_full_path) in reexports {
        if alias_local != func_name {
            continue;
        }


        let cm_segs: Vec<&str> = containing_module.split("::").filter(|s| !s.is_empty()).collect();
        if cm_segs.is_empty() {


            let stripped: Vec<&str> = qualifier_segs
                .iter()
                .copied()
                .filter(|s| *s != "crate" && *s != "self" && *s != "super")
                .collect();
            if !stripped.is_empty() {
                continue;
            }
        } else {


            let q_stripped: Vec<&str> = qualifier_segs
                .iter()
                .copied()
                .skip_while(|s| matches!(*s, "crate" | "self" | "super"))
                .collect();
            if q_stripped.len() < cm_segs.len() {
                continue;
            }
            let tail = &q_stripped[q_stripped.len() - cm_segs.len()..];
            if tail != cm_segs.as_slice() {
                continue;
            }
        }


        let tfp_segs: Vec<&str> = target_full_path.split("::").filter(|s| !s.is_empty()).collect();
        if tfp_segs.is_empty() {
            continue;
        }

        let mut tfp_qualifier: Vec<&str> = tfp_segs[..tfp_segs.len().saturating_sub(1)]
            .iter()
            .copied()
            .skip_while(|s| matches!(*s, "crate" | "self" | "super"))
            .collect();


        if tfp_qualifier.is_empty() {

            let our = path_match_score(target_path, &target_segs);
            let max_other = other_candidate_paths
                .iter()
                .map(|p| {
                    let p_segs = file_path_to_module_segs(p);
                    path_match_score(p, &p_segs)
                })
                .max()
                .unwrap_or(0);
            if our >= max_other {
                return true;
            }
            continue;
        }

        let our_score = path_match_score(target_path, &tfp_qualifier);
        let max_other_score = other_candidate_paths
            .iter()
            .map(|p| path_match_score(p, &tfp_qualifier))
            .max()
            .unwrap_or(0);
        if our_score >= max_other_score && our_score > 0 {
            return true;
        }


        let target_segs_sliced = &target_segs[..];
        if target_segs_sliced.len() >= tfp_qualifier.len() {
            let tail = &target_segs_sliced[target_segs_sliced.len() - tfp_qualifier.len()..];
            if tail == tfp_qualifier.as_slice() {
                return true;
            }
        }


        if let Some(last_q) = tfp_qualifier.pop()
            && let Some(last_t) = target_segs.last()
            && last_q == *last_t
        {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
pub fn build_call_site_views<'a>(
    func_name: &str,
    root_id: &str,
    root_kind: &str,
    section_flags_call_sites: bool,
    max_call_sites: usize,
    analysis: &'a ProjectAnalysis,
    framework_refs_by_target: &HashMap<String, Vec<&'a CallSite>>,
    file_cache: &mut HashMap<String, Vec<String>>,


    resolved_caller_ids: Option<&std::collections::HashSet<String>>,
    resolver_claimed_elsewhere: &std::collections::HashSet<String>,


    other_candidate_paths: &[String],
    target_path: &str,


    name_collision: bool,


    has_method_homonym: bool,
) -> (usize, bool, Vec<CallSiteWithLine>) {
    if !section_flags_call_sites {
        return (0, false, Vec::new());
    }


    let owner: Option<&str> = root_kind
        .strip_prefix("method(")
        .and_then(|s| s.strip_suffix(')'));


    let candidate_is_free_fn = root_kind == "function";

    let mut call_sites: Vec<&CallSite> = analysis
        .call_sites
        .iter()
        .filter(|cs| {
            if cs.callee_base != func_name {
                return false;
            }


            if candidate_is_free_fn && has_method_homonym && cs.call_kind == "method" {
                return false;
            }

            if name_collision {
                if let Some(caller_id) = cs.caller_id.as_ref() {


                    let we_were_picked = resolved_caller_ids
                        .map(|s| s.contains(caller_id))
                        .unwrap_or(false);
                    if !we_were_picked && resolver_claimed_elsewhere.contains(caller_id) {
                        return false;
                    }
                }


                if cs.callee != cs.callee_base {
                    let qualifier_segs: Vec<&str> = cs
                        .callee
                        .rsplit_once("::")
                        .map(|(q, _)| q.split("::").collect())
                        .unwrap_or_default();
                    if !qualifier_segs.is_empty() {


                        let we_match_via_reexport = reexport_chain_matches(
                            func_name,
                            &qualifier_segs,
                            target_path,
                            other_candidate_paths,
                            &analysis.reexports,
                        );
                        if we_match_via_reexport {
                            return true;
                        }


                        let other_matches_via_reexport =
                            other_candidate_paths.iter().any(|other_path| {
                                reexport_chain_matches(
                                    func_name,
                                    &qualifier_segs,
                                    other_path,


                                    &other_candidate_paths
                                        .iter()
                                        .filter(|p| p.as_str() != other_path.as_str())
                                        .cloned()
                                        .chain(std::iter::once(target_path.to_string()))
                                        .collect::<Vec<_>>(),
                                    &analysis.reexports,
                                )
                            });
                        if other_matches_via_reexport {
                            return false;
                        }
                        let our_score = path_match_score(target_path, &qualifier_segs);
                        let max_other = other_candidate_paths
                            .iter()
                            .map(|p| path_match_score(p, &qualifier_segs))
                            .max()
                            .unwrap_or(0);
                        if max_other > our_score {
                            return false;
                        }
                    }
                }
            }


            if let Some(owner) = owner {
                if cs.callee != cs.callee_base {
                    let qualifier = cs
                        .callee
                        .rsplit_once("::")
                        .map(|(q, _)| q)
                        .unwrap_or("");
                    let last = qualifier.rsplit("::").next().unwrap_or(qualifier);


                    let qualifier_is_type =
                        last.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                    if qualifier_is_type && last != owner {
                        return false;
                    }
                }
            }
            true
        })
        .collect();
    if let Some(framework_refs) = framework_refs_by_target.get(root_id) {
        call_sites.extend(framework_refs.iter().copied());
    }

    call_sites.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line.cmp(&b.line))
            .then(a.column.cmp(&b.column))
    });

    let total_call_sites = call_sites.len();
    let (call_sites_truncated, call_sites_iter): (bool, Box<dyn Iterator<Item = &&CallSite>>) =
        if max_call_sites == 0 || total_call_sites <= max_call_sites {
            (false, Box::new(call_sites.iter()))
        } else {
            (true, Box::new(call_sites.iter().take(max_call_sites)))
        };

    let mut views = Vec::new();
    for cs in call_sites_iter {
        if !file_cache.contains_key(&cs.file_path) {


            let resolved = resolve_read_path(&cs.file_path);
            let content = fs::read_to_string(&resolved).unwrap_or_default();
            let lines = content.lines().map(|l| l.to_string()).collect::<Vec<_>>();
            file_cache.insert(cs.file_path.clone(), lines);
        }

        let line_text = file_cache
            .get(&cs.file_path)
            .and_then(|lines| lines.get(cs.line.saturating_sub(1)))
            .cloned()
            .unwrap_or_default();

        views.push(CallSiteWithLine {
            caller_id: cs.caller_id.clone(),
            caller_name: cs.caller_name.clone(),
            callee: cs.callee.clone(),
            callee_base: cs.callee_base.clone(),
            file_path: cs.file_path.clone(),
            line: cs.line,
            column: cs.column,
            line_text,
        });
    }

    (total_call_sites, call_sites_truncated, views)
}

#[allow(clippy::too_many_arguments)]
pub fn build_neighborhood(
    root_id: &str,
    section_flags_neighborhood: bool,
    call_depth: usize,
    max_related: usize,
    resolved_incoming: &HashMap<String, Vec<String>>,
    resolved_outgoing: &HashMap<String, Vec<String>>,
    function_by_id: &HashMap<String, &FunctionInfo>,
    unresolved_outgoing: &HashMap<String, Vec<String>>,
) -> FunctionNeighborhood {
    if !section_flags_neighborhood {
        return FunctionNeighborhood {
            call_depth,
            max_related,
            upstream_total: 0,
            upstream_truncated: false,
            upstream: Vec::new(),
            downstream_total: 0,
            downstream_truncated: false,
            downstream: Vec::new(),
            unresolved_outgoing_calls: Vec::new(),
        };
    }

    let (upstream_total, upstream_truncated, upstream) = collect_related_functions(
        root_id,
        resolved_incoming,
        function_by_id,
        call_depth,
        max_related,
    );
    let (downstream_total, downstream_truncated, downstream) = collect_related_functions(
        root_id,
        resolved_outgoing,
        function_by_id,
        call_depth,
        max_related,
    );

    FunctionNeighborhood {
        call_depth,
        max_related,
        upstream_total,
        upstream_truncated,
        upstream,
        downstream_total,
        downstream_truncated,
        downstream,
        unresolved_outgoing_calls: unresolved_outgoing
            .get(root_id)
            .cloned()
            .unwrap_or_default(),
    }
}

pub fn build_lifecycles(
    section_flags_lifecycles: bool,
    lifecycle_queries: &[String],
    lifecycle_context: &TypeLifecycleContext,
    max_related: usize,
    max_lifecycle_paths: usize,
    explicit_lifecycle_types_is_empty: bool,
    lifecycle_max_functions: usize,
) -> Vec<TypeLifecycle> {
    if !section_flags_lifecycles {
        return Vec::new();
    }

    let mut out = lifecycle_queries
        .iter()
        .map(|type_name| {
            build_type_lifecycle(
                type_name,
                lifecycle_context,
                max_related,
                max_lifecycle_paths,
            )
        })
        .collect::<Vec<_>>();

    if explicit_lifecycle_types_is_empty && lifecycle_max_functions != 0 {
        out.retain(|lc| lc.functions_total <= lifecycle_max_functions);
    }
    out
}

pub fn build_dataflow_hints(
    func: &FunctionInfo,
    root_id: &str,
    section_flags_dataflow: bool,
    max_related: usize,
    call_sites_by_caller: &HashMap<String, Vec<&CallSite>>,
    file_cache: &mut HashMap<String, Vec<String>>,
) -> Vec<ValueFlowHint> {
    if !section_flags_dataflow {
        return Vec::new();
    }

    let caller_sites = call_sites_by_caller
        .get(root_id)
        .cloned()
        .unwrap_or_default();
    build_value_flow_hints(func, &caller_sites, file_cache, max_related)
}

#[allow(clippy::too_many_arguments)]
pub fn build_io_boundaries(
    root_id: &str,
    section_flags_boundaries: bool,
    call_depth: usize,
    max_related: usize,
    resolved_incoming: &HashMap<String, Vec<String>>,
    resolved_outgoing: &HashMap<String, Vec<String>>,
    function_by_id: &HashMap<String, &FunctionInfo>,
    unresolved_outgoing: &HashMap<String, Vec<String>>,
) -> Vec<IoBoundarySignal> {
    if !section_flags_boundaries {
        return Vec::new();
    }

    let (_, _, up_all) =
        collect_related_functions(root_id, resolved_incoming, function_by_id, call_depth, 0);
    let (_, _, down_all) =
        collect_related_functions(root_id, resolved_outgoing, function_by_id, call_depth, 0);

    let mut boundary_ids: HashSet<String> = HashSet::new();
    boundary_ids.insert(root_id.to_string());
    for r in up_all {
        boundary_ids.insert(r.id);
    }
    for r in down_all {
        boundary_ids.insert(r.id);
    }

    let mut signals = Vec::new();
    let mut ids = boundary_ids.into_iter().collect::<Vec<_>>();
    ids.sort();
    for id in ids {
        if let Some(target_func) = function_by_id.get(&id) {
            let mut outgoing_names = resolved_outgoing
                .get(&id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|tid| function_by_id.get(&tid).map(|f| f.name.clone()))
                .collect::<Vec<_>>();
            outgoing_names.extend(unresolved_outgoing.get(&id).cloned().unwrap_or_default());
            signals.extend(classify_io_boundaries_for_function(
                &id,
                target_func,
                &outgoing_names,
            ));
        }
    }
    if max_related != 0 && signals.len() > max_related {
        signals.truncate(max_related);
    }
    signals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn func(name: &str, file_path: &str, start: usize, end: usize) -> FunctionInfo {
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

    #[test]
    fn build_code_snippet_returns_empty_lines_when_disabled() {
        let f = func("foo", "src/lib.rs", 1, 5);
        let snip = build_code_snippet(&f, false).expect("snip");
        assert!(snip.lines.is_empty());
        assert_eq!(snip.start_line, 1);
        assert_eq!(snip.end_line, 5);
    }

    #[test]
    fn build_struct_ensembles_returns_empty_when_both_flags_false() {
        let f = func("foo", "src/lib.rs", 1, 1);
        let known = HashSet::new();
        let by_name: HashMap<String, Vec<&StructInfo>> = HashMap::new();
        let (names, ensembles) =
            build_struct_ensembles(&f, false, false, true, &known, &by_name).expect("ok");
        assert!(names.is_empty());
        assert!(ensembles.is_empty());
    }

    #[test]
    fn build_neighborhood_returns_empty_when_disabled() {
        let function_by_id = HashMap::new();
        let nbh = build_neighborhood(
            "id",
            false,
            5,
            10,
            &HashMap::new(),
            &HashMap::new(),
            &function_by_id,
            &HashMap::new(),
        );
        assert_eq!(nbh.upstream_total, 0);
        assert_eq!(nbh.downstream_total, 0);
        assert!(nbh.upstream.is_empty());
        assert!(nbh.downstream.is_empty());
        assert_eq!(nbh.call_depth, 5);
        assert_eq!(nbh.max_related, 10);
    }

    #[test]
    fn build_neighborhood_collects_unresolved_outgoing_calls() {
        let function_by_id = HashMap::new();
        let mut unresolved = HashMap::new();
        unresolved.insert(
            "root".to_string(),
            vec!["external_fn".to_string()],
        );
        let nbh = build_neighborhood(
            "root",
            true,
            1,
            10,
            &HashMap::new(),
            &HashMap::new(),
            &function_by_id,
            &unresolved,
        );
        assert_eq!(nbh.unresolved_outgoing_calls, vec!["external_fn".to_string()]);
    }

    #[test]
    fn build_lifecycles_returns_empty_when_disabled() {
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
        let lc = build_lifecycles(false, &["Foo".to_string()], &ctx, 5, 5, true, 0);
        assert!(lc.is_empty());
    }

    #[test]
    fn build_dataflow_hints_returns_empty_when_disabled() {
        let f = func("foo", "src/lib.rs", 1, 1);
        let mut cache = HashMap::new();
        let hints = build_dataflow_hints(
            &f,
            "id",
            false,
            10,
            &HashMap::new(),
            &mut cache,
        );
        assert!(hints.is_empty());
    }

    #[test]
    fn build_io_boundaries_returns_empty_when_disabled() {
        let function_by_id = HashMap::new();
        let signals = build_io_boundaries(
            "id",
            false,
            5,
            10,
            &HashMap::new(),
            &HashMap::new(),
            &function_by_id,
            &HashMap::new(),
        );
        assert!(signals.is_empty());
    }

    #[test]
    fn build_call_site_views_returns_empty_when_disabled() {
        let analysis = ProjectAnalysis {
            functions: vec![],
            structs: vec![],
            enums: vec![],
            call_map: HashMap::new(),
            call_sites: vec![],
            files_analyzed: 0,
            parse_errors: vec![],
            reexports: vec![],
        };
        let mut cache = HashMap::new();
        let (total, truncated, views) = build_call_site_views(
            "foo",
            "id",
            "function",
            false,
            10,
            &analysis,
            &HashMap::new(),
            &mut cache,
            None,
            &std::collections::HashSet::new(),
            &[],
            "src/dummy.rs",
            false,
            false,
        );
        assert_eq!(total, 0);
        assert!(!truncated);
        assert!(views.is_empty());
    }
}
