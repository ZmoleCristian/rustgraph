use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;

use serde::Serialize;

use super::super::modes::CallGraphConfig;
use super::super::project::ProjectData;
use super::switchboard::write_string_output;
use crate::api::CallGraphDetail;
use crate::cli::Args;
use crate::index::qualifier::{
    ResolutionStats, candidate_matches_qualifier, qualifier_for_callee, resolve_callee_candidates,
};
use crate::{CallSite, FunctionInfo, function_id, generate_call_graph};

/// Run `rustgraph call-graph [--root <FN>] [--depth N] [--dot] [--show-external]`.
///
/// Produces a DOT or text call graph for the project or a subgraph rooted at a specific function.
/// When `--root` is given the graph is scoped by BFS to `depth` call levels; `--include-callers`
/// extends the same BFS upward. Default output is the indented text tree; `--dot` switches to
/// Graphviz DOT, and `--json` emits the structured JSON envelope.
pub fn run(
    args: &Args,
    project: &ProjectData,
    detail: CallGraphDetail,
    config: CallGraphConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let strict_only = args.strict_resolution;
    let mut resolution_stats = ResolutionStats::default();

    let (functions_view, call_map_view, call_sites_view, scope_note) = if let Some(root_query) =
        config.root.as_deref()
    {
        scope_to_root(
            project,
            root_query,
            config.depth,
            config.include_callers,
            strict_only,
            &mut resolution_stats,
        )?
    } else if let Some(search_terms) = args.search.as_deref() {
        scope_to_search(
            project,
            search_terms,
            args.search_threshold,
            args.match_signature,
            config.depth,
            config.include_callers,
            strict_only,
            &mut resolution_stats,
        )?
    } else {
        let n = project.functions.len();

        if n > 50 && !config.all {
            return Err(format!(
                    "call-graph would render {} functions; pass --root <FN>, --search <Q>, or --all (no-cap dump) to proceed",
                    n
                )
                .into());
        }
        if !config.text && n > 50 && args.output.is_none() {
            eprintln!(
                "warning: call-graph --dot --all will emit a {} -node DOT graph (likely unrenderable). \
                     Pass --root <fn> to focus, or `-o graph.dot` to dump silently.",
                n
            );
        }
        (
            project.functions.clone(),
            project.call_map.clone(),
            project.call_sites.clone(),
            None,
        )
    };

    if let Some(note) = &scope_note {
        eprintln!("{}", note);
    }

    if args.json {
        let payload = render_json(
            config.root.as_deref(),
            config.depth,
            config.include_callers,
            config.show_external,
            &functions_view,
            &call_map_view,
            strict_only,
            &mut resolution_stats,
        )?;
        write_string_output(args.output.as_deref(), &payload)?;

        resolution_stats.emit_stderr_note();
        return Ok(());
    }

    let payload = if config.text {
        let (rendered, all_external_hint) = render_text(
            config.root.as_deref(),
            &functions_view,
            &call_map_view,
            config.show_external,
            config.include_callers,
            strict_only,
            &mut resolution_stats,
        );

        if all_external_hint {
            eprintln!(
                "note: all callees of '{}' are external (std lib, other crates, iterator chains); pass --show-external to see them",
                config.root.as_deref().unwrap_or("<root>")
            );
        }
        rendered
    } else {
        generate_call_graph(
            &args.path,
            &functions_view,
            &call_map_view,
            &call_sites_view,
            detail.as_str(),
        )?
    };

    if let Some(output_path) = args.output.as_ref() {
        fs::write(output_path, &payload)?;
        if config.text {
            println!("Call graph (text) written to: {}", output_path.display());
        } else {
            println!("Call graph written to: {}", output_path.display());
        }
    } else {
        println!("{}", payload);
    }

    resolution_stats.emit_stderr_note();

    Ok(())
}

#[derive(Serialize)]
struct CallGraphJson<'a> {
    scope: ScopeJson<'a>,
    functions: Vec<FunctionJson<'a>>,
    edges: Vec<EdgeJson<'a>>,
}

#[derive(Serialize)]
struct ScopeJson<'a> {
    root: Option<&'a str>,

    depth: i64,
    include_callers: bool,
    show_external: bool,
    kept_function_count: usize,
    internal_edge_count: usize,
    external_edge_count: usize,
}

#[derive(Serialize)]
struct FunctionJson<'a> {
    id: String,
    name: &'a str,
    file_path: &'a str,
    start_line: usize,
    end_line: usize,
}

