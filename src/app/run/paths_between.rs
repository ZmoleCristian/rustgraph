use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

use serde::Serialize;

use super::super::modes::PathsBetweenRequest;
use super::super::project::ProjectData;
use super::switchboard::write_string_output;
use crate::cli::Args;
use crate::index::qualifier::{candidate_matches_qualifier, qualifier_for_callee};
use crate::index::{collect_framework_handler_refs, resolve_call_targets};
use crate::{FunctionInfo, function_id};

/// Implements `rustgraph paths-between <from> <to>` — bounded shortest-first search in the call graph.
///
/// Resolves both endpoints (by name, `path:line`, or qualified path), optionally restricts to a
/// module via `--to-module`, and returns call paths up to `--depth` nodes and `--max-results`
/// total paths. Call sites are resolved and deduplicated once, then a reverse reachability pass
/// prunes every node that cannot reach the destination. `--max-expansions` provides a hard bound
/// for explicit all-path enumeration. When no forward path exists, the reverse direction is
/// probed and a swapped command is suggested. Outputs a `PathsReport` in text or JSON.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: PathsBetweenRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let by_id: HashMap<String, &FunctionInfo> = project
        .functions
        .iter()
        .map(|f| (function_id(f), f))
        .collect();

    let allowed_ids: HashSet<String> = project
        .functions
        .iter()
        .filter(|function| !args.exclude_tests || !function.is_test)
        .filter(|function| {
            request
                .in_path
                .as_ref()
                .is_none_or(|needle| function.file_path.contains(needle))
        })
        .map(function_id)
        .collect();

    let mut from_ids = resolve_query(&request.from, project);


    let mut to_ids: Vec<String> = if !request.to.is_empty() {
        resolve_query(&request.to, project)
    } else {
        Vec::new()
    };
    if let Some(module_substr) = &request.to_module {
        let module_ids: Vec<String> = project
            .functions
            .iter()
            .filter(|f| f.file_path.contains(module_substr))
            .map(function_id)
            .filter(|id| allowed_ids.contains(id))
            .collect();

        let mut seen: HashSet<String> = to_ids.iter().cloned().collect();
        for id in module_ids {
            if seen.insert(id.clone()) {
                to_ids.push(id);
            }
        }
    }


    from_ids.retain(|id| allowed_ids.contains(id));
    to_ids.retain(|id| allowed_ids.contains(id));
    from_ids.sort();
    from_ids.dedup();
    to_ids.sort();
    to_ids.dedup();

    if from_ids.is_empty() {
        return Err(format!(
            "no function matches FROM '{}'{} (try `rustgraph find {}`)",
            request.from,
            request
                .in_path
                .as_deref()
                .map(|p| format!(" with --target-in '{}'", p))
                .unwrap_or_default(),
            request.from
        )
        .into());
    }
    if to_ids.is_empty() {


        if let Some(module_substr) = &request.to_module
            && request.to.is_empty()
        {
            return Err(format!(
                "no function in any file containing '{}' (try `rustgraph tree` to inspect modules)",
                module_substr
            )
            .into());
        }
        return Err(format!(
            "no function matches TO '{}'{} (try `rustgraph find {}`)",
            request.to,
            request
                .in_path
                .as_deref()
                .map(|p| format!(" with --target-in '{}'", p))
                .unwrap_or_default(),
            request.to
        )
        .into());
    }
    let max_nodes = if request.depth == 0 {
        usize::MAX
    } else {
        request.depth
    };

    let graph = ResolvedGraph::build(project, &allowed_ids);
    let from_nodes = graph.indices(&from_ids);
    let to_nodes: HashSet<usize> = graph.indices(&to_ids).into_iter().collect();
    let search = search_paths(
        &graph,
        &from_nodes,
        &to_nodes,
        max_nodes,
        request.max_results,
        request.max_expansions,
    );
    let mapped_paths = search
        .paths
        .iter()
        .map(|path| map_path(path, &graph, &by_id, request.show_call_sites))
        .collect();


    let to_display = compose_to_display(&request.to, request.to_module.as_deref());
    let mut report = PathsReport {
        from: request.from.clone(),
        to: request.to.clone(),
        to_display: to_display.clone(),
        from_resolved: from_ids
            .iter()
            .filter_map(|id| by_id.get(id).map(|f| node_view(f)))
            .collect(),
        to_resolved: to_ids
            .iter()
            .filter_map(|id| by_id.get(id).map(|f| node_view(f)))
            .collect(),
        paths: mapped_paths,
        cap_hit: search.cap_hit,
        search_truncated: search.expansion_limit_hit,
        expansions: search.expansions,
        max_expansions: request.max_expansions,
        reverse_suggestion: None,
    };

    if report.paths.is_empty() {
        if search.expansion_limit_hit && search.reachable {
            eprintln!(
                "a path from '{}' to '{}' exists within --depth={}, but enumeration stopped after --max-expansions={}; raise the limit to materialize it",
                request.from, to_display, request.depth, request.max_expansions
            );
        } else {
            let from_set: HashSet<usize> = from_nodes.iter().copied().collect();
            let reverse_path_cap = if args.json { 3 } else { 1 };
            let reverse = search_paths(
                &graph,
                &to_nodes.iter().copied().collect::<Vec<_>>(),
                &from_set,
                max_nodes,
                reverse_path_cap,
                request.max_expansions,
            );
            if !reverse.paths.is_empty() {


            let to_label_for_revcmd = if request.to.is_empty() {
                to_display.as_str()
            } else {
                request.to.as_str()
            };
            eprintln!(
                "no path from '{}' to '{}' within --depth={}; reverse direction has path(s) — try `rustgraph paths-between {} {}`",
                request.from, to_display, request.depth, to_label_for_revcmd, request.from
            );


                if args.json {
                    let mut suggestion: Vec<Vec<NodeView>> = reverse
                        .paths
                        .iter()
                        .take(3)
                        .map(|path| map_path(path, &graph, &by_id, false))
                        .collect();
                    suggestion.retain(|path| !path.is_empty());
                    if !suggestion.is_empty() {
                        report.reverse_suggestion = Some(suggestion);
                    }
                }
            } else {
                eprintln!(
                    "no path from '{}' to '{}' within --depth={} (call graph reachability)",
                    request.from, to_display, request.depth
                );
            }
        }
    }

    if args.json {
        let payload = serde_json::to_string_pretty(&report)?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {
        write_string_output(args.output.as_deref(), &render_text(&report))?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct ResolvedEdge {
    target: usize,
    call_site_line: usize,
}

#[derive(Debug)]
struct ResolvedGraph {
    ids: Vec<String>,
    index: HashMap<String, usize>,
    outgoing: Vec<Vec<ResolvedEdge>>,
    incoming: Vec<Vec<usize>>,
}

impl ResolvedGraph {
    fn build(project: &ProjectData, allowed_ids: &HashSet<String>) -> Self {
        let mut ids: Vec<String> = allowed_ids.iter().cloned().collect();
        ids.sort();
        let index: HashMap<String, usize> = ids
            .iter()
            .enumerate()
            .map(|(position, id)| (id.clone(), position))
            .collect();
        let allowed_functions: Vec<FunctionInfo> = project
            .functions
            .iter()
            .filter(|function| allowed_ids.contains(&function_id(function)))
            .cloned()
            .collect();

        let mut call_sites = project.call_sites.clone();
        let mut file_cache = HashMap::new();
        call_sites.extend(collect_framework_handler_refs(
            &project.call_sites,
            &mut file_cache,
        ));
        call_sites.retain(|site| {
            matches!(
                site.call_kind.as_str(),
                "function" | "method" | "function_via_alias" | "method_via_alias"
            )
                && site
                    .caller_id
                    .as_ref()
                    .is_some_and(|caller| allowed_ids.contains(caller))
        });

        let mut resolved = resolve_call_targets(&allowed_functions, &call_sites);
        resolved.sort_by(|left, right| {
            left.caller_id
                .cmp(&right.caller_id)
                .then(left.line.cmp(&right.line))
                .then(left.column.cmp(&right.column))
                .then(left.resolved_internal_id.cmp(&right.resolved_internal_id))
        });

        let mut outgoing = vec![Vec::new(); ids.len()];
        let mut incoming = vec![Vec::new(); ids.len()];
        let mut seen = HashSet::new();
        for call in resolved {
            let Some(target_id) = call.resolved_internal_id else {
                continue;
            };
            let (Some(&caller), Some(&target)) =
                (index.get(&call.caller_id), index.get(&target_id))
            else {
                continue;
            };
            if !seen.insert((caller, target)) {
                continue;
            }
            outgoing[caller].push(ResolvedEdge {
                target,
                call_site_line: call.line,
            });
            incoming[target].push(caller);
        }
        for edges in &mut outgoing {
            edges.sort_by(|left, right| {
                left.call_site_line
                    .cmp(&right.call_site_line)
                    .then(ids[left.target].cmp(&ids[right.target]))
            });
        }
        for callers in &mut incoming {
            callers.sort_unstable();
            callers.dedup();
        }

        Self {
            ids,
            index,
            outgoing,
            incoming,
        }
    }

    fn indices(&self, ids: &[String]) -> Vec<usize> {
        let mut indices: Vec<usize> = ids
            .iter()
            .filter_map(|id| self.index.get(id).copied())
            .collect();
        indices.sort_unstable();
        indices.dedup();
        indices
    }

    fn call_site_line(&self, caller: usize, target: usize) -> Option<usize> {
        self.outgoing[caller]
            .iter()
            .find(|edge| edge.target == target)
            .map(|edge| edge.call_site_line)
    }
}

#[derive(Debug)]
struct PathSearch {
    paths: Vec<Vec<usize>>,
    cap_hit: bool,
    expansion_limit_hit: bool,
    expansions: usize,
    reachable: bool,
}

fn search_paths(
    graph: &ResolvedGraph,
    sources: &[usize],
    targets: &HashSet<usize>,
    max_nodes: usize,
    max_results: usize,
    max_expansions: usize,
) -> PathSearch {
    let distances = reverse_distances(graph, targets);
    let mut queue: BinaryHeap<Reverse<(usize, usize, Vec<usize>)>> = BinaryHeap::new();
    let mut reachable = false;
    let mut states_queued = 0usize;
    let mut expansion_limit_hit = false;
    for &source in sources {
        let Some(distance) = distances[source] else {
            continue;
        };
        if max_nodes != usize::MAX && distance.saturating_add(1) > max_nodes {
            continue;
        }
        reachable = true;
        if max_expansions != 0 && states_queued >= max_expansions {
            expansion_limit_hit = true;
            break;
        }
        queue.push(Reverse((distance.saturating_add(1), 1, vec![source])));
        states_queued += 1;
    }

    let result_limit = if max_results == 0 {
        usize::MAX
    } else {
        max_results.saturating_add(1)
    };
    let mut paths = Vec::new();
    let mut expansions = 0usize;

    while let Some(Reverse((_estimate, _length, path))) = queue.pop() {
        expansions += 1;
        let current = *path.last().expect("queued paths are non-empty");
        if path.len() > 1 && targets.contains(&current) {
            paths.push(path);
            if paths.len() >= result_limit {
                break;
            }
            continue;
        }
        if path.len() >= max_nodes {
            continue;
        }

        // The budget counts admitted path states, not only popped ones. Once
        // exhausted, draining already-queued targets is safe, but creating
        // successors would violate the CPU/memory bound.
        if max_expansions != 0 && states_queued >= max_expansions {
            expansion_limit_hit = true;
            continue;
        }

        for (edge_position, edge) in graph.outgoing[current].iter().enumerate() {
            if path.contains(&edge.target) {
                continue;
            }
            let Some(distance) = distances[edge.target] else {
                continue;
            };
            let next_len = path.len().saturating_add(1);
            let estimated_nodes = next_len.saturating_add(distance);
            if max_nodes != usize::MAX && estimated_nodes > max_nodes {
                continue;
            }
            let mut next = path.clone();
            next.push(edge.target);
            queue.push(Reverse((estimated_nodes, next_len, next)));
            states_queued += 1;
            if max_expansions != 0 && states_queued >= max_expansions {
                let has_omitted_successor = graph.outgoing[current][edge_position + 1..]
                    .iter()
                    .any(|candidate| {
                        if path.contains(&candidate.target) {
                            return false;
                        }
                        distances[candidate.target].is_some_and(|distance| {
                            max_nodes == usize::MAX
                                || next_len.saturating_add(distance) <= max_nodes
                        })
                    });
                expansion_limit_hit |= has_omitted_successor;
                break;
            }
        }
    }

    let cap_hit = max_results != 0 && paths.len() > max_results;
    if cap_hit {
        paths.truncate(max_results);
    }
    PathSearch {
        paths,
        cap_hit,
        expansion_limit_hit,
        expansions,
        reachable,
    }
}

fn reverse_distances(graph: &ResolvedGraph, targets: &HashSet<usize>) -> Vec<Option<usize>> {
    let mut distances: Vec<Option<usize>> = vec![None; graph.ids.len()];
    let mut queue = VecDeque::new();
    let mut ordered_targets: Vec<usize> = targets.iter().copied().collect();
    ordered_targets.sort_unstable();
    for target in ordered_targets {
        distances[target] = Some(0);
        queue.push_back(target);
    }
    while let Some(current) = queue.pop_front() {
        let next_distance = distances[current]
            .expect("queued nodes have a distance")
            .saturating_add(1);
        for &caller in &graph.incoming[current] {
            if distances[caller].is_none() {
                distances[caller] = Some(next_distance);
                queue.push_back(caller);
            }
        }
    }
    distances
}

fn map_path(
    path: &[usize],
    graph: &ResolvedGraph,
    by_id: &HashMap<String, &FunctionInfo>,
    show_call_sites: bool,
) -> Vec<NodeView> {
    path.iter()
        .enumerate()
        .filter_map(|(position, &node)| {
            let function = by_id.get(&graph.ids[node])?;
            let mut view = node_view(function);
            if show_call_sites && let Some(&next) = path.get(position + 1) {
                view.call_site_line = graph.call_site_line(node, next);
            }
            Some(view)
        })
        .collect()
}

fn resolve_query(query: &str, project: &ProjectData) -> Vec<String> {
    let trimmed = query.trim();

    let direct_id_match: Vec<String> = project
        .functions
        .iter()
        .filter(|f| function_id(f) == trimmed)
        .map(function_id)
        .collect();
    if !direct_id_match.is_empty() {
        return direct_id_match;
    }

    if let Some((path_part, line_part)) = trimmed.rsplit_once(':')
        && let Ok(line) = line_part.parse::<usize>()
    {
        let hits: Vec<String> = project
            .functions
            .iter()
            .filter(|f| {
                f.file_path.ends_with(path_part) && f.start_line <= line && line <= f.end_line
            })
            .map(function_id)
            .collect();
        if !hits.is_empty() {
            return hits;
        }
    }


    if let Some(qualifier) = qualifier_for_callee(trimmed) {
        let bare = trimmed
            .rsplit(|c: char| c == '.' || c == ':')
            .next()
            .unwrap_or(trimmed);
        let hits: Vec<String> = project
            .functions
            .iter()
            .filter(|f| f.name == bare && candidate_matches_qualifier(f, Some(qualifier.as_str())))
            .map(function_id)
            .collect();
        if !hits.is_empty() {
            return hits;
        }
    }

    project
        .functions
        .iter()
        .filter(|f| f.name == trimmed)
        .map(function_id)
        .collect()
}

/// Compact JSON/text representation of one function on a call path, optionally annotated with
/// the line where it calls the next hop.
#[derive(Serialize)]
struct NodeView {
    name: String,
    file_path: String,
    start_line: usize,
    symbol_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    call_site_line: Option<usize>,
}

fn node_view(f: &FunctionInfo) -> NodeView {
    NodeView {
        name: f.name.clone(),
        file_path: f.file_path.clone(),
        start_line: f.start_line,
        symbol_id: function_id(f),
        call_site_line: None,
    }
}


fn compose_to_display(to: &str, to_module: Option<&str>) -> String {
    if !to.is_empty() {
        return to.to_string();
    }
    if let Some(module) = to_module {
        return format!("module:{}", module);
    }
    String::new()
}

/// Full output of a `paths-between` query: resolved endpoints, all found paths, and an optional
/// reverse-direction suggestion when the forward direction yields nothing.
#[derive(Serialize)]
struct PathsReport {
    from: String,
    to: String,


    #[serde(skip)]
    to_display: String,
    from_resolved: Vec<NodeView>,
    to_resolved: Vec<NodeView>,
    paths: Vec<Vec<NodeView>>,


    #[serde(skip)]
    cap_hit: bool,

    /// True when the hard path-state budget stopped enumeration before the queue emptied.
    search_truncated: bool,
    /// Number of path states examined by the bounded enumerator.
    expansions: usize,
    /// Configured hard path-state budget (`0` means unlimited).
    max_expansions: usize,


    #[serde(skip_serializing_if = "Option::is_none")]
    reverse_suggestion: Option<Vec<Vec<NodeView>>>,
}

fn render_text(report: &PathsReport) -> String {
    let mut out = String::new();


    let path_count_label = if report.cap_hit {
        format!(
            "≥{} path(s) shown (search clamped — pass --max-results 0 for full enumeration)",
            report.paths.len()
        )
    } else if report.search_truncated {
        format!(
            "{} path(s) shown (search stopped after {} expansions — raise --max-expansions; current limit {})",
            report.paths.len(), report.expansions, report.max_expansions
        )
    } else {
        format!("{} path(s)", report.paths.len())
    };
    out.push_str(&format!(
        "rustgraph paths-between '{}' → '{}'  [{}]\n",
        report.from,
        report.to_display,
        path_count_label
    ));
    if report.paths.is_empty() {
        return out;
    }
    for (i, path) in report.paths.iter().enumerate() {
        out.push_str(&format!("\nPath #{} (length {}):\n", i + 1, path.len()));
        for (j, node) in path.iter().enumerate() {
            let arrow = if j == 0 { "  " } else { "  → " };
            let site = node
                .call_site_line
                .map(|l| format!("  [calls next @ {}:{}]", node.file_path, l))
                .unwrap_or_default();
            out.push_str(&format!(
                "{}{} ({}:{}){}\n",
                arrow, node.name, node.file_path, node.start_line, site
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{ResolvedEdge, ResolvedGraph, search_paths};
    use std::collections::{HashMap, HashSet};

    fn graph(node_count: usize, edges: &[(usize, usize)]) -> ResolvedGraph {
        let ids: Vec<String> = (0..node_count).map(|node| format!("n{node}")).collect();
        let index = ids
            .iter()
            .enumerate()
            .map(|(node, id)| (id.clone(), node))
            .collect::<HashMap<_, _>>();
        let mut outgoing = vec![Vec::new(); node_count];
        let mut incoming = vec![Vec::new(); node_count];
        for (line, &(from, to)) in edges.iter().enumerate() {
            outgoing[from].push(ResolvedEdge {
                target: to,
                call_site_line: line + 1,
            });
            incoming[to].push(from);
        }
        ResolvedGraph {
            ids,
            index,
            outgoing,
            incoming,
        }
    }

    #[test]
    fn depth_is_a_strict_node_count_limit() {
        let graph = graph(3, &[(0, 1), (1, 2)]);
        let targets = HashSet::from([2]);
        let too_shallow = search_paths(&graph, &[0], &targets, 2, 10, 100);
        assert!(too_shallow.paths.is_empty());
        assert!(!too_shallow.reachable);

        let exact = search_paths(&graph, &[0], &targets, 3, 10, 100);
        assert_eq!(exact.paths, vec![vec![0, 1, 2]]);
    }

    #[test]
    fn reverse_reachability_prunes_dense_dead_ends() {
        let mut edges = vec![(0, 1), (1, 2)];
        for node in 3..100 {
            edges.push((0, node));
            if node + 1 < 100 {
                edges.push((node, node + 1));
            }
        }
        let graph = graph(100, &edges);
        let result = search_paths(&graph, &[0], &HashSet::from([2]), usize::MAX, 1, 100);
        assert_eq!(result.paths, vec![vec![0, 1, 2]]);
        assert_eq!(
            result.expansions, 3,
            "dead-end component must never enter the queue"
        );
    }

    #[test]
    fn expansion_budget_is_a_hard_stop() {
        let graph = graph(3, &[(0, 1), (1, 2)]);
        let result = search_paths(
            &graph,
            &[0],
            &HashSet::from([2]),
            usize::MAX,
            10,
            1,
        );
        assert!(result.paths.is_empty());
        assert!(result.reachable);
        assert!(result.expansion_limit_hit);
        assert_eq!(result.expansions, 1);
    }
}
