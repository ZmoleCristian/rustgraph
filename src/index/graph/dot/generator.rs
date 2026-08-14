use super::super::super::{CallSite, FunctionInfo, function_id, resolve_call_targets};
use super::filter::should_include_external_call;
use super::utils::{callee_base_name, dot_escape};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Render a call graph as a Graphviz DOT document.
///
/// `detail_level` accepts `"internal"`, `"external"`, or `"all"` to control
/// which call edges are emitted.
pub fn generate_call_graph(
    _project_path: &Path,
    functions: &[FunctionInfo],
    call_map: &HashMap<String, Vec<String>>,
    call_sites: &[CallSite],
    detail_level: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut dot_graph = String::from("digraph call_graph {\n");
    dot_graph.push_str("  rankdir=LR;\n");
    dot_graph.push_str("  node [shape=box, style=rounded];\n");
    dot_graph.push_str("  edge [color=blue, fontsize=10];\n\n");

    let mut name_to_ids: HashMap<String, Vec<String>> = HashMap::new();
    let mut node_ids = HashSet::new();

    for func in functions {
        let func_id = function_id(func);
        node_ids.insert(func_id.clone());
        name_to_ids
            .entry(func.name.clone())
            .or_default()
            .push(func_id.clone());

        let file_label = Path::new(&func.file_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&func.file_path);
        let label = format!("{}\\n{}:{}", func.name, file_label, func.start_line);
        dot_graph.push_str(&format!(
            "  \"{}\" [label=\"{}\"];\n",
            dot_escape(&func_id),
            dot_escape(&label)
        ));
    }

    dot_graph.push('\n');

    let include_internal = matches!(detail_level, "internal" | "all");
    let include_external = matches!(detail_level, "external" | "all");
    let mut edge_seen = HashSet::<(String, String, String)>::new();
    let mut external_node_seen = HashSet::<String>::new();

    if !call_sites.is_empty() {
        for call in resolve_call_targets(functions, call_sites) {
            if !node_ids.contains(&call.caller_id) {
                continue;
            }

            let mut resolved_internal = false;
            if include_internal
                && let Some(target_id) = call.resolved_internal_id.clone()
                && node_ids.contains(&target_id)
            {
                resolved_internal = true;
                if edge_seen.insert((
                    call.caller_id.clone(),
                    target_id.clone(),
                    "internal".to_string(),
                )) {
                    dot_graph.push_str(&format!(
                        "  \"{}\" -> \"{}\";\n",
                        dot_escape(&call.caller_id),
                        dot_escape(&target_id)
                    ));
                }
            }
            if resolved_internal {
                continue;
            }

            if include_external && should_include_external_call(&call.callee) {
                let ext_id = format!("ext::{}", call.callee);
                if external_node_seen.insert(ext_id.clone()) {
                    dot_graph.push_str(&format!(
                        "  \"{}\" [label=\"{}\", shape=ellipse, style=dashed, color=gray, fontcolor=gray];\n",
                        dot_escape(&ext_id),
                        dot_escape(&call.callee)
                    ));
                }
                if edge_seen.insert((
                    call.caller_id.clone(),
                    ext_id.clone(),
                    "external".to_string(),
                )) {
                    dot_graph.push_str(&format!(
                        "  \"{}\" -> \"{}\" [style=dashed, color=gray];\n",
                        dot_escape(&call.caller_id),
                        dot_escape(&ext_id)
                    ));
                }
            }
        }
    } else {
        for (caller_id, callees) in call_map {
            if !node_ids.contains(caller_id) {
                continue;
            }

            for callee in callees {
                let base = callee_base_name(callee);
                let maybe_ids = name_to_ids.get(&base);
                let is_internal_unique = maybe_ids.is_some_and(|ids| ids.len() == 1);

                if include_internal && is_internal_unique {
                    let target_id = &maybe_ids.expect("checked")[0];
                    if edge_seen.insert((
                        caller_id.clone(),
                        target_id.clone(),
                        "internal".to_string(),
                    )) {
                        dot_graph.push_str(&format!(
                            "  \"{}\" -> \"{}\";\n",
                            dot_escape(caller_id),
                            dot_escape(target_id)
                        ));
                    }
                } else if include_external && should_include_external_call(callee) {
                    let ext_id = format!("ext::{}", callee);
                    if external_node_seen.insert(ext_id.clone()) {
                        dot_graph.push_str(&format!(
                            "  \"{}\" [label=\"{}\", shape=ellipse, style=dashed, color=gray, fontcolor=gray];\n",
                            dot_escape(&ext_id),
                            dot_escape(callee)
                        ));
                    }
                    if edge_seen.insert((caller_id.clone(), ext_id.clone(), "external".to_string()))
                    {
                        dot_graph.push_str(&format!(
                            "  \"{}\" -> \"{}\" [style=dashed, color=gray];\n",
                            dot_escape(caller_id),
                            dot_escape(&ext_id)
                        ));
                    }
                }
            }
        }
    }

    dot_graph.push_str("}\n");

    Ok(dot_graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn func(name: &str, file_path: &str, line: usize) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}"),
            file_path: file_path.to_string(),
            start_line: line,
            end_line: line,
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
    fn generate_call_graph_emits_digraph_header_and_footer() {
        let project = PathBuf::from(".");
        let dot = generate_call_graph(&project, &[], &HashMap::new(), &[], "internal").unwrap();
        assert!(dot.starts_with("digraph call_graph {"));
        assert!(dot.ends_with("}\n"));
    }

    #[test]
    fn generate_call_graph_emits_node_for_each_function() {
        let project = PathBuf::from(".");
        let funcs = vec![func("alpha", "src/a.rs", 1), func("beta", "src/b.rs", 2)];
        let dot = generate_call_graph(&project, &funcs, &HashMap::new(), &[], "internal").unwrap();
        assert!(dot.contains("\"src/a.rs:1:alpha\""));
        assert!(dot.contains("\"src/b.rs:2:beta\""));

        assert!(dot.contains(r"alpha\\na.rs:1"));
        assert!(dot.contains(r"beta\\nb.rs:2"));
    }

    #[test]
    fn generate_call_graph_internal_includes_resolved_edges() {
        let project = PathBuf::from(".");
        let caller = func("caller", "src/a.rs", 1);
        let callee = func("callee", "src/b.rs", 5);
        let funcs = vec![caller.clone(), callee.clone()];

        let cs = CallSite {
            caller_id: Some(function_id(&caller)),
            caller_name: Some(caller.name.clone()),
            callee: "callee".to_string(),
            callee_base: "callee".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/a.rs".to_string(),
            line: 1,
            column: 0,
        };
        let dot =
            generate_call_graph(&project, &funcs, &HashMap::new(), &[cs], "internal").unwrap();
        assert!(dot.contains("\"src/a.rs:1:caller\" -> \"src/b.rs:5:callee\";"));
    }

    #[test]
    fn generate_call_graph_external_only_emits_external_edges() {
        let project = PathBuf::from(".");
        let caller = func("caller", "src/a.rs", 1);
        let funcs = vec![caller.clone()];
        let cs = CallSite {
            caller_id: Some(function_id(&caller)),
            caller_name: Some(caller.name.clone()),
            callee: "std::fs::read_to_string".to_string(),
            callee_base: "read_to_string".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/a.rs".to_string(),
            line: 1,
            column: 0,
        };
        let dot =
            generate_call_graph(&project, &funcs, &HashMap::new(), &[cs], "external").unwrap();
        assert!(dot.contains("ext::std::fs::read_to_string"));
        assert!(dot.contains("style=dashed"));
    }

    #[test]
    fn generate_call_graph_falls_back_to_call_map_when_no_call_sites() {
        let project = PathBuf::from(".");
        let caller = func("caller", "src/a.rs", 1);
        let callee = func("callee", "src/b.rs", 5);
        let funcs = vec![caller.clone(), callee.clone()];
        let mut call_map = HashMap::new();
        call_map.insert(function_id(&caller), vec!["callee".to_string()]);

        let dot = generate_call_graph(&project, &funcs, &call_map, &[], "internal").unwrap();
        assert!(dot.contains("\"src/a.rs:1:caller\" -> \"src/b.rs:5:callee\";"));
    }
}