#[derive(Serialize)]
struct EdgeJson<'a> {
    caller_id: &'a str,

    callee_id: Option<String>,

    callee_kind: &'static str,

    callee_name: &'a str,
}

#[allow(clippy::too_many_arguments)]
fn render_json(
    root_query: Option<&str>,
    depth: usize,
    include_callers: bool,
    show_external: bool,
    functions: &[FunctionInfo],
    call_map: &HashMap<String, Vec<String>>,
    strict_only: bool,
    stats: &mut ResolutionStats,
) -> Result<String, Box<dyn std::error::Error>> {
    let by_id: HashMap<String, &FunctionInfo> =
        functions.iter().map(|f| (function_id(f), f)).collect();
    let by_name: HashMap<&str, Vec<&FunctionInfo>> = {
        let mut m: HashMap<&str, Vec<&FunctionInfo>> = HashMap::new();
        for f in functions {
            m.entry(f.name.as_str()).or_default().push(f);
        }
        m
    };

    let mut caller_ids: Vec<&String> = call_map.keys().collect();
    caller_ids.sort();

    let mut edges: Vec<EdgeJson> = Vec::new();
    let mut internal_count = 0usize;
    let mut external_count = 0usize;

    for caller_id in caller_ids {
        if !by_id.contains_key(caller_id) {
            continue;
        }
        let Some(callees) = call_map.get(caller_id) else {
            continue;
        };
        for callee_name in callees {
            let bare = callee_name
                .rsplit(|c: char| c == '.' || c == ':')
                .next()
                .unwrap_or(callee_name.as_str());

            let (candidates, mode) =
                resolve_callee_candidates(callee_name, bare, &by_name, strict_only);
            stats.record(mode);
            let resolved: Option<String> = candidates.iter().find_map(|m| {
                let cid = function_id(m);
                if by_id.contains_key(&cid) {
                    Some(cid)
                } else {
                    None
                }
            });
            if let Some(cid) = resolved {
                internal_count += 1;
                edges.push(EdgeJson {
                    caller_id,
                    callee_id: Some(cid),
                    callee_kind: "internal",
                    callee_name,
                });
            } else {
                external_count += 1;
                if show_external {
                    edges.push(EdgeJson {
                        caller_id,
                        callee_id: None,
                        callee_kind: "external",
                        callee_name,
                    });
                }
            }
        }
    }

    let mut function_records: Vec<FunctionJson> = functions
        .iter()
        .map(|f| FunctionJson {
            id: function_id(f),
            name: f.name.as_str(),
            file_path: f.file_path.as_str(),
            start_line: f.start_line,
            end_line: f.end_line,
        })
        .collect();

    function_records.sort_by(|a, b| a.id.cmp(&b.id));

    let payload = CallGraphJson {
        scope: ScopeJson {
            root: root_query,

            depth: if depth == 0 { -1 } else { depth as i64 },
            include_callers,
            show_external,
            kept_function_count: functions.len(),
            internal_edge_count: internal_count,
            external_edge_count: external_count,
        },
        functions: function_records,
        edges,
    };

    Ok(serde_json::to_string_pretty(&payload)?)
}

