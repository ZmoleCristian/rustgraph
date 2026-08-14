//! Heuristics for picking a single function definition when multiple candidates share
//! the same bare name.

use super::super::{CallSite, FunctionInfo, function_id};
use super::module_paths::infer_file_crate_key;
use std::collections::HashMap;

pub(super) fn module_path_ends_with(full: &[String], suffix: &[String]) -> bool {
    if suffix.len() > full.len() {
        return false;
    }
    full[(full.len() - suffix.len())..] == suffix[..]
}

pub(super) fn normalize_callee_prefix(
    prefix: &[String],
    caller_module: Option<&[String]>,
) -> Option<Vec<String>> {
    if prefix.is_empty() {
        return None;
    }

    let mut prefix_parts = prefix.to_vec();

    if prefix_parts.first().map(|s| s.as_str()) == Some("crate") {
        prefix_parts.remove(0);
        return Some(prefix_parts);
    }

    if prefix_parts.first().map(|s| s.as_str()) == Some("self") {
        let caller = caller_module?;
        prefix_parts.remove(0);
        let mut out = caller.to_vec();
        out.extend(prefix_parts);
        return Some(out);
    }

    if prefix_parts.first().map(|s| s.as_str()) == Some("super") {
        let caller = caller_module?;
        let super_count = prefix_parts
            .iter()
            .take_while(|s| s.as_str() == "super")
            .count();
        if super_count > caller.len() {
            return None;
        }
        let mut out = caller[..caller.len() - super_count].to_vec();
        out.extend_from_slice(&prefix_parts[super_count..]);
        return Some(out);
    }

    Some(prefix_parts)
}

/// Attempt to resolve an ambiguous call to a single candidate function ID.
///
/// Resolution tries, in order: module-path prefix match, same-file match, and
/// same-crate match.  Returns `None` when ambiguity cannot be broken.
pub(crate) fn resolve_ambiguous_call_target(
    call_site: &CallSite,
    candidate_ids: &[String],
    function_by_id: &HashMap<String, &FunctionInfo>,
    module_lookup: &HashMap<String, Vec<String>>,
) -> Option<String> {
    if candidate_ids.is_empty() {
        return None;
    }
    if candidate_ids.len() == 1 {
        return Some(candidate_ids[0].clone());
    }

    let caller_module = call_site
        .caller_id
        .as_ref()
        .and_then(|id| module_lookup.get(id).cloned());

    let callee_segments: Vec<String> = call_site
        .callee
        .split("::")
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .collect();

    if callee_segments.len() >= 2 {
        let prefix = &callee_segments[..callee_segments.len() - 1];
        if let Some(normalized_prefix) = normalize_callee_prefix(prefix, caller_module.as_deref()) {
            let match_prefix = |target_prefix: &[String]| {
                let mut matched = Vec::new();
                for id in candidate_ids {
                    let func_module = module_lookup.get(id).cloned().unwrap_or_default();
                    let is_match = if target_prefix.is_empty() {
                        func_module.is_empty()
                    } else {
                        module_path_ends_with(&func_module, target_prefix)
                    };
                    if is_match {
                        matched.push(id.clone());
                    }
                }
                matched
            };

            let matched = match_prefix(&normalized_prefix);
            if matched.len() == 1 {
                return matched.into_iter().next();
            }

            let is_relative_prefix = prefix
                .first()
                .map(|s| !matches!(s.as_str(), "crate" | "self" | "super"))
                .unwrap_or(false);
            if is_relative_prefix && let Some(caller) = caller_module.as_deref() {
                let mut local_prefix = caller.to_vec();
                local_prefix.extend_from_slice(&normalized_prefix);
                let local_matched = match_prefix(&local_prefix);
                if local_matched.len() == 1 {
                    return local_matched.into_iter().next();
                }
            }

            if normalized_prefix.len() > 1 {
                let stripped: Vec<String> = normalized_prefix.iter().skip(1).cloned().collect();
                let stripped_matched = match_prefix(&stripped);
                if stripped_matched.len() == 1 {
                    return stripped_matched.into_iter().next();
                }
            }

            if let Some(last_seg) = normalized_prefix.last() {
                let suffix_matched: Vec<String> = candidate_ids
                    .iter()
                    .filter(|id| {
                        module_lookup
                            .get(*id)
                            .is_some_and(|m| m.last() == Some(last_seg))
                    })
                    .cloned()
                    .collect();
                if suffix_matched.len() == 1 {
                    return suffix_matched.into_iter().next();
                }
            }
        }
    }

    let mut same_file = candidate_ids
        .iter()
        .filter_map(|id| {
            function_by_id
                .get(id)
                .map(|function| (id.clone(), function.file_path.clone()))
        })
        .filter(|(_, path)| *path == call_site.file_path)
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    same_file.sort();
    same_file.dedup();
    if same_file.len() == 1 {
        return same_file.into_iter().next();
    }

    let caller_crate = infer_file_crate_key(&call_site.file_path);
    if !caller_crate.is_empty() {
        let mut same_crate = candidate_ids
            .iter()
            .filter_map(|id| {
                function_by_id
                    .get(id)
                    .map(|function| (id.clone(), infer_file_crate_key(&function.file_path)))
            })
            .filter(|(_, crate_key)| *crate_key == caller_crate)
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        same_crate.sort();
        same_crate.dedup();
        if same_crate.len() == 1 {
            return same_crate.into_iter().next();
        }
    }

    None
}

