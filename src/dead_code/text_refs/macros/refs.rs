use std::collections::{HashMap, HashSet};

use crate::CallSite;

use super::body::extract_macro_body_from_file;
use super::candidates::{split_macro_call_candidates, split_macro_identifier_candidates};
use super::supports::macro_supports_identifier_refs;

pub(crate) fn extract_macro_function_refs(
    call_sites: &[CallSite],
    file_lines_cache: &mut HashMap<String, Vec<String>>,
    candidate_names: &HashSet<String>,
) -> Vec<CallSite> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();
    for cs in call_sites.iter().filter(|cs| cs.call_kind == "macro") {
        let macro_base = cs.callee_base.trim_end_matches('!');
        if macro_base == "macro_rules" {
            continue;
        }
        let Some(body) = extract_macro_body_from_file(
            &cs.file_path,
            cs.line,
            cs.column,
            macro_base,
            file_lines_cache,
        ) else {
            continue;
        };
        for (callee, callee_base) in split_macro_call_candidates(&body) {
            if !candidate_names.contains(&callee_base) {
                continue;
            }
            let key = format!(
                "macro:{}:{}:{}:{}",
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
    }
    refs
}

#[cfg(test)]
mod refs_tests {
    use super::*;

    fn write_temp(content: &str) -> std::path::PathBuf {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        std::fs::write(&path, content).expect("write");
        let leaked = dir.keep();
        leaked.join("a.rs")
    }

    fn macro_cs(callee: &str, file: &str) -> CallSite {
        CallSite {
            caller_id: None,
            caller_name: None,
            callee: format!("{callee}!"),
            callee_base: format!("{callee}!"),
            call_kind: "macro".to_string(),
            file_path: file.to_string(),
            line: 1,
            column: 0,
        }
    }

    #[test]
    fn extract_macro_function_refs_finds_known_callees_in_macro_body() {
        let path = write_temp("fn x() { tokio::join!(my_handler(), other()); }\n");
        let mut cache = HashMap::new();
        let candidate: HashSet<String> = ["my_handler", "other"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cs = macro_cs("join", path.to_str().unwrap());

        let refs = extract_macro_function_refs(&[cs], &mut cache, &candidate);
        let bases: Vec<_> = refs.iter().map(|r| r.callee_base.clone()).collect();
        assert!(bases.contains(&"my_handler".to_string()));
        assert!(bases.contains(&"other".to_string()));
    }

    #[test]
    fn extract_macro_function_refs_skips_unknown_candidates() {
        let path = write_temp("fn x() { tokio::join!(unknown_fn()); }\n");
        let mut cache = HashMap::new();
        let candidate: HashSet<String> = ["my_handler"].iter().map(|s| s.to_string()).collect();
        let cs = macro_cs("join", path.to_str().unwrap());
        let refs = extract_macro_function_refs(&[cs], &mut cache, &candidate);
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_macro_function_refs_skips_macro_rules() {
        let path = write_temp("macro_rules! my { () => { foo(); }; }\n");
        let mut cache = HashMap::new();
        let candidate: HashSet<String> = ["foo"].iter().map(|s| s.to_string()).collect();
        let cs = macro_cs("macro_rules", path.to_str().unwrap());
        let refs = extract_macro_function_refs(&[cs], &mut cache, &candidate);
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_macro_identifier_refs_only_for_supported_macros() {
        let path = write_temp("entrypoint!(process_inst);\n");
        let mut cache = HashMap::new();
        let candidate: HashSet<String> = ["process_inst"].iter().map(|s| s.to_string()).collect();
        let cs = macro_cs("entrypoint", path.to_str().unwrap());
        let refs = extract_macro_identifier_refs(&[cs], &mut cache, &candidate);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].callee_base, "process_inst");
    }

    #[test]
    fn extract_macro_identifier_refs_ignores_unsupported_macros() {
        let path = write_temp("println!(\"hi\");\n");
        let mut cache = HashMap::new();
        let candidate: HashSet<String> = ["hi"].iter().map(|s| s.to_string()).collect();
        let cs = macro_cs("println", path.to_str().unwrap());
        let refs = extract_macro_identifier_refs(&[cs], &mut cache, &candidate);
        assert!(refs.is_empty());
    }

    #[test]
    fn extract_macro_function_refs_dedups_repeated_invocations_of_same_call() {
        let path = write_temp("fn x() { vec!(my_fn(), my_fn()); }\n");
        let mut cache = HashMap::new();
        let candidate: HashSet<String> = ["my_fn"].iter().map(|s| s.to_string()).collect();
        let cs = macro_cs("vec", path.to_str().unwrap());
        let refs = extract_macro_function_refs(&[cs], &mut cache, &candidate);

        assert_eq!(refs.len(), 1);
    }
}

pub(crate) fn extract_macro_identifier_refs(
    call_sites: &[CallSite],
    file_lines_cache: &mut HashMap<String, Vec<String>>,
    candidate_names: &HashSet<String>,
) -> Vec<CallSite> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();
    for cs in call_sites.iter().filter(|cs| cs.call_kind == "macro") {
        let macro_base = cs.callee_base.trim_end_matches('!');
        if macro_base == "macro_rules" {
            continue;
        }
        if !macro_supports_identifier_refs(macro_base) {
            continue;
        }
        let Some(body) = extract_macro_body_from_file(
            &cs.file_path,
            cs.line,
            cs.column,
            macro_base,
            file_lines_cache,
        ) else {
            continue;
        };
        for (callee, callee_base) in split_macro_identifier_candidates(&body) {
            if !candidate_names.contains(&callee_base) {
                continue;
            }
            let key = format!(
                "macro_ident:{}:{}:{}:{}",
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
    }
    refs
}
