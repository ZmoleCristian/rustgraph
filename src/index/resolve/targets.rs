//! Top-level call-target resolution: maps every call site to a `ResolvedCallTarget`.

use super::super::{CallSite, FunctionInfo, ResolvedCallTarget, function_id};
use super::ambiguous::{
    module_path_ends_with, normalize_callee_prefix, resolve_ambiguous_call_target,
};
use super::module_paths::build_function_module_lookup;
use std::collections::HashMap;

/// Resolve every call site in `call_sites` to a [`ResolvedCallTarget`].
///
/// Call sites with no known caller or an unrecognised caller ID are skipped.
/// When resolution succeeds, `resolved_internal_id` holds the callee's stable ID;
/// when it fails, the field is `None` and the site is still included in the output.
pub fn resolve_call_targets(
    functions: &[FunctionInfo],
    call_sites: &[CallSite],
) -> Vec<ResolvedCallTarget> {
    let function_by_id = functions
        .iter()
        .map(|function| (function_id(function), function))
        .collect::<HashMap<_, _>>();

    let mut name_to_ids: HashMap<String, Vec<String>> = HashMap::new();
    for function in functions {
        name_to_ids
            .entry(function.name.clone())
            .or_default()
            .push(function_id(function));
    }

    let module_lookup = build_function_module_lookup(functions);
    let mut resolved = Vec::new();

    for call_site in call_sites {
        let Some(caller_id) = &call_site.caller_id else {
            continue;
        };
        if !function_by_id.contains_key(caller_id) {
            continue;
        }

        let resolved_internal_id = name_to_ids.get(&call_site.callee_base).and_then(|candidate_ids| {
            let compatible_ids = compatible_candidate_ids(call_site, candidate_ids, &function_by_id);
            resolve_owner_qualified_method_target(
                call_site,
                &compatible_ids,
                &function_by_id,
                &module_lookup,
            )
            .or_else(|| {
                resolve_ambiguous_call_target(
                    call_site,
                    &compatible_ids,
                    &function_by_id,
                    &module_lookup,
                )
            })
        });

        resolved.push(ResolvedCallTarget {
            caller_id: caller_id.clone(),
            callee: call_site.callee.clone(),
            callee_base: call_site.callee_base.clone(),
            resolved_internal_id,
            file_path: call_site.file_path.clone(),
            line: call_site.line,
            column: call_site.column,
        });
    }

    resolved
}

/// Keep only definitions that can be invoked by the syntactic call kind.
///
/// Without this guard, a variable method call such as `listener.accept()` can
/// resolve to an unrelated free function named `accept`, creating a phantom
/// edge that is especially damaging to transitive path searches.
fn compatible_candidate_ids(
    call_site: &CallSite,
    candidate_ids: &[String],
    function_by_id: &HashMap<String, &FunctionInfo>,
) -> Vec<String> {
    let call_kind = call_site
        .call_kind
        .strip_suffix("_via_alias")
        .unwrap_or(call_site.call_kind.as_str());
    let type_qualified = call_site
        .callee
        .rsplit_once("::")
        .and_then(|(owner, _)| owner.rsplit("::").next())
        .and_then(|owner| owner.trim().chars().next())
        .is_some_and(|first| first.is_ascii_uppercase());

    candidate_ids
        .iter()
        .filter(|id| {
            let Some(function) = function_by_id.get(*id).copied() else {
                return false;
            };
            let is_method = function.kind.starts_with("method(")
                || function.kind.starts_with("trait_fn(");
            match call_kind {
                "method" => is_method,
                "function" if type_qualified => is_method,
                "function" => !is_method,
                _ => false,
            }
        })
        .cloned()
        .collect()
}

fn resolve_owner_qualified_method_target(
    call_site: &CallSite,
    candidate_ids: &[String],
    function_by_id: &HashMap<String, &FunctionInfo>,
    module_lookup: &HashMap<String, Vec<String>>,
) -> Option<String> {
    let callee_segments: Vec<String> = call_site
        .callee
        .split("::")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect();
    if callee_segments.len() < 2 {
        return None;
    }

    let raw_owner_prefix = &callee_segments[..callee_segments.len() - 1];
    let caller_module = call_site
        .caller_id
        .as_ref()
        .and_then(|id| module_lookup.get(id).cloned());
    let normalized_owner_prefix = normalize_callee_prefix(raw_owner_prefix, caller_module.as_deref());

    let mut matched = Vec::new();
    for id in candidate_ids {
        let Some(function) = function_by_id.get(id).copied() else {
            continue;
        };
        let Some(owner_segments) = method_owner_segments(function) else {
            continue;
        };
        if owner_path_matches(&owner_segments, raw_owner_prefix, normalized_owner_prefix.as_deref()) {
            matched.push(id.clone());
        }
    }

    if matched.len() == 1 {
        matched.into_iter().next()
    } else {
        None
    }
}

