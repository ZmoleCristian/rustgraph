use super::super::CallSite;
use crate::source_refs::{extract_nth_call_arg_segment_from_file, normalize_handler_expr};
use std::collections::{HashMap, HashSet};

/// Scan `call_sites` for framework routing registrations (e.g. `axum::get`,
/// `post`, `on`) and synthesize a `CallSite` whose callee points at the actual
/// handler function extracted from the source text.
///
/// `file_cache` is populated lazily: each file is read once and its lines are
/// stored for reuse across calls.
pub(crate) fn collect_framework_handler_refs(
    call_sites: &[CallSite],
    file_cache: &mut HashMap<String, Vec<String>>,
) -> Vec<CallSite> {
    let mut refs = Vec::new();
    let mut seen = HashSet::new();

    for call_site in call_sites
        .iter()
        .filter(|call_site| call_site.call_kind == "function" || call_site.call_kind == "method")
    {
        let arg_index = match call_site.callee_base.as_str() {
            "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "trace" | "any"
            | "to" | "service" | "from_fn" => 0usize,
            "on" => 1usize,
            _ => continue,
        };

        let Some(arg_expr) = extract_nth_call_arg_segment_from_file(
            &call_site.file_path,
            call_site.line,
            call_site.column,
            &call_site.callee_base,
            arg_index,
            file_cache,
        ) else {
            continue;
        };
        let Some((callee, callee_base)) = normalize_handler_expr(&arg_expr) else {
            continue;
        };
        let key = format!(
            "{}:{}:{}:{callee}",
            call_site.file_path, call_site.line, call_site.column
        );
        if !seen.insert(key) {
            continue;
        }

        refs.push(CallSite {
            caller_id: call_site.caller_id.clone(),
            caller_name: call_site.caller_name.clone(),
            callee,
            callee_base,
            call_kind: "function".to_string(),
            file_path: call_site.file_path.clone(),
            line: call_site.line,
            column: call_site.column,
        });
    }

    refs
}

/// Returns `true` for callee identifiers that carry no useful resolution
/// signal (e.g. `Ok`, `Err`, `Some`, `None`).
pub(in crate::index) fn is_low_signal_unresolved_callee(callee_base: &str) -> bool {
    matches!(callee_base, "Ok" | "Err" | "Some" | "None")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_low_signal_unresolved_callee_flags_constructor_like_idents() {
        for word in ["Ok", "Err", "Some", "None"] {
            assert!(is_low_signal_unresolved_callee(word));
        }
    }

    #[test]
    fn is_low_signal_unresolved_callee_returns_false_for_normal_calls() {
        assert!(!is_low_signal_unresolved_callee("foo"));
        assert!(!is_low_signal_unresolved_callee("ok"));
    }

    #[test]
    fn collect_framework_handler_refs_is_empty_when_no_relevant_calls() {
        let cs = vec![CallSite {
            caller_id: Some("c".to_string()),
            caller_name: Some("c".to_string()),
            callee: "foo".to_string(),
            callee_base: "foo".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/x.rs".to_string(),
            line: 1,
            column: 0,
        }];
        let mut cache = HashMap::new();
        let out = collect_framework_handler_refs(&cs, &mut cache);
        assert!(out.is_empty());
    }

    #[test]
    fn collect_framework_handler_refs_skips_unparseable_files() {
        let cs = vec![CallSite {
            caller_id: Some("c".to_string()),
            caller_name: Some("c".to_string()),
            callee: "axum::routing::get".to_string(),
            callee_base: "get".to_string(),
            call_kind: "function".to_string(),
            file_path: "/nonexistent/path/x.rs".to_string(),
            line: 1,
            column: 0,
        }];
        let mut cache = HashMap::new();
        let out = collect_framework_handler_refs(&cs, &mut cache);
        assert!(out.is_empty());
    }
}
