//! Qualifier resolution: extracts type and module qualifiers from call expressions and
//! narrows candidate function lists using strict (`Type::method`) or loose (bare-name)
//! matching strategies.

use std::collections::HashMap;

use crate::FunctionInfo;


/// How a call site was resolved to a set of candidate functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionMode {
    /// Type-qualifier matching produced at least one candidate without fallback.
    Strict,

    /// Type-qualifier matching returned no candidates; bare-name fallback was used.
    FellBackToLoose,

    /// Type-qualifier matching returned no candidates and `strict_only` was set,
    /// so the call site was suppressed rather than falling back.
    StrictSuppressed,
}


/// Counters tracking how many call sites were resolved under each [`ResolutionMode`].
#[derive(Debug, Default, Clone, Copy)]
pub struct ResolutionStats {
    /// Number of call sites resolved in strict mode.
    pub strict: usize,
    /// Number of call sites that fell back to loose (bare-name) resolution.
    pub fell_back_to_loose: usize,
    /// Number of call sites suppressed by `--strict-resolution`.
    pub strict_suppressed: usize,
}

impl ResolutionStats {
    /// Increment the counter for the given `mode`.
    pub fn record(&mut self, mode: ResolutionMode) {
        match mode {
            ResolutionMode::Strict => self.strict += 1,
            ResolutionMode::FellBackToLoose => self.fell_back_to_loose += 1,
            ResolutionMode::StrictSuppressed => self.strict_suppressed += 1,
        }
    }


    /// Add all counters from `other` into `self`.
    pub fn merge(&mut self, other: &Self) {
        self.strict += other.strict;
        self.fell_back_to_loose += other.fell_back_to_loose;
        self.strict_suppressed += other.strict_suppressed;
    }


    /// Return the sum of all three counters.
    pub fn total(&self) -> usize {
        self.strict + self.fell_back_to_loose + self.strict_suppressed
    }


    /// Print a human-readable note to stderr when loose fallback or suppression occurred.
    pub fn emit_stderr_note(&self) {
        if self.fell_back_to_loose > 0 {
            let total = self.total();
            let pct = if total > 0 {
                (self.fell_back_to_loose * 100) / total
            } else {
                0
            };
            eprintln!(
                "note: {} / {} call sites ({}%) resolved by name when type-strict matching returned 0 candidates. This is normal for codebases with same-name functions across modules. Pass --strict-resolution to drop these (may hide some legit edges).",
                self.fell_back_to_loose, total, pct
            );
        }
        if self.strict_suppressed > 0 {
            eprintln!(
                "note: {} site(s) suppressed by --strict-resolution (would have used loose name resolution).",
                self.strict_suppressed
            );
        }
    }
}


/// Resolve a callee expression to a ranked candidate list.
///
/// Applies type-qualifier filtering, module-path narrowing, and method-preference
/// narrowing in order.  Falls back to bare-name candidates when strict resolution
/// yields nothing, unless `strict_only` is `true`.
pub fn resolve_callee_candidates<'a>(
    callee_name: &str,
    bare: &str,
    by_name: &'a HashMap<&str, Vec<&'a FunctionInfo>>,
    strict_only: bool,
) -> (Vec<&'a FunctionInfo>, ResolutionMode) {
    let Some(matches) = by_name.get(bare) else {

        return (Vec::new(), ResolutionMode::Strict);
    };
    let qualifier = qualifier_for_callee(callee_name);
    // Pre-format the two qualifier needles once outside the per-candidate
    // loop; without this, every `matches` entry re-allocates two strings.
    let needles = qualifier.as_deref().map(QualifierNeedles::new);
    let type_filtered: Vec<&FunctionInfo> = matches
        .iter()
        .copied()
        .filter(|m| match needles.as_ref() {
            Some(n) => candidate_matches_needles(m, n),
            None => true,
        })
        .collect();
    if type_filtered.is_empty() {

        return loose_fallback(matches, strict_only);
    }
    let module_narrowed = narrow_candidates_by_module_path(
        callee_name,
        &type_filtered,
        |f| f.file_path.as_str(),
    );
    let module_owned: Vec<&FunctionInfo> = module_narrowed.into_iter().copied().collect();
    if module_owned.is_empty() {


        return loose_fallback(matches, strict_only);
    }
    let method_narrowed = narrow_candidates_to_methods(
        callee_name,
        &module_owned,
        |f| f.kind.as_str(),
    );
    let final_owned: Vec<&FunctionInfo> = method_narrowed.into_iter().copied().collect();
    if final_owned.is_empty() {
        return loose_fallback(matches, strict_only);
    }
    (final_owned, ResolutionMode::Strict)
}


