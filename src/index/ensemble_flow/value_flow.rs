use super::super::{CallSite, FunctionInfo, ValueFlowHint};
use super::parsing::{
    extract_assigned_var_from_call_line, extract_call_arg_segment, extract_identifier_list,
};
use std::collections::{HashMap, HashSet};
use std::fs;

/// Build lightweight dataflow hints showing how parameter values of `root_func`
/// flow into its outgoing calls.
///
/// For each call-site in `caller_call_sites` the source text is read (and
/// cached in `file_cache`) and scanned to detect which named arguments
/// originate from `root_func`'s parameters.  Results are sorted by file,
/// producer line, then consumer line, and capped at `max_related` (0 =
/// unlimited).
pub(in crate::index) fn build_value_flow_hints(
    root_func: &FunctionInfo,
    caller_call_sites: &[&CallSite],
    file_cache: &mut HashMap<String, Vec<String>>,
    max_related: usize,
) -> Vec<ValueFlowHint> {
    let mut producers: HashMap<String, (String, usize, String)> = HashMap::new();
    for parameter in &root_func.parameters {
        if let Some((name, _)) = parameter.split_once(':') {
            let parameter_name = name.trim();
            if !parameter_name.is_empty() {
                producers.insert(
                    parameter_name.to_string(),
                    (
                        "<param>".to_string(),
                        root_func.start_line,
                        parameter_name.to_string(),
                    ),
                );
            }
        }
    }

    let mut hints = Vec::new();
    let mut seen: HashSet<(String, usize, String, usize)> = HashSet::new();

    for call_site in caller_call_sites {
        if !file_cache.contains_key(&call_site.file_path) {
            let content = fs::read_to_string(crate::index::resolve_read_path(&call_site.file_path))
                .unwrap_or_default();
            let lines = content
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            file_cache.insert(call_site.file_path.clone(), lines);
        }

        let line_text = file_cache
            .get(&call_site.file_path)
            .and_then(|lines| lines.get(call_site.line.saturating_sub(1)))
            .cloned()
            .unwrap_or_default();

        if let Some(args_segment) = extract_call_arg_segment(&line_text, &call_site.callee_base) {
            for variable in extract_identifier_list(&args_segment) {
                if let Some((producer_call, producer_line, producer_text)) =
                    producers.get(&variable)
                {
                    let key = (
                        variable.clone(),
                        *producer_line,
                        call_site.callee_base.clone(),
                        call_site.line,
                    );
                    if seen.insert(key) {
                        hints.push(ValueFlowHint {
                            variable: variable.clone(),
                            producer_call: producer_call.clone(),
                            producer_line: *producer_line,
                            consumer_call: call_site.callee_base.clone(),
                            consumer_line: call_site.line,
                            file_path: call_site.file_path.clone(),
                            producer_text: producer_text.clone(),
                            consumer_text: line_text.clone(),
                        });
                    }
                }
            }
        }

        if let Some(variable) =
            extract_assigned_var_from_call_line(&line_text, &call_site.callee_base)
        {
            producers.insert(
                variable,
                (call_site.callee_base.clone(), call_site.line, line_text),
            );
        }
    }

    hints.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then(left.producer_line.cmp(&right.producer_line))
            .then(left.consumer_line.cmp(&right.consumer_line))
            .then(left.variable.cmp(&right.variable))
    });
    if max_related != 0 && hints.len() > max_related {
        hints.truncate(max_related);
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    fn func_with_params(parameters: Vec<&str>) -> FunctionInfo {
        FunctionInfo {
            name: "outer".to_string(),
            signature: "fn outer()".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 10,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: Vec::new(),
            parameters: parameters.into_iter().map(|s| s.to_string()).collect(),
            return_type: None,
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    #[test]
    fn build_value_flow_hints_returns_empty_when_no_call_sites() {
        let func = func_with_params(vec!["data: u32"]);
        let mut cache = HashMap::new();
        let hints = build_value_flow_hints(&func, &[], &mut cache, 0);
        assert!(hints.is_empty());
    }

    #[test]
    fn build_value_flow_hints_skips_call_sites_with_unreadable_files() {
        let func = func_with_params(vec!["state: AppState"]);
        let cs = CallSite {
            caller_id: Some("caller".to_string()),
            caller_name: Some("outer".to_string()),
            callee: "process".to_string(),
            callee_base: "process".to_string(),
            call_kind: "function".to_string(),
            file_path: "/no/such/file.rs".to_string(),
            line: 1,
            column: 0,
        };
        let mut cache = HashMap::new();
        let hints = build_value_flow_hints(&func, &[&cs], &mut cache, 0);

        assert!(hints.is_empty());
    }

    #[test]
    fn build_value_flow_hints_truncates_with_max_related() {
        let func = func_with_params(vec!["param: u32"]);
        let mut cache = HashMap::new();

        cache.insert(
            "src/lib.rs".to_string(),
            vec![
                "fn outer(param: u32) {".to_string(),
                "    let mid = first(param);".to_string(),
                "    let other = second(param);".to_string(),
                "}".to_string(),
            ],
        );
        let cs_first = CallSite {
            caller_id: Some("outer_id".to_string()),
            caller_name: Some("outer".to_string()),
            callee: "first".to_string(),
            callee_base: "first".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/lib.rs".to_string(),
            line: 2,
            column: 0,
        };
        let cs_second = CallSite {
            line: 3,
            callee: "second".to_string(),
            callee_base: "second".to_string(),
            ..cs_first.clone()
        };
        let hints = build_value_flow_hints(&func, &[&cs_first, &cs_second], &mut cache, 1);
        assert_eq!(hints.len(), 1);
    }
}
