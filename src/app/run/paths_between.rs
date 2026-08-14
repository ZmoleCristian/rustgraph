use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::super::modes::PathsBetweenRequest;
use super::super::project::ProjectData;
use super::switchboard::write_string_output;
use crate::cli::Args;
use crate::index::qualifier::{
    ResolutionStats, candidate_matches_qualifier, qualifier_for_callee, resolve_callee_candidates,
};
use crate::{FunctionInfo, function_id};

/// Implements `rustgraph paths-between <from> <to>` — DFS reachability search in the call graph.
///
/// Resolves both endpoints (by name, `path:line`, or qualified path), optionally restricts to a
/// module via `--to-module`, and returns all call paths up to `--depth` hops and `--max-results`
/// total paths. When no forward path exists, automatically probes the reverse direction and
/// suggests the swapped command. Outputs a `PathsReport` in text or JSON.
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
            .collect();

        let mut seen: HashSet<String> = to_ids.iter().cloned().collect();
        for id in module_ids {
            if seen.insert(id.clone()) {
                to_ids.push(id);
            }
        }
    }

    if let Some(needle) = &request.in_path {
        from_ids.retain(|id| by_id.get(id).is_some_and(|f| f.file_path.contains(needle)));
        to_ids.retain(|id| by_id.get(id).is_some_and(|f| f.file_path.contains(needle)));
    }

    if from_ids.is_empty() {
        return Err(format!(
            "no function matches FROM '{}'{} (try `rustgraph find {}`)",
            request.from,
            request
                .in_path
                .as_deref()
                .map(|p| format!(" with --in '{}'", p))
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
                .map(|p| format!(" with --in '{}'", p))
                .unwrap_or_default(),
            request.to
        )
        .into());
    }
    let by_name: HashMap<&str, Vec<&FunctionInfo>> = {
        let mut m: HashMap<&str, Vec<&FunctionInfo>> = HashMap::new();
        for f in &project.functions {
            m.entry(f.name.as_str()).or_default().push(f);
        }
        m
    };
    let to_set: HashSet<String> = to_ids.iter().cloned().collect();

    let max_depth = if request.depth == 0 {
        usize::MAX
    } else {
        request.depth
    };

    let cap_is_unlimited = request.max_results == 0;
    let max_paths = if cap_is_unlimited {
        usize::MAX
    } else {
        request.max_results + 1
    };

    let mut resolution_stats = ResolutionStats::default();
    let strict_only = args.strict_resolution;

    let mut paths: Vec<Vec<String>> = Vec::new();
    for from in &from_ids {
        let mut stack: Vec<String> = vec![from.clone()];
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(from.clone());
        dfs(
            from,
            &to_set,
            &project.call_map,
            &by_name,
            &by_id,
            &mut stack,
            &mut visited,
            &mut paths,
            max_depth,
            max_paths,
            strict_only,
            &mut resolution_stats,
        );
        if paths.len() >= max_paths {
            break;
        }
    }

    let bfs_clamped = !cap_is_unlimited && paths.len() == request.max_results + 1;

    paths = dedup_preserve_order(paths);

    let cap_hit = bfs_clamped || (!cap_is_unlimited && paths.len() > request.max_results);
    if !cap_is_unlimited && paths.len() > request.max_results {
        paths.truncate(request.max_results);
    }

    let call_site_index: HashMap<(String, String), usize> = if request.show_call_sites {
        let mut m: HashMap<(String, String), usize> = HashMap::new();
        for s in &project.call_sites {
            if let Some(cid) = &s.caller_id {
                m.entry((cid.clone(), s.callee_base.clone()))
                    .or_insert(s.line);
            }
        }
        m
    } else {
        HashMap::new()
    };

    let mapped_paths: Vec<Vec<NodeView>> = paths
        .into_iter()
        .map(|p| {
            let mut nv: Vec<NodeView> = p
                .iter()
                .filter_map(|id| by_id.get(id).map(|f| node_view(f)))
                .collect();
            if request.show_call_sites {
                for i in 0..nv.len().saturating_sub(1) {
                    let caller_id = &p[i];
                    let next_name = nv[i + 1].name.clone();
                    let line = call_site_index
                        .get(&(caller_id.clone(), next_name.clone()))
                        .copied();
                    nv[i].call_site_line = line;
                }
            }
            nv
        })
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
        cap_hit,
        reverse_suggestion: None,
    };

    if report.paths.is_empty() {
        let from_set: HashSet<String> = from_ids.iter().cloned().collect();
        let reverse_path_cap = if args.json { 3 } else { 1 };
        let mut reverse_paths: Vec<Vec<String>> = Vec::new();
        for to in &to_ids {
            let mut stack: Vec<String> = vec![to.clone()];
            let mut visited: HashSet<String> = HashSet::new();
            visited.insert(to.clone());
            dfs(
                to,
                &from_set,
                &project.call_map,
                &by_name,
                &by_id,
                &mut stack,
                &mut visited,
                &mut reverse_paths,
                max_depth,
                reverse_path_cap,
                strict_only,
                &mut resolution_stats,
            );
            if reverse_paths.len() >= reverse_path_cap {
                break;
            }
        }
        let reverse_has_path = !reverse_paths.is_empty();
        if reverse_has_path {
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
                reverse_paths.truncate(3);
                let mut suggestion: Vec<Vec<NodeView>> = reverse_paths
                    .into_iter()
                    .map(|p| {
                        p.iter()
                            .filter_map(|id| by_id.get(id).map(|f| node_view(f)))
                            .collect()
                    })
                    .collect();

                suggestion.retain(|p| !p.is_empty());
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

    if args.json {
        let payload = serde_json::to_string_pretty(&report)?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {
        write_string_output(args.output.as_deref(), &render_text(&report))?;
    }

    resolution_stats.emit_stderr_note();

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn dfs(
    current: &str,
    to_set: &HashSet<String>,
    call_map: &HashMap<String, Vec<String>>,
    by_name: &HashMap<&str, Vec<&FunctionInfo>>,
    by_id: &HashMap<String, &FunctionInfo>,
    stack: &mut Vec<String>,
    visited: &mut HashSet<String>,
    out: &mut Vec<Vec<String>>,
    max_depth: usize,
    max_paths: usize,

    strict_only: bool,
    stats: &mut ResolutionStats,
) {
    if out.len() >= max_paths {
        return;
    }
    if to_set.contains(current) && stack.len() > 1 {
        out.push(stack.clone());
        return;
    }
    if stack.len() > max_depth {
        return;
    }
    let Some(callees) = call_map.get(current) else {
        return;
    };
    for callee_name in callees {
        let bare = callee_name
            .rsplit(|c: char| c == '.' || c == ':')
            .next()
            .unwrap_or(callee_name.as_str());

        let (filtered, mode) = resolve_callee_candidates(callee_name, bare, by_name, strict_only);
        stats.record(mode);
        for m in filtered {
            let id = function_id(m);
            if !by_id.contains_key(&id) {
                continue;
            }
            if !visited.insert(id.clone()) {
                continue;
            }
            stack.push(id.clone());
            dfs(
                &id,
                to_set,
                call_map,
                by_name,
                by_id,
                stack,
                visited,
                out,
                max_depth,
                max_paths,
                strict_only,
                stats,
            );
            stack.pop();
            visited.remove(&id);
            if out.len() >= max_paths {
                return;
            }
        }
    }
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
    } else {
        format!("{} path(s)", report.paths.len())
    };
    out.push_str(&format!(
        "rustgraph paths-between '{}' → '{}'  [{}]\n",
        report.from, report.to_display, path_count_label
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

/// Deduplicate `paths` in-place while preserving insertion order.
///
/// Avoids the per-element clone that `paths.retain(|p| seen.insert(p.clone()))`
/// would otherwise pay: phase 1 walks the input by reference to compute a
/// keep-mask using borrowed slices as the set key; phase 2 moves only the
/// kept entries via `mem::take` — duplicates are dropped without any clone.
fn dedup_preserve_order(mut paths: Vec<Vec<String>>) -> Vec<Vec<String>> {
    if paths.is_empty() {
        return paths;
    }
    let mut keep: Vec<bool> = Vec::with_capacity(paths.len());
    {
        let mut seen: HashSet<&[String]> = HashSet::with_capacity(paths.len());
        for p in &paths {
            keep.push(seen.insert(p.as_slice()));
        }
    }
    let mut deduped: Vec<Vec<String>> = Vec::with_capacity(paths.len());
    for (i, p) in paths.iter_mut().enumerate() {
        if keep[i] {
            deduped.push(std::mem::take(p));
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::dedup_preserve_order;

    fn p<const N: usize>(items: [&str; N]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn dedup_preserves_first_occurrence_order() {
        let input = vec![
            p(["a", "b"]),
            p(["c", "d"]),
            p(["a", "b"]),
            p(["e"]),
            p(["c", "d"]),
        ];
        let out = dedup_preserve_order(input);
        assert_eq!(
            out,
            vec![p(["a", "b"]), p(["c", "d"]), p(["e"])],
            "duplicates after the first must be dropped, order preserved"
        );
    }

    #[test]
    fn dedup_is_a_noop_when_no_duplicates() {
        let input = vec![p(["a"]), p(["a", "b"]), p(["b"])];
        let out = dedup_preserve_order(input.clone());
        assert_eq!(out, input);
    }

    #[test]
    fn dedup_handles_empty_input() {
        let out = dedup_preserve_order(Vec::<Vec<String>>::new());
        assert!(out.is_empty());
    }
}