#[allow(clippy::too_many_arguments)]
fn render_text(
    root_query: Option<&str>,
    functions: &[FunctionInfo],
    call_map: &HashMap<String, Vec<String>>,
    show_external: bool,
    include_callers: bool,
    strict_only: bool,
    stats: &mut ResolutionStats,
) -> (String, bool) {
    let by_id: HashMap<String, &FunctionInfo> =
        functions.iter().map(|f| (function_id(f), f)).collect();
    let by_name: HashMap<&str, Vec<&FunctionInfo>> = {
        let mut m: HashMap<&str, Vec<&FunctionInfo>> = HashMap::new();
        for f in functions {
            m.entry(f.name.as_str()).or_default().push(f);
        }
        m
    };

    let (internal_edges, external_edges) =
        count_edges(call_map, &by_id, &by_name, strict_only, stats);
    let displayed_edges = if show_external {
        internal_edges + external_edges
    } else {
        internal_edges
    };
    let all_external_hint =
        root_query.is_some() && !show_external && internal_edges == 0 && external_edges > 0;

    let mut out = String::new();
    let header_suffix = if all_external_hint {
        "  (all external — pass --show-external to see)"
    } else {
        ""
    };
    out.push_str(&format!(
        "rustgraph call-graph --text  [{} fn(s), {} call edge(s)]{}\n",
        functions.len(),
        displayed_edges,
        header_suffix
    ));

    if let Some(query) = root_query {
        let roots: Vec<&FunctionInfo> = functions
            .iter()
            .filter(|f| {
                f.name == query
                    || query
                        .rsplit_once(':')
                        .and_then(|(p, l)| l.parse::<usize>().ok().map(|line| (p, line)))
                        .is_some_and(|(p, line)| f.start_line == line && f.file_path.contains(p))
            })
            .collect();

        if include_callers {
            let mut rev: HashMap<&str, Vec<&str>> = HashMap::new();
            for (caller_id, callees) in call_map {
                for callee_name in callees {
                    let bare = callee_name
                        .rsplit(|c: char| c == '.' || c == ':')
                        .next()
                        .unwrap_or(callee_name.as_str());
                    rev.entry(bare).or_default().push(caller_id.as_str());
                }
            }
            for r in &roots {
                let mut up_visited: HashSet<String> = HashSet::new();
                let r_id = function_id(r);
                up_visited.insert(r_id.clone());
                let mut callers: Vec<&FunctionInfo> = rev
                    .get(r.name.as_str())
                    .map(|cids| {
                        cids.iter()
                            .filter_map(|cid| by_id.get(*cid).copied())
                            .filter(|f| caller_resolves(f, r, call_map))
                            .collect()
                    })
                    .unwrap_or_default();
                callers.sort_by(|a, b| {
                    a.file_path
                        .cmp(&b.file_path)
                        .then(a.start_line.cmp(&b.start_line))
                });
                callers.dedup_by(|a, b| function_id(a) == function_id(b));
                let total_callers = callers.len();
                if total_callers > 0 {
                    out.push('\n');
                    out.push_str(&format!(
                        "Callers of {} ({}:{}) [{}]:\n",
                        r.name, r.file_path, r.start_line, total_callers
                    ));
                    for (i, caller) in callers.iter().enumerate() {
                        let last = i + 1 == total_callers;
                        walk_text_callers(
                            caller,
                            &by_id,
                            &rev,
                            call_map,
                            "",
                            last,
                            &mut up_visited,
                            &mut out,
                        );
                    }
                }
            }
        }

        for r in &roots {
            out.push('\n');
            let mut visited: HashSet<String> = HashSet::new();
            walk_text(
                r,
                &by_id,
                &by_name,
                call_map,
                "",
                true,
                true,
                &mut visited,
                &mut out,
                show_external,
                strict_only,
                stats,
            );
        }
    } else {
        let mut per_file: HashMap<&str, Vec<&FunctionInfo>> = HashMap::new();
        for f in functions {
            per_file.entry(f.file_path.as_str()).or_default().push(f);
        }
        let mut files: Vec<String> = per_file.keys().map(|s| s.to_string()).collect();
        files.sort();
        for file in &files {
            let mut fns = per_file.remove(file.as_str()).unwrap_or_default();
            fns.sort_by_key(|f| f.start_line);
            out.push_str(&format!("\n{}:\n", file));
            for f in fns {
                let id = function_id(f);
                let callees = call_map.get(&id).map(|v| v.join(", ")).unwrap_or_default();
                if callees.is_empty() {
                    continue;
                }
                out.push_str(&format!(
                    "  fn {} @ {}  →  {}\n",
                    f.name, f.start_line, callees
                ));
            }
        }
    }

    (out, all_external_hint)
}

fn count_edges(
    call_map: &HashMap<String, Vec<String>>,
    by_id: &HashMap<String, &FunctionInfo>,
    by_name: &HashMap<&str, Vec<&FunctionInfo>>,
    strict_only: bool,
    stats: &mut ResolutionStats,
) -> (usize, usize) {
    let mut internal = 0usize;
    let mut external = 0usize;
    for callees in call_map.values() {
        for callee_name in callees {
            let bare = callee_name
                .rsplit(|c: char| c == '.' || c == ':')
                .next()
                .unwrap_or(callee_name.as_str());

            let (candidates, mode) =
                resolve_callee_candidates(callee_name, bare, by_name, strict_only);
            stats.record(mode);
            let resolves_internal = candidates
                .iter()
                .any(|m| by_id.contains_key(&function_id(m)));
            if resolves_internal {
                internal += 1;
            } else {
                external += 1;
            }
        }
    }
    (internal, external)
}