fn loose_fallback<'a>(
    bare_matches: &Vec<&'a FunctionInfo>,
    strict_only: bool,
) -> (Vec<&'a FunctionInfo>, ResolutionMode) {
    if strict_only {


        (Vec::new(), ResolutionMode::StrictSuppressed)
    } else {

        (bare_matches.iter().copied().collect(), ResolutionMode::FellBackToLoose)
    }
}


/// Extract the Pascal-case type qualifier from a callee expression, if present.
///
/// Returns `Some("Foo")` for `"Foo::method"` and `"obj.method"` where `obj`
/// starts with an uppercase letter; returns `None` for bare names, lowercase
/// module paths, or variable receivers.
pub fn qualifier_for_callee(callee_name: &str) -> Option<String> {
    if !callee_name.contains('.') && !callee_name.contains("::") {
        return None;
    }


    let last_dot = callee_name.rfind('.');
    let last_colon_pair = callee_name.rfind("::");
    let split_idx = match (last_dot, last_colon_pair) {
        (Some(d), Some(c)) => d.max(c),
        (Some(d), None) => d,
        (None, Some(c)) => c,
        (None, None) => return None,
    };
    let prefix = &callee_name[..split_idx];
    if prefix.is_empty() {
        return None;
    }

    let last_seg = prefix
        .rsplit(|c: char| c == '.' || c == ':')
        .find(|s| !s.is_empty())?;


    let first = last_seg.chars().next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    Some(last_seg.to_string())
}


/// Pre-formatted needles for matching a single qualifier against many
/// candidate functions. Build once outside the per-candidate loop, then
/// pass to [`candidate_matches_needles`] for each candidate.
pub struct QualifierNeedles<'a> {
    qualifier: &'a str,
    method_exact: String,
    trait_fn_exact: String,
}

impl<'a> QualifierNeedles<'a> {
    pub fn new(qualifier: &'a str) -> Self {
        Self {
            qualifier,
            method_exact: format!("method({qualifier})"),
            trait_fn_exact: format!("trait_fn({qualifier})"),
        }
    }
}

/// Variant of [`candidate_matches_qualifier`] that takes pre-formatted
/// needles. Used by hot loops that resolve many candidates against the same
/// qualifier; saves two `format!` allocations per candidate.
pub fn candidate_matches_needles(
    candidate: &FunctionInfo,
    needles: &QualifierNeedles<'_>,
) -> bool {
    let kind = candidate.kind.as_str();
    if kind == needles.method_exact || kind == needles.trait_fn_exact {
        return true;
    }
    if let Some(stripped) = kind.strip_prefix("method(").and_then(|s| s.strip_suffix(')')) {
        let bare_type = stripped.split('<').next().unwrap_or(stripped).trim();
        if bare_type == needles.qualifier {
            return true;
        }
    }
    if let Some(stripped) = kind.strip_prefix("trait_fn(").and_then(|s| s.strip_suffix(')')) {
        let bare_type = stripped.split('<').next().unwrap_or(stripped).trim();
        if bare_type == needles.qualifier {
            return true;
        }
    }
    false
}