/// Like [`resolve_ambiguous_call_target`] but accepts `&FunctionInfo` slices
/// and returns `(file_path, start_line)` of the resolved function.
pub(crate) fn resolve_ambiguous_call_target_for_functions(
    call_site: &CallSite,
    candidates: &[&FunctionInfo],
    function_by_id: &HashMap<String, &FunctionInfo>,
    module_lookup: &HashMap<String, Vec<String>>,
) -> Option<(String, usize)> {
    let candidate_ids = candidates
        .iter()
        .map(|function| function_id(function))
        .collect::<Vec<_>>();
    let resolved_id =
        resolve_ambiguous_call_target(call_site, &candidate_ids, function_by_id, module_lookup)?;
    let resolved_function = function_by_id.get(&resolved_id)?;
    Some((
        resolved_function.file_path.clone(),
        resolved_function.start_line,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn module_path_ends_with_returns_true_for_matching_suffix() {
        assert!(module_path_ends_with(&s(&["a", "b", "c"]), &s(&["b", "c"])));
    }

    #[test]
    fn module_path_ends_with_returns_false_for_mismatch() {
        assert!(!module_path_ends_with(&s(&["a", "b", "c"]), &s(&["d"])));
    }

    #[test]
    fn module_path_ends_with_returns_false_when_suffix_too_long() {
        assert!(!module_path_ends_with(&s(&["a"]), &s(&["a", "b"])));
    }

    #[test]
    fn module_path_ends_with_handles_empty_suffix() {
        assert!(module_path_ends_with(&s(&["a"]), &s(&[])));
    }

    #[test]
    fn normalize_callee_prefix_returns_none_for_empty() {
        assert_eq!(normalize_callee_prefix(&[], None), None);
    }

    #[test]
    fn normalize_callee_prefix_strips_crate_keyword() {
        let out = normalize_callee_prefix(&s(&["crate", "api", "users"]), None);
        assert_eq!(out, Some(s(&["api", "users"])));
    }

    #[test]
    fn normalize_callee_prefix_resolves_self_against_caller() {
        let out = normalize_callee_prefix(&s(&["self", "child"]), Some(&s(&["api"])));
        assert_eq!(out, Some(s(&["api", "child"])));
    }

    #[test]
    fn normalize_callee_prefix_returns_none_for_self_without_caller() {
        let out = normalize_callee_prefix(&s(&["self", "child"]), None);
        assert_eq!(out, None);
    }

    #[test]
    fn normalize_callee_prefix_resolves_super_relative() {
        let out = normalize_callee_prefix(&s(&["super", "sib"]), Some(&s(&["api", "users"])));
        assert_eq!(out, Some(s(&["api", "sib"])));
    }

    #[test]
    fn normalize_callee_prefix_returns_none_for_too_many_super_segments() {
        let out = normalize_callee_prefix(&s(&["super", "super", "super", "x"]), Some(&s(&["a"])));
        assert_eq!(out, None);
    }

    #[test]
    fn normalize_callee_prefix_passes_through_relative_paths() {
        let out = normalize_callee_prefix(&s(&["other", "thing"]), Some(&s(&["api"])));
        assert_eq!(out, Some(s(&["other", "thing"])));
    }

    #[test]
    fn resolve_ambiguous_call_target_returns_single_candidate_directly() {
        let cs = CallSite {
            caller_id: None,
            caller_name: None,
            callee: "foo".to_string(),
            callee_base: "foo".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/x.rs".to_string(),
            line: 1,
            column: 0,
        };
        let fbi = HashMap::new();
        let ml = HashMap::new();
        let id = "src/y.rs:1:foo".to_string();
        let resolved = resolve_ambiguous_call_target(&cs, &[id.clone()], &fbi, &ml);
        assert_eq!(resolved, Some(id));
    }

    #[test]
    fn resolve_ambiguous_call_target_returns_none_for_empty_candidates() {
        let cs = CallSite {
            caller_id: None,
            caller_name: None,
            callee: "foo".to_string(),
            callee_base: "foo".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/x.rs".to_string(),
            line: 1,
            column: 0,
        };
        let fbi = HashMap::new();
        let ml = HashMap::new();
        assert_eq!(resolve_ambiguous_call_target(&cs, &[], &fbi, &ml), None);
    }
}