#[allow(clippy::too_many_arguments)]
fn walk_text(
    node: &FunctionInfo,
    by_id: &HashMap<String, &FunctionInfo>,
    by_name: &HashMap<&str, Vec<&FunctionInfo>>,
    call_map: &HashMap<String, Vec<String>>,
    prefix: &str,
    is_last: bool,
    is_root: bool,
    visited: &mut HashSet<String>,
    out: &mut String,
    show_external: bool,
    strict_only: bool,
    stats: &mut ResolutionStats,
) {
    let id = function_id(node);

    if !visited.insert(id.clone()) {
        return;
    }
    let connector = if is_root {
        ""
    } else if is_last {
        "└── "
    } else {
        "├── "
    };
    out.push_str(&format!(
        "{}{}{} ({}:{})\n",
        prefix, connector, node.name, node.file_path, node.start_line
    ));
    let next_prefix = if is_root {
        prefix.to_string()
    } else {
        format!("{}{}", prefix, if is_last { "    " } else { "│   " })
    };
    let Some(callees) = call_map.get(&id) else {
        return;
    };
    let mut child_fns: Vec<&FunctionInfo> = Vec::new();
    let mut external: Vec<String> = Vec::new();
    let mut seen_child_ids: HashSet<String> = HashSet::new();
    for callee_name in callees {
        let bare = callee_name
            .rsplit(|c: char| c == '.' || c == ':')
            .next()
            .unwrap_or(callee_name.as_str());

        let (filtered, mode) = resolve_callee_candidates(callee_name, bare, by_name, strict_only);
        stats.record(mode);
        if !filtered.is_empty() {
            for m in &filtered {
                let cid = function_id(m);
                if by_id.contains_key(&cid) && seen_child_ids.insert(cid.clone()) {
                    child_fns.push(*m);
                }
            }
        } else {
            external.push(callee_name.clone());
        }
    }
    let render_external = show_external && !external.is_empty();
    let total = child_fns.len() + if render_external { 1 } else { 0 };
    for (i, child) in child_fns.iter().enumerate() {
        let last = i + 1 == total;
        walk_text(
            child,
            by_id,
            by_name,
            call_map,
            &next_prefix,
            last,
            false,
            visited,
            out,
            show_external,
            strict_only,
            stats,
        );
    }
    if render_external {
        out.push_str(&format!(
            "{}└── <external> {}\n",
            next_prefix,
            external.join(", ")
        ));
    }
}

fn caller_resolves(
    caller: &FunctionInfo,
    target: &FunctionInfo,
    call_map: &HashMap<String, Vec<String>>,
) -> bool {
    let cid = function_id(caller);
    let Some(callees) = call_map.get(&cid) else {
        return false;
    };
    for callee_name in callees {
        let bare = callee_name
            .rsplit(|c: char| c == '.' || c == ':')
            .next()
            .unwrap_or(callee_name.as_str());
        if bare != target.name {
            continue;
        }
        let qualifier = qualifier_for_callee(callee_name);
        if candidate_matches_qualifier(target, qualifier.as_deref()) {
            return true;
        }
    }
    false
}

fn walk_text_callers(
    node: &FunctionInfo,
    by_id: &HashMap<String, &FunctionInfo>,
    rev: &HashMap<&str, Vec<&str>>,
    call_map: &HashMap<String, Vec<String>>,
    prefix: &str,
    is_last: bool,
    visited: &mut HashSet<String>,
    out: &mut String,
) {
    let id = function_id(node);
    if !visited.insert(id.clone()) {
        return;
    }
    let connector = if is_last { "└── " } else { "├── " };
    out.push_str(&format!(
        "{}{}{} ({}:{})\n",
        prefix, connector, node.name, node.file_path, node.start_line
    ));
    let next_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
    let mut parents: Vec<&FunctionInfo> = rev
        .get(node.name.as_str())
        .map(|cids| {
            cids.iter()
                .filter_map(|cid| by_id.get(*cid).copied())
                .filter(|f| caller_resolves(f, node, call_map))
                .collect()
        })
        .unwrap_or_default();
    parents.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.start_line.cmp(&b.start_line))
    });
    parents.dedup_by(|a, b| function_id(a) == function_id(b));
    let total = parents.len();
    for (i, p) in parents.iter().enumerate() {
        let last = i + 1 == total;
        walk_text_callers(p, by_id, rev, call_map, &next_prefix, last, visited, out);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]

fn scope_to_search(
    project: &ProjectData,
    search_terms: &str,
    threshold: f64,
    match_signature: bool,
    depth: usize,
    include_callers: bool,
    strict_only: bool,
    stats: &mut ResolutionStats,
) -> Result<
    (
        Vec<FunctionInfo>,
        HashMap<String, Vec<String>>,
        Vec<CallSite>,
        Option<String>,
    ),
    Box<dyn std::error::Error>,