fn method_owner_segments(function: &FunctionInfo) -> Option<Vec<String>> {
    let kind = function
        .kind
        .strip_prefix("method(")
        .or_else(|| function.kind.strip_prefix("trait_fn("))?
        .strip_suffix(')')?
        .trim();
    if kind.is_empty() {
        return None;
    }

    let normalized = kind
        .strip_prefix("impl ")
        .unwrap_or(kind)
        .replace(' ', "");
    let without_generics = normalized
        .split('<')
        .next()
        .unwrap_or(normalized.as_str())
        .trim();
    let segments = without_generics
        .split("::")
        .map(str::trim)
        .filter(|segment| !segment.is_empty() && *segment != "self" && *segment != "Self")
        .map(str::to_string)
        .collect::<Vec<_>>();
    if segments.is_empty() {
        None
    } else {
        Some(segments)
    }
}

fn owner_path_matches(
    owner_segments: &[String],
    raw_owner_prefix: &[String],
    normalized_owner_prefix: Option<&[String]>,
) -> bool {
    if owner_segments.is_empty() || raw_owner_prefix.is_empty() {
        return false;
    }

    if owner_segments == raw_owner_prefix
        || module_path_ends_with(owner_segments, raw_owner_prefix)
        || module_path_ends_with(raw_owner_prefix, owner_segments)
    {
        return true;
    }

    if let Some(normalized) = normalized_owner_prefix {
        if owner_segments == normalized
            || module_path_ends_with(owner_segments, normalized)
            || module_path_ends_with(normalized, owner_segments)
        {
            return true;
        }
    }

    owner_segments.last() == raw_owner_prefix.last()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn function(name: &str, file: &str, kind: &str, line: usize) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}()"),
            file_path: file.to_string(),
            start_line: line,
            end_line: line + 1,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: Vec::new(),
            parameters: Vec::new(),
            return_type: None,
            kind: kind.to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    #[test]
    fn method_owner_segments_extracts_clean_owner() {
        let f = function("compute", "src/lib.rs", "method(MyType)", 1);
        let segs = method_owner_segments(&f);
        assert_eq!(segs.as_deref(), Some(&[String::from("MyType")][..]));
    }

    #[test]
    fn method_owner_segments_strips_generics() {
        let f = function("len", "src/lib.rs", "method(Vec<u8>)", 1);
        let segs = method_owner_segments(&f);
        assert_eq!(segs.as_deref(), Some(&[String::from("Vec")][..]));
    }

    #[test]
    fn method_owner_segments_strips_self_and_self_keywords() {
        let f = function("len", "src/lib.rs", "method(self::MyType)", 1);
        let segs = method_owner_segments(&f);
        assert_eq!(segs.as_deref(), Some(&[String::from("MyType")][..]));
    }

    #[test]
    fn method_owner_segments_returns_none_for_function_kind() {
        let f = function("free", "src/lib.rs", "function", 1);
        assert!(method_owner_segments(&f).is_none());
    }

    #[test]
    fn method_owner_segments_returns_none_for_empty_kind() {
        let f = function("free", "src/lib.rs", "method()", 1);
        assert!(method_owner_segments(&f).is_none());
    }

    #[test]
    fn owner_path_matches_full_match() {
        assert!(owner_path_matches(&s(&["MyType"]), &s(&["MyType"]), None));
    }

    #[test]
    fn owner_path_matches_via_suffix_match() {
        assert!(owner_path_matches(
            &s(&["module", "MyType"]),
            &s(&["MyType"]),
            None
        ));
    }

    #[test]
    fn owner_path_matches_via_normalized_path() {

        let normalized = s(&["MyType"]);
        assert!(owner_path_matches(
            &s(&["MyType"]),
            &s(&["unrelated"]),
            Some(&normalized)
        ));
    }

    #[test]
    fn owner_path_matches_falls_back_to_last_segment() {

        assert!(owner_path_matches(&s(&["a", "MyType"]), &s(&["b", "MyType"]), None));
    }

    #[test]
    fn owner_path_matches_returns_false_when_either_empty() {
        assert!(!owner_path_matches(&[], &s(&["MyType"]), None));
        assert!(!owner_path_matches(&s(&["MyType"]), &[], None));
    }

    #[test]
    fn resolve_call_targets_returns_empty_when_no_call_sites() {
        let resolved = resolve_call_targets(&[], &[]);
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolve_call_targets_records_resolved_internal_id() {
        let caller = function("caller", "src/lib.rs", "function", 1);
        let callee = function("compute", "src/lib.rs", "function", 5);
        let cs = CallSite {
            caller_id: Some(function_id(&caller)),
            caller_name: Some("caller".to_string()),
            callee: "compute".to_string(),
            callee_base: "compute".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/lib.rs".to_string(),
            line: 2,
            column: 0,
        };
        let resolved = resolve_call_targets(&[caller, callee.clone()], &[cs]);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].resolved_internal_id, Some(function_id(&callee)));
    }

    #[test]
    fn resolve_call_targets_skips_call_sites_with_unknown_caller() {
        let callee = function("compute", "src/lib.rs", "function", 5);
        let cs = CallSite {
            caller_id: Some("unknown_caller_id".to_string()),
            caller_name: Some("caller".to_string()),
            callee: "compute".to_string(),
            callee_base: "compute".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/lib.rs".to_string(),
            line: 2,
            column: 0,
        };
        let resolved = resolve_call_targets(&[callee], &[cs]);
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolve_call_targets_unresolved_when_callee_unknown() {
        let caller = function("caller", "src/lib.rs", "function", 1);
        let cs = CallSite {
            caller_id: Some(function_id(&caller)),
            caller_name: Some("caller".to_string()),
            callee: "missing".to_string(),
            callee_base: "missing".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/lib.rs".to_string(),
            line: 2,
            column: 0,
        };
        let resolved = resolve_call_targets(&[caller], &[cs]);
        assert_eq!(resolved.len(), 1);
        assert!(resolved[0].resolved_internal_id.is_none());
    }

    #[test]
    fn method_call_never_resolves_to_same_name_free_function() {
        let caller = function("caller", "src/lib.rs", "function", 1);
        let free_accept = function("accept", "src/listen.rs", "function", 5);
        let cs = CallSite {
            caller_id: Some(function_id(&caller)),
            caller_name: Some("caller".to_string()),
            callee: "listener.accept".to_string(),
            callee_base: "accept".to_string(),
            call_kind: "method".to_string(),
            file_path: "src/lib.rs".to_string(),
            line: 2,
            column: 0,
        };
        let resolved = resolve_call_targets(&[caller, free_accept], &[cs]);
        assert!(resolved[0].resolved_internal_id.is_none());
    }

    #[test]
    fn external_qualified_call_never_resolves_to_same_name_local_function() {
        let caller = function("caller", "src/lib.rs", "function", 1);
        let local_spawn = function("spawn", "src/runtime.rs", "function", 5);
        let cs = CallSite {
            caller_id: Some(function_id(&caller)),
            caller_name: Some("caller".to_string()),
            callee: "tokio::spawn".to_string(),
            callee_base: "spawn".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/lib.rs".to_string(),
            line: 2,
            column: 0,
        };
        let resolved = resolve_call_targets(&[caller, local_spawn], &[cs]);
        assert!(resolved[0].resolved_internal_id.is_none());
    }

    #[test]
    fn module_qualified_call_resolves_only_matching_module() {
        let caller = function("caller", "src/lib.rs", "function", 1);
        let status_serve = function("serve", "src/status.rs", "function", 5);
        let edge_serve = function("serve", "src/edge.rs", "function", 5);
        let expected = function_id(&status_serve);
        let cs = CallSite {
            caller_id: Some(function_id(&caller)),
            caller_name: Some("caller".to_string()),
            callee: "status::serve".to_string(),
            callee_base: "serve".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/lib.rs".to_string(),
            line: 2,
            column: 0,
        };
        let resolved = resolve_call_targets(&[caller, status_serve, edge_serve], &[cs]);
        assert_eq!(resolved[0].resolved_internal_id.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn variable_method_call_prefers_method_over_free_function() {
        let caller = function("caller", "src/lib.rs", "function", 1);
        let free_refresh = function("refresh", "src/api.rs", "function", 5);
        let method_refresh = function("refresh", "src/state.rs", "method(Daemon)", 5);
        let expected = function_id(&method_refresh);
        let cs = CallSite {
            caller_id: Some(function_id(&caller)),
            caller_name: Some("caller".to_string()),
            callee: "daemon.refresh".to_string(),
            callee_base: "refresh".to_string(),
            call_kind: "method".to_string(),
            file_path: "src/lib.rs".to_string(),
            line: 2,
            column: 0,
        };
        let resolved = resolve_call_targets(
            &[caller, free_refresh, method_refresh],
            &[cs],
        );
        assert_eq!(resolved[0].resolved_internal_id.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn function_alias_call_resolves_to_the_original_function() {
        let caller = function("caller", "src/lib.rs", "function", 1);
        let original = function("handle_get", "src/inner.rs", "function", 5);
        let expected = function_id(&original);
        let cs = CallSite {
            caller_id: Some(function_id(&caller)),
            caller_name: Some("caller".to_string()),
            callee: "handle_session_get".to_string(),
            callee_base: "handle_get".to_string(),
            call_kind: "function_via_alias".to_string(),
            file_path: "src/lib.rs".to_string(),
            line: 2,
            column: 0,
        };
        let resolved = resolve_call_targets(&[caller, original], &[cs]);
        assert_eq!(resolved[0].resolved_internal_id.as_deref(), Some(expected.as_str()));
    }
}