/// Return `true` if `candidate`'s kind matches the given type qualifier.
///
/// Accepts `None` (any function passes) or a Pascal-case type name.  Generics
/// in the kind string (e.g. `method(Wrapper<T>)`) are stripped before comparison.
///
/// For per-candidate-in-a-loop callers, prefer building [`QualifierNeedles`]
/// once and using [`candidate_matches_needles`] to avoid repeated `format!`.
pub fn candidate_matches_qualifier(candidate: &FunctionInfo, qualifier: Option<&str>) -> bool {
    let Some(q) = qualifier else { return true };
    let needles = QualifierNeedles::new(q);
    candidate_matches_needles(candidate, &needles)
}


/// Extract lowercase module-path segments from a `::` qualified callee name.
///
/// Returns `None` for bare names, dotted receivers, or paths whose last
/// non-function segment is Pascal-case (type-qualified calls are handled by
/// [`qualifier_for_callee`]).
pub fn module_path_qualifier(callee_name: &str) -> Option<Vec<String>> {
    if callee_name.contains('.') {
        return None;
    }
    let last_colon_pair = callee_name.rfind("::")?;
    let prefix = &callee_name[..last_colon_pair];
    if prefix.is_empty() {
        return None;
    }

    let segments: Vec<String> = prefix
        .split("::")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if segments.is_empty() {
        return None;
    }


    let last = segments.last()?;
    if last.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return None;
    }
    Some(segments)
}


/// Retain only the candidates whose file path matches the module-path qualifier of `callee_name`.
///
/// Returns the full `candidates` slice unchanged when there is no qualifier, when
/// fewer than two candidates exist, or when no candidates match the qualifier.
pub fn narrow_candidates_by_module_path<'a, T, F>(
    callee_name: &str,
    candidates: &'a [T],
    path_of: F,
) -> Vec<&'a T>
where
    F: Fn(&T) -> &str,
{
    let segments = match module_path_qualifier(callee_name) {
        Some(s) => s,
        None => return candidates.iter().collect(),
    };
    if candidates.len() <= 1 {
        return candidates.iter().collect();
    }

    let useful: Vec<&str> = segments
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !matches!(*s, "crate" | "self" | "super"))
        .collect();
    if useful.is_empty() {
        return candidates.iter().collect();
    }

    let last = useful.last().copied().unwrap();
    let needle_slash = format!("/{}/", last);
    let needle_slash_alt = format!("/{}.", last);
    let narrowed: Vec<&T> = candidates
        .iter()
        .filter(|c| {
            let path = path_of(c);
            path.contains(&needle_slash) || path.contains(&needle_slash_alt)
        })
        .collect();
    if narrowed.is_empty() {


        return candidates.iter().collect();
    }
    narrowed
}


/// Return `true` when the callee's receiver looks like a variable (lowercase or `_`) rather
/// than a type, indicating that method candidates should be preferred over free functions.
pub fn prefer_methods_for_dotted(callee_name: &str) -> bool {
    let Some(dot_idx) = callee_name.rfind('.') else {
        return false;
    };
    let receiver = &callee_name[..dot_idx];
    if receiver.is_empty() {
        return false;
    }

    let last_seg = receiver
        .rsplit(|c: char| c == ':' || c == '.')
        .find(|s| !s.is_empty())
        .unwrap_or(receiver);
    if last_seg == "receiver" {


        return true;
    }
    let first = last_seg.chars().next();
    matches!(first, Some(c) if c.is_ascii_lowercase() || c == '_')
}