> {
    let (matched_fns, _, _, _, _) = crate::search_items_with_options(
        &project.functions,
        &project.structs,
        &project.enums,
        &project.consts,
        &project.type_decls,
        search_terms,
        threshold,
        match_signature,
    );
    if matched_fns.is_empty() {
        return Err(format!(
            "no function matches --search '{}' (threshold {:.2})",
            search_terms, threshold
        )
        .into());
    }
    let seed_ids: Vec<String> = matched_fns.iter().map(function_id).collect();
    let note = format!(
        "rustgraph call-graph --search '{}' depth={} include_callers={}: {} seed fn(s)",
        search_terms,
        depth,
        include_callers,
        seed_ids.len()
    );
    let (fns, cm, cs, _) = scope_from_seed_ids(
        project,
        &seed_ids,
        depth,
        include_callers,
        strict_only,
        stats,
    );
    Ok((fns, cm, cs, Some(note)))
}

fn scope_from_seed_ids(
    project: &ProjectData,
    seed_ids: &[String],
    depth: usize,
    include_callers: bool,
    strict_only: bool,
    stats: &mut ResolutionStats,
) -> (
    Vec<FunctionInfo>,
    HashMap<String, Vec<String>>,
    Vec<CallSite>,
    Option<String>,
) {
    let by_id: HashMap<String, &FunctionInfo> = project
        .functions
        .iter()
        .map(|f| (function_id(f), f))
        .collect();
    let by_name: HashMap<&str, Vec<&FunctionInfo>> = {
        let mut m: HashMap<&str, Vec<&FunctionInfo>> = HashMap::new();
        for f in &project.functions {
            m.entry(f.name.as_str()).or_default().push(f);
        }
        m
    };
    let max_depth = if depth == 0 { usize::MAX } else { depth };
    let mut keep: HashSet<String> = HashSet::new();
    for r in seed_ids {
        keep.insert(r.clone());
    }
    let mut frontier: VecDeque<(String, usize)> =
        seed_ids.iter().map(|id| (id.clone(), 0)).collect();
    while let Some((id, d)) = frontier.pop_front() {
        if d >= max_depth {
            continue;
        }
        if let Some(callees) = project.call_map.get(&id) {
            for callee_name in callees {
                let bare = callee_name
                    .rsplit(|c: char| c == '.' || c == ':')
                    .next()
                    .unwrap_or(callee_name.as_str());

                let (filtered, mode) =
                    resolve_callee_candidates(callee_name, bare, &by_name, strict_only);
                stats.record(mode);
                for f in filtered {
                    let cid = function_id(f);
                    if keep.insert(cid.clone()) {
                        frontier.push_back((cid, d + 1));
                    }
                }
            }
        }
    }
    if include_callers {
        let mut callers_by_callee: HashMap<&str, Vec<&CallSite>> = HashMap::new();
        for site in &project.call_sites {
            callers_by_callee
                .entry(site.callee_base.as_str())
                .or_default()
                .push(site);
        }
        let mut up: VecDeque<(String, usize)> = seed_ids.iter().map(|id| (id.clone(), 0)).collect();
        while let Some((id, d)) = up.pop_front() {
            if d >= max_depth {
                continue;
            }
            let Some(target) = by_id.get(&id) else {
                continue;
            };
            if let Some(sites) = callers_by_callee.get(target.name.as_str()) {
                for s in sites {
                    let Some(caller_id) = s.caller_id.as_ref() else {
                        continue;
                    };
                    if keep.insert(caller_id.clone()) {
                        up.push_back((caller_id.clone(), d + 1));
                    }
                }
            }
        }
    }
    let functions_view: Vec<FunctionInfo> = project
        .functions
        .iter()
        .filter(|f| keep.contains(&function_id(f)))
        .cloned()
        .collect();
    let call_map_view: HashMap<String, Vec<String>> = project
        .call_map
        .iter()
        .filter(|(caller_id, _)| keep.contains(caller_id.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let call_sites_view: Vec<CallSite> = project
        .call_sites
        .iter()
        .filter(|s| {
            s.caller_id
                .as_ref()
                .is_some_and(|cid| keep.contains(cid.as_str()))
        })
        .cloned()
        .collect();
    (functions_view, call_map_view, call_sites_view, None)
}

#[allow(clippy::too_many_arguments)]
fn scope_to_root(
    project: &ProjectData,
    root_query: &str,
    depth: usize,
    include_callers: bool,
    strict_only: bool,
    stats: &mut ResolutionStats,
) -> Result<
    (
        Vec<FunctionInfo>,
        HashMap<String, Vec<String>>,
        Vec<CallSite>,
        Option<String>,
    ),
    Box<dyn std::error::Error>,
> {
    let by_id: HashMap<String, &FunctionInfo> = project
        .functions
        .iter()
        .map(|f| (function_id(f), f))
        .collect();

    let root_ids = resolve_root(root_query, project);
    if root_ids.is_empty() {
        return Err(format!(
            "no function matches --root '{}'; try `rustgraph find {}` first",
            root_query, root_query
        )
        .into());
    }

    let max_depth = if depth == 0 { usize::MAX } else { depth };
    let mut keep: HashSet<String> = HashSet::new();
    for r in &root_ids {
        keep.insert(r.clone());
    }

    let by_name: HashMap<&str, Vec<&FunctionInfo>> = {
        let mut m: HashMap<&str, Vec<&FunctionInfo>> = HashMap::new();
        for f in &project.functions {
            m.entry(f.name.as_str()).or_default().push(f);
        }
        m
    };
    let mut frontier: VecDeque<(String, usize)> =
        root_ids.iter().map(|id| (id.clone(), 0)).collect();
    while let Some((id, d)) = frontier.pop_front() {
        if d >= max_depth {
            continue;
        }
        if let Some(callees) = project.call_map.get(&id) {
            for callee_name in callees {
                let bare = callee_name
                    .rsplit(|c: char| c == '.' || c == ':')
                    .next()
                    .unwrap_or(callee_name.as_str());

                let (filtered, mode) =
                    resolve_callee_candidates(callee_name, bare, &by_name, strict_only);
                stats.record(mode);
                for f in filtered {
                    let cid = function_id(f);
                    if keep.insert(cid.clone()) {
                        frontier.push_back((cid, d + 1));
                    }
                }
            }
        }
    }

    if include_callers {
        let mut callers_by_callee: HashMap<&str, Vec<&CallSite>> = HashMap::new();
        for site in &project.call_sites {
            callers_by_callee
                .entry(site.callee_base.as_str())
                .or_default()
                .push(site);
        }
        let mut up: VecDeque<(String, usize)> = root_ids.iter().map(|id| (id.clone(), 0)).collect();
        while let Some((id, d)) = up.pop_front() {
            if d >= max_depth {
                continue;
            }
            let Some(target) = by_id.get(&id) else {
                continue;
            };
            if let Some(sites) = callers_by_callee.get(target.name.as_str()) {
                for s in sites {
                    let Some(caller_id) = s.caller_id.as_ref() else {
                        continue;
                    };
                    if keep.insert(caller_id.clone()) {
                        up.push_back((caller_id.clone(), d + 1));
                    }
                }
            }
        }
    }

    let functions_view: Vec<FunctionInfo> = project
        .functions
        .iter()
        .filter(|f| keep.contains(&function_id(f)))
        .cloned()
        .collect();

    let call_map_view: HashMap<String, Vec<String>> = project
        .call_map
        .iter()
        .filter(|(caller_id, _)| keep.contains(caller_id.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let call_sites_view: Vec<CallSite> = project
        .call_sites
        .iter()
        .filter(|s| {
            s.caller_id
                .as_ref()
                .is_some_and(|c| keep.contains(c.as_str()))
        })
        .cloned()
        .collect();

    let note = format!(
        "rustgraph call-graph --root '{}' depth={} include_callers={}: scoped to {} fn(s) (down from {})",
        root_query,
        if depth == 0 {
            "∞".into()
        } else {
            depth.to_string()
        },
        include_callers,
        functions_view.len(),
        project.functions.len()
    );

    Ok((functions_view, call_map_view, call_sites_view, Some(note)))
}

fn resolve_root(query: &str, project: &ProjectData) -> Vec<String> {
    let trimmed = query.trim();
    if let Some((path_part, line_part)) = trimmed.rsplit_once(':')
        && let Ok(line) = line_part.parse::<usize>()
    {
        return project
            .functions
            .iter()
            .filter(|f| f.start_line == line && f.file_path.contains(path_part))
            .map(function_id)
            .collect();
    }
    project
        .functions
        .iter()
        .filter(|f| f.name == trimmed)
        .map(function_id)
        .collect()
}
