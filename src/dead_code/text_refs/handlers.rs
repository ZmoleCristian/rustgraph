//! Detectors for framework handler and functional-argument call patterns.

use std::collections::{HashMap, HashSet};

use super::parsing::{extract_call_arg_segment_from_file, normalize_handler_expr};
use crate::CallSite;

/// Extract synthetic `CallSite`s for functions passed as handlers to routing macros.
///
/// Scans `call_sites` for known framework routing methods (`get`, `post`, `put`, etc.)
/// and reads the source file to extract the handler argument, normalising it to a
/// `CallSite` so the downstream reference counter treats it as a call.
/// Duplicate `(file, line, column, callee)` tuples are deduplicated.
pub(crate) fn extract_framework_handler_refs(
    call_sites: &[CallSite],
    file_lines_cache: &mut HashMap<String, Vec<String>>,
) -> Vec<CallSite> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();

    for cs in call_sites
        .iter()
        .filter(|cs| cs.call_kind == "function" || cs.call_kind == "method")
    {
        let arg_index = match cs.callee_base.as_str() {
            "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "trace" | "any"
            | "to" | "service" | "from_fn" => 0usize,
            "on" => 1usize,
            _ => continue,
        };

        let Some(arg_expr) = extract_call_arg_segment_from_file(
            &cs.file_path,
            cs.line,
            cs.column,
            &cs.callee_base,
            arg_index,
            file_lines_cache,
        ) else {
            continue;
        };
        let Some((callee, callee_base)) = normalize_handler_expr(&arg_expr) else {
            continue;
        };

        let key = format!("{}:{}:{}:{}", cs.file_path, cs.line, cs.column, callee);
        if !seen.insert(key) {
            continue;
        }

        refs.push(CallSite {
            caller_id: cs.caller_id.clone(),
            caller_name: cs.caller_name.clone(),
            callee,
            callee_base,
            call_kind: "function".to_string(),
            file_path: cs.file_path.clone(),
            line: cs.line,
            column: cs.column,
        });
    }

    refs
}

#[cfg(test)]
mod handler_tests {
    use super::*;
    use std::io::Write;

    fn write_temp(content: &str) -> std::path::PathBuf {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(content.as_bytes()).expect("write");
        let leaked = dir.keep();
        leaked.join("a.rs")
    }

    fn cs(callee_base: &str, file_path: &str) -> CallSite {
        CallSite {
            caller_id: None,
            caller_name: None,
            callee: callee_base.to_string(),
            callee_base: callee_base.to_string(),
            call_kind: "function".to_string(),
            file_path: file_path.to_string(),
            line: 1,
            column: 0,
        }
    }

    #[test]
    fn extract_framework_handler_refs_returns_empty_for_unrelated_calls() {
        let path = write_temp("fn x() {}\n");
        let mut cache = HashMap::new();
        let calls = vec![cs("foo", path.to_str().unwrap())];
        let refs = extract_framework_handler_refs(&calls, &mut cache);
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_framework_handler_refs_extracts_handler_for_get() {
        let path = write_temp("get(my_handler);\n");
        let mut cache = HashMap::new();
        let calls = vec![cs("get", path.to_str().unwrap())];
        let refs = extract_framework_handler_refs(&calls, &mut cache);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].callee_base, "my_handler");
    }

    #[test]
    fn extract_framework_handler_refs_handles_on_uses_second_arg() {
        let path = write_temp("on(method, my_on_handler);\n");
        let mut cache = HashMap::new();
        let calls = vec![cs("on", path.to_str().unwrap())];
        let refs = extract_framework_handler_refs(&calls, &mut cache);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].callee_base, "my_on_handler");
    }

    #[test]
    fn extract_framework_handler_refs_skips_closure_arguments() {
        let path = write_temp("get(|x| x);\n");
        let mut cache = HashMap::new();
        let calls = vec![cs("get", path.to_str().unwrap())];
        let refs = extract_framework_handler_refs(&calls, &mut cache);
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_framework_handler_refs_dedups_repeated_lookups() {
        let path = write_temp("get(my_handler);\n");
        let mut cache = HashMap::new();
        let calls = vec![
            cs("get", path.to_str().unwrap()),
            cs("get", path.to_str().unwrap()),
        ];
        let refs = extract_framework_handler_refs(&calls, &mut cache);
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn extract_functional_arg_refs_picks_up_map_arg() {
        let path = write_temp("vec.into_iter().map(my_mapper);\n");
        let mut cache = HashMap::new();
        let mut call = cs("map", path.to_str().unwrap());
        call.call_kind = "method".to_string();
        let calls = vec![call];
        let refs = extract_functional_arg_refs(&calls, &mut cache);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].callee_base, "my_mapper");
    }

    #[test]
    fn extract_functional_arg_refs_picks_up_fold_second_arg() {
        let path = write_temp("vec.into_iter().fold(0, my_reducer);\n");
        let mut cache = HashMap::new();
        let mut call = cs("fold", path.to_str().unwrap());
        call.call_kind = "method".to_string();
        let refs = extract_functional_arg_refs(&[call], &mut cache);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].callee_base, "my_reducer");
    }

    #[test]
    fn extract_functional_arg_refs_returns_empty_for_unsupported_method() {
        let path = write_temp("vec.unsupported(thing);\n");
        let mut cache = HashMap::new();
        let mut call = cs("unsupported", path.to_str().unwrap());
        call.call_kind = "method".to_string();
        let refs = extract_functional_arg_refs(&[call], &mut cache);
        assert!(refs.is_empty());
    }
}

/// Extract synthetic `CallSite`s for functions passed as arguments to functional combinators.
///
/// Handles methods such as `map`, `filter_map`, `for_each`, `fold`, `spawn`, and similar
/// higher-order combinators.  Reads source files to resolve the argument expression and
/// emits a `CallSite` for each uniquely resolved function reference.
pub(crate) fn extract_functional_arg_refs(
    call_sites: &[CallSite],
    file_lines_cache: &mut HashMap<String, Vec<String>>,
) -> Vec<CallSite> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();

    for cs in call_sites
        .iter()
        .filter(|cs| cs.call_kind == "function" || cs.call_kind == "method")
    {
        let arg_index = match cs.callee_base.as_str() {
            "map" | "filter_map" | "for_each" | "inspect" | "then" | "and_then" | "or_else"
            | "spawn" | "spawn_blocking" | "unwrap_or_else" | "validator" | "validator_os" => {
                0usize
            }
            "fold" | "try_fold" | "map_or_else" => 1usize,
            _ => continue,
        };

        let Some(arg_expr) = extract_call_arg_segment_from_file(
            &cs.file_path,
            cs.line,
            cs.column,
            &cs.callee_base,
            arg_index,
            file_lines_cache,
        ) else {
            continue;
        };
        let Some((callee, callee_base)) = normalize_handler_expr(&arg_expr) else {
            continue;
        };

        let key = format!(
            "functional:{}:{}:{}:{}",
            cs.file_path, cs.line, cs.column, callee
        );
        if !seen.insert(key) {
            continue;
        }

        refs.push(CallSite {
            caller_id: cs.caller_id.clone(),
            caller_name: cs.caller_name.clone(),
            callee,
            callee_base,
            call_kind: "function".to_string(),
            file_path: cs.file_path.clone(),
            line: cs.line,
            column: cs.column,
        });
    }

    refs
}