/// Retain only method/trait-function candidates when the callee expression uses a
/// variable receiver (as determined by [`prefer_methods_for_dotted`]).
///
/// Returns the full slice unchanged when narrowing is not applicable or when no
/// method candidates are present.
pub fn narrow_candidates_to_methods<'a, T, F>(
    callee_name: &str,
    candidates: &'a [T],
    kind_of: F,
) -> Vec<&'a T>
where
    F: Fn(&T) -> &str,
{
    if !prefer_methods_for_dotted(callee_name) {
        return candidates.iter().collect();
    }
    if candidates.len() <= 1 {
        return candidates.iter().collect();
    }
    let methods: Vec<&T> = candidates
        .iter()
        .filter(|c| {
            let kind = kind_of(c);
            kind.starts_with("method(") || kind.starts_with("trait_fn(")
        })
        .collect();
    if methods.is_empty() {

        return candidates.iter().collect();
    }
    methods
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fn_with_kind(name: &str, kind: &str) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}"),
            file_path: "src/x.rs".to_string(),
            start_line: 1,
            end_line: 1,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: vec![],
            parameters: vec![],
            return_type: None,
            kind: kind.to_string(),
            cfg_attrs: Vec::new(),
        }
    }


    #[test]
    fn qualifier_for_type_colon_method() {
        assert_eq!(qualifier_for_callee("Daemon::run_cli"), Some("Daemon".to_string()));
    }

    #[test]
    fn qualifier_for_type_dot_method() {

        assert_eq!(qualifier_for_callee("Daemon.run_cli"), Some("Daemon".to_string()));
    }

    #[test]
    fn qualifier_for_module_path_picks_last_segment() {


        assert_eq!(qualifier_for_callee("module::A::new"), Some("A".to_string()));
    }

    #[test]
    fn qualifier_for_lowercase_module_returns_none() {


        assert_eq!(qualifier_for_callee("std::mem::take"), None);
    }

    #[test]
    fn qualifier_for_variable_receiver_returns_none() {

        assert_eq!(qualifier_for_callee("obj.foo"), None);
    }

    #[test]
    fn qualifier_for_complex_receiver_returns_none() {

        assert_eq!(qualifier_for_callee("receiver.method"), None);
    }

    #[test]
    fn qualifier_for_bare_name_returns_none() {

        assert_eq!(qualifier_for_callee("foo"), None);
    }

    #[test]
    fn qualifier_for_generics_in_type_segment() {


        assert_eq!(
            qualifier_for_callee("Wrapper<'a, T>::method"),
            Some("Wrapper<'a, T>".to_string())
        );
    }

    #[test]
    fn qualifier_for_empty_prefix_returns_none() {

        assert_eq!(qualifier_for_callee("::foo"), None);
    }


    #[test]
    fn candidate_matches_when_qualifier_is_none() {

        let f = fn_with_kind("foo", "function");
        assert!(candidate_matches_qualifier(&f, None));
    }

    #[test]
    fn candidate_matches_method_of_type() {
        let f = fn_with_kind("new", "method(Daemon)");
        assert!(candidate_matches_qualifier(&f, Some("Daemon")));
    }

    #[test]
    fn candidate_matches_trait_fn_of_type() {
        let f = fn_with_kind("from", "trait_fn(MyTrait)");
        assert!(candidate_matches_qualifier(&f, Some("MyTrait")));
    }

    #[test]
    fn candidate_rejects_method_of_other_type() {


        let f = fn_with_kind("new", "method(Session)");
        assert!(!candidate_matches_qualifier(&f, Some("Daemon")));
    }

    #[test]
    fn candidate_rejects_free_function_when_qualified() {


        let f = fn_with_kind("compute", "function");
        assert!(!candidate_matches_qualifier(&f, Some("Foo")));
    }

    #[test]
    fn candidate_matches_generics_method_kind() {

        let f = fn_with_kind("get", "method(Wrapper<'a, T>)");
        assert!(candidate_matches_qualifier(&f, Some("Wrapper")));
    }

    #[test]
    fn candidate_matches_generics_trait_fn_kind() {
        let f = fn_with_kind("convert", "trait_fn(Trait<T>)");
        assert!(candidate_matches_qualifier(&f, Some("Trait")));
    }

    #[test]
    fn candidate_rejects_generic_when_bare_type_name_differs() {
        let f = fn_with_kind("get", "method(Other<T>)");
        assert!(!candidate_matches_qualifier(&f, Some("Wrapper")));
    }

    #[test]
    fn candidate_matches_needles_matches_method_exact() {
        let needles = QualifierNeedles::new("Daemon");
        let f = fn_with_kind("new", "method(Daemon)");
        assert!(candidate_matches_needles(&f, &needles));
    }

    #[test]
    fn candidate_matches_needles_matches_trait_fn_exact() {
        let needles = QualifierNeedles::new("MyTrait");
        let f = fn_with_kind("from", "trait_fn(MyTrait)");
        assert!(candidate_matches_needles(&f, &needles));
    }

    #[test]
    fn candidate_matches_needles_strips_generics() {
        let needles = QualifierNeedles::new("Wrapper");
        let f = fn_with_kind("get", "method(Wrapper<'a, T>)");
        assert!(candidate_matches_needles(&f, &needles));
    }

    #[test]
    fn candidate_matches_needles_rejects_other_type() {
        let needles = QualifierNeedles::new("Daemon");
        let f = fn_with_kind("new", "method(Session)");
        assert!(!candidate_matches_needles(&f, &needles));
    }

    #[test]
    fn candidate_matches_qualifier_still_matches_needles_form() {
        // Equivalence check: the public API and the inner needles helper must
        // agree for every candidate kind we care about.
        let cases = [
            ("function", "Daemon", false),
            ("method(Daemon)", "Daemon", true),
            ("method(Session)", "Daemon", false),
            ("trait_fn(MyTrait)", "MyTrait", true),
            ("method(Wrapper<'a, T>)", "Wrapper", true),
        ];
        for (kind, q, expected) in cases {
            let f = fn_with_kind("x", kind);
            let needles = QualifierNeedles::new(q);
            assert_eq!(
                candidate_matches_qualifier(&f, Some(q)),
                expected,
                "case: kind={kind}, q={q}"
            );
            assert_eq!(
                candidate_matches_needles(&f, &needles),
                expected,
                "case: kind={kind}, q={q}"
            );
        }
    }


    #[test]
    fn module_path_qualifier_extracts_lowercase_segments() {
        assert_eq!(
            module_path_qualifier("session::discovery::normalize_worktree_path"),
            Some(vec!["session".to_string(), "discovery".to_string()])
        );
    }

    #[test]
    fn module_path_qualifier_returns_none_for_bare() {
        assert_eq!(module_path_qualifier("foo"), None);
    }

    #[test]
    fn module_path_qualifier_returns_none_for_pascal_last_segment() {

        assert_eq!(module_path_qualifier("module::Type::method"), None);
    }

    #[test]
    fn module_path_qualifier_returns_none_for_dotted_receiver() {
        assert_eq!(module_path_qualifier("obj.method"), None);
    }

    #[test]
    fn module_path_qualifier_handles_single_segment() {
        assert_eq!(
            module_path_qualifier("module::name"),
            Some(vec!["module".to_string()])
        );
    }

    #[test]
    fn module_path_qualifier_skips_empty_prefix() {

        assert_eq!(module_path_qualifier("::foo"), None);
    }


    fn fn_with_path(name: &str, file_path: &str) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}"),
            file_path: file_path.to_string(),
            start_line: 1,
            end_line: 1,
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
    fn narrow_keeps_candidate_whose_path_contains_segment() {
        let cands = vec![
            fn_with_path("foo", "src/session/discovery/identity.rs"),
            fn_with_path("foo", "src/daemon/http/endpoints/session.rs"),
        ];
        let kept = narrow_candidates_by_module_path("session::discovery::foo", &cands, |f| {
            f.file_path.as_str()
        });
        assert_eq!(kept.len(), 1, "expected to keep only the discovery match");
        assert!(kept[0].file_path.contains("/discovery/"));
    }

    #[test]
    fn narrow_returns_all_when_qualifier_doesnt_match_any_path() {
        let cands = vec![
            fn_with_path("foo", "src/api.rs"),
            fn_with_path("foo", "src/lib.rs"),
        ];
        let kept = narrow_candidates_by_module_path("nonexistent::foo", &cands, |f| {
            f.file_path.as_str()
        });

        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn narrow_is_noop_for_bare_name() {
        let cands = vec![
            fn_with_path("foo", "src/a.rs"),
            fn_with_path("foo", "src/b.rs"),
        ];
        let kept = narrow_candidates_by_module_path("foo", &cands, |f| f.file_path.as_str());
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn narrow_is_noop_for_single_candidate() {
        let cands = vec![fn_with_path("foo", "src/a.rs")];
        let kept = narrow_candidates_by_module_path("module::foo", &cands, |f| {
            f.file_path.as_str()
        });
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn narrow_skips_crate_prefix() {

        let cands = vec![
            fn_with_path("foo", "src/a.rs"),
            fn_with_path("foo", "src/b.rs"),
        ];
        let kept = narrow_candidates_by_module_path("crate::foo", &cands, |f| {
            f.file_path.as_str()
        });

        assert_eq!(kept.len(), 2);
    }


    #[test]
    fn prefer_methods_for_lowercase_variable_receiver() {
        assert!(prefer_methods_for_dotted("daemon_state.refresh_project"));
    }

    #[test]
    fn prefer_methods_for_underscore_receiver() {
        assert!(prefer_methods_for_dotted("_state.refresh"));
    }

    #[test]
    fn prefer_methods_for_complex_receiver_sentinel() {

        assert!(prefer_methods_for_dotted("receiver.method"));
    }

    #[test]
    fn prefer_methods_skips_pascal_receiver() {

        assert!(!prefer_methods_for_dotted("Daemon.method"));
    }

    #[test]
    fn prefer_methods_skips_bare_name() {
        assert!(!prefer_methods_for_dotted("foo"));
    }

    #[test]
    fn prefer_methods_skips_module_path() {
        assert!(!prefer_methods_for_dotted("module::foo"));
    }

    #[test]
    fn prefer_methods_handles_module_qualified_variable() {


        assert!(prefer_methods_for_dotted("crate::state.refresh"));
    }


    #[test]
    fn narrow_to_methods_keeps_method_when_free_fn_present() {
        let cands = vec![
            fn_with_kind("refresh", "function"),
            fn_with_kind("refresh", "method(Daemon)"),
        ];
        let kept = narrow_candidates_to_methods("daemon_state.refresh", &cands, |f| {
            f.kind.as_str()
        });
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].kind, "method(Daemon)");
    }

    #[test]
    fn narrow_to_methods_returns_all_when_no_method_candidate() {


        let cands = vec![
            fn_with_kind("refresh", "function"),
            fn_with_kind("refresh", "function"),
        ];
        let kept = narrow_candidates_to_methods("obj.refresh", &cands, |f| f.kind.as_str());
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn narrow_to_methods_is_noop_for_bare_name() {
        let cands = vec![
            fn_with_kind("refresh", "function"),
            fn_with_kind("refresh", "method(Daemon)"),
        ];
        let kept = narrow_candidates_to_methods("refresh", &cands, |f| f.kind.as_str());
        assert_eq!(kept.len(), 2, "bare name → no narrowing, both kept");
    }

    #[test]
    fn narrow_to_methods_includes_trait_fn() {
        let cands = vec![
            fn_with_kind("from", "function"),
            fn_with_kind("from", "trait_fn(MyTrait)"),
        ];
        let kept = narrow_candidates_to_methods("obj.from", &cands, |f| f.kind.as_str());
        assert_eq!(kept.len(), 1);
        assert!(kept[0].kind.starts_with("trait_fn("));
    }


    fn fn_with_kind_and_path(name: &str, kind: &str, file_path: &str) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}"),
            file_path: file_path.to_string(),
            start_line: 1,
            end_line: 1,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: vec![],
            parameters: vec![],
            return_type: None,
            kind: kind.to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    fn build_index<'a>(fns: &'a [FunctionInfo]) -> HashMap<&'a str, Vec<&'a FunctionInfo>> {
        let mut m: HashMap<&str, Vec<&FunctionInfo>> = HashMap::new();
        for f in fns {
            m.entry(f.name.as_str()).or_default().push(f);
        }
        m
    }

    #[test]
    fn resolve_strict_when_type_qualifier_matches() {

        let fns = vec![
            fn_with_kind_and_path("new", "method(Daemon)", "src/daemon.rs"),
            fn_with_kind_and_path("new", "method(Session)", "src/session.rs"),
        ];
        let by_name = build_index(&fns);
        let (kept, mode) = resolve_callee_candidates("Daemon::new", "new", &by_name, false);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].kind, "method(Daemon)");
        assert_eq!(mode, ResolutionMode::Strict);
    }

    #[test]
    fn resolve_falls_back_when_strict_zeroes() {


        let fns = vec![
            fn_with_kind_and_path("foo", "function", "src/util.rs"),
        ];
        let by_name = build_index(&fns);


        let (kept, mode) = resolve_callee_candidates("Bar::foo", "foo", &by_name, false);


        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].kind, "function");
        assert_eq!(mode, ResolutionMode::FellBackToLoose);
    }

    #[test]
    fn resolve_strict_only_suppresses_fallback() {


        let fns = vec![
            fn_with_kind_and_path("foo", "function", "src/util.rs"),
        ];
        let by_name = build_index(&fns);
        let (kept, mode) = resolve_callee_candidates("Bar::foo", "foo", &by_name, true);
        assert!(kept.is_empty(), "strict_only should produce empty result");
        assert_eq!(mode, ResolutionMode::StrictSuppressed);
    }

    #[test]
    fn resolve_no_bare_match_returns_strict_empty() {


        let fns: Vec<FunctionInfo> = vec![];
        let by_name = build_index(&fns);
        let (kept, mode) = resolve_callee_candidates("foo", "foo", &by_name, false);
        assert!(kept.is_empty());
        assert_eq!(mode, ResolutionMode::Strict);
    }

    #[test]
    fn resolve_dotted_method_preference_strict_succeeds() {


        let fns = vec![
            fn_with_kind_and_path("refresh", "function", "src/api.rs"),
            fn_with_kind_and_path("refresh", "method(Daemon)", "src/daemon.rs"),
        ];
        let by_name = build_index(&fns);
        let (kept, mode) = resolve_callee_candidates("obj.refresh", "refresh", &by_name, false);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].kind, "method(Daemon)");
        assert_eq!(mode, ResolutionMode::Strict);
    }

    #[test]
    fn resolve_bare_name_strict_returns_all() {


        let fns = vec![
            fn_with_kind_and_path("foo", "function", "src/a.rs"),
            fn_with_kind_and_path("foo", "function", "src/b.rs"),
        ];
        let by_name = build_index(&fns);
        let (kept, mode) = resolve_callee_candidates("foo", "foo", &by_name, false);
        assert_eq!(kept.len(), 2);
        assert_eq!(mode, ResolutionMode::Strict);
    }


    #[test]
    fn stats_record_increments_per_mode() {
        let mut s = ResolutionStats::default();
        s.record(ResolutionMode::Strict);
        s.record(ResolutionMode::Strict);
        s.record(ResolutionMode::FellBackToLoose);
        s.record(ResolutionMode::StrictSuppressed);
        assert_eq!(s.strict, 2);
        assert_eq!(s.fell_back_to_loose, 1);
        assert_eq!(s.strict_suppressed, 1);
    }

    #[test]
    fn stats_merge_sums_counters() {
        let mut a = ResolutionStats { strict: 1, fell_back_to_loose: 2, strict_suppressed: 3 };
        let b = ResolutionStats { strict: 10, fell_back_to_loose: 20, strict_suppressed: 30 };
        a.merge(&b);
        assert_eq!(a.strict, 11);
        assert_eq!(a.fell_back_to_loose, 22);
        assert_eq!(a.strict_suppressed, 33);
    }
}
