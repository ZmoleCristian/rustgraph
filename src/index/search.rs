//! Fuzzy and exact symbol search across functions, structs, and enums.
//!
//! All public entry points delegate to the internal `search_items_full` pipeline
//! which scores candidates by name, with optional signature/path fallback.

use super::{EnumInfo, FunctionInfo, StructInfo};


/// Which field of a symbol record caused it to match a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    /// The symbol name matched the query.
    Name,
    /// The full signature matched the query (fallback when name did not match).
    Sig,
    /// The file path matched the query (fallback when neither name nor signature matched).
    Path,
}

impl MatchKind {
    /// Human-readable bracketed tag for display (e.g. `"[name]"`).
    pub fn tag(self) -> &'static str {
        match self {
            MatchKind::Name => "[name]",
            MatchKind::Sig => "[sig]",
            MatchKind::Path => "[path]",
        }
    }


    /// Lower-case string identifier (e.g. `"name"`, `"sig"`, `"path"`).
    pub fn as_str(self) -> &'static str {
        match self {
            MatchKind::Name => "name",
            MatchKind::Sig => "sig",
            MatchKind::Path => "path",
        }
    }
}

/// Compute the Levenshtein edit distance between two strings.
///
/// Implemented with a rolling-row dynamic-programming table: only the previous
/// and current rows are kept in memory, giving O(n) space instead of O(m*n).
pub fn edit_distance(s1: &str, s2: &str) -> usize {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    let m = s1_chars.len();
    let n = s2_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            curr[j] = if s1_chars[i - 1] == s2_chars[j - 1] {
                prev[j - 1]
            } else {
                1 + std::cmp::min(std::cmp::min(prev[j], curr[j - 1]), prev[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

/// Return `true` if `query` is a substring of `target` or if their edit-distance similarity
/// meets `threshold`.
///
/// Comparison is case-insensitive.
pub fn fuzzy_match(query: &str, target: &str, threshold: f64) -> bool {
    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();

    if target_lower.contains(&query_lower) {
        return true;
    }

    let distance = edit_distance(&query_lower, &target_lower);
    let max_len = std::cmp::max(query_lower.len(), target_lower.len());

    if max_len == 0 {
        return true;
    }

    let similarity = 1.0 - (distance as f64 / max_len as f64);
    similarity >= threshold
}

/// Return a similarity score in [0, 1] between `query` and `target`.
///
/// Returns `1.0` when `query` is a substring of `target` (case-insensitive),
/// otherwise falls back to edit-distance normalised by the longer string.
pub fn fuzzy_similarity(query: &str, target: &str) -> f64 {
    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();

    if target_lower.contains(&query_lower) {
        return 1.0;
    }

    let distance = edit_distance(&query_lower, &target_lower);
    let max_len = std::cmp::max(query_lower.len(), target_lower.len());

    if max_len == 0 {
        return 1.0;
    }

    1.0 - (distance as f64 / max_len as f64)
}


/// Return the threshold that should actually be applied given `query`.
///
/// Short single-term queries (≤ 3 characters) are clamped to `1.0` to prevent
/// low-quality substring explosions; multi-term `|`-separated queries always
/// use the caller-supplied threshold.
pub fn effective_threshold(query: &str, requested_threshold: f64) -> f64 {
    let trimmed = query.trim();
    let multi_term = trimmed.contains('|');
    let alts: Vec<&str> = trimmed
        .split('|')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let short_query = !multi_term && alts.iter().any(|a| a.len() <= 3);
    if short_query { 1.0_f64.max(requested_threshold) } else { requested_threshold }
}

/// Search functions, structs, and enums by name using fuzzy matching.
///
/// `search_terms` may be `|`-separated to match any of several names.
/// Returns only the matched items without match-kind annotations.
pub fn search_items(
    functions: &[FunctionInfo],
    structs: &[StructInfo],
    enums: &[EnumInfo],
    search_terms: &str,
    threshold: f64,
) -> (Vec<FunctionInfo>, Vec<StructInfo>, Vec<EnumInfo>) {
    search_items_with_options(functions, structs, enums, search_terms, threshold, false)
}


/// Like [`search_items`] with an additional `match_signature` flag.
///
/// When `match_signature` is `true`, items that did not match by name are also
/// checked against their signature and file path.
pub fn search_items_with_options(
    functions: &[FunctionInfo],
    structs: &[StructInfo],
    enums: &[EnumInfo],
    search_terms: &str,
    threshold: f64,
    match_signature: bool,
) -> (Vec<FunctionInfo>, Vec<StructInfo>, Vec<EnumInfo>) {
    let (fns, structs_out, enums_out, _fallback) = search_items_full(
        functions,
        structs,
        enums,
        search_terms,
        threshold,
        match_signature,
        false,
    );
    (
        fns.into_iter().map(|(f, _)| f).collect(),
        structs_out.into_iter().map(|(s, _)| s).collect(),
        enums_out.into_iter().map(|(e, _)| e).collect(),
    )
}


/// Like [`search_items_with_options`] but returns each item paired with its [`MatchKind`].
pub fn search_items_with_kinds(
    functions: &[FunctionInfo],
    structs: &[StructInfo],
    enums: &[EnumInfo],
    search_terms: &str,
    threshold: f64,
    match_signature: bool,
) -> (
    Vec<(FunctionInfo, MatchKind)>,
    Vec<(StructInfo, MatchKind)>,
    Vec<(EnumInfo, MatchKind)>,
) {
    let (fns, structs_out, enums_out, _fallback) = search_items_full(
        functions,
        structs,
        enums,
        search_terms,
        threshold,
        match_signature,
        false,
    );
    (fns, structs_out, enums_out)
}


/// Like [`search_items_with_kinds`] but also returns a `bool` indicating whether a
/// threshold fallback was triggered (i.e. the effective threshold was relaxed).
pub fn search_items_with_kinds_and_fallback(
    functions: &[FunctionInfo],
    structs: &[StructInfo],
    enums: &[EnumInfo],
    search_terms: &str,
    threshold: f64,
    match_signature: bool,
) -> (
    Vec<(FunctionInfo, MatchKind)>,
    Vec<(StructInfo, MatchKind)>,
    Vec<(EnumInfo, MatchKind)>,
    bool,
) {
    search_items_full(
        functions,
        structs,
        enums,
        search_terms,
        threshold,
        match_signature,
        false,
    )
}


/// Exact case-insensitive name search: only items whose lowercased name equals a search term.
pub fn search_items_exact(
    functions: &[FunctionInfo],
    structs: &[StructInfo],
    enums: &[EnumInfo],
    search_terms: &str,
) -> (Vec<FunctionInfo>, Vec<StructInfo>, Vec<EnumInfo>) {
    let (fns, structs_out, enums_out, _fallback) =
        search_items_full(functions, structs, enums, search_terms, 1.0, false, true);
    (
        fns.into_iter().map(|(f, _)| f).collect(),
        structs_out.into_iter().map(|(s, _)| s).collect(),
        enums_out.into_iter().map(|(e, _)| e).collect(),
    )
}


/// Like [`search_items_exact`] but returns each item paired with its [`MatchKind`].
pub fn search_items_exact_with_kinds(
    functions: &[FunctionInfo],
    structs: &[StructInfo],
    enums: &[EnumInfo],
    search_terms: &str,
) -> (
    Vec<(FunctionInfo, MatchKind)>,
    Vec<(StructInfo, MatchKind)>,
    Vec<(EnumInfo, MatchKind)>,
) {
    let (fns, structs_out, enums_out, _fallback) =
        search_items_full(functions, structs, enums, search_terms, 1.0, false, true);
    (fns, structs_out, enums_out)
}


fn search_items_full(
    functions: &[FunctionInfo],
    structs: &[StructInfo],
    enums: &[EnumInfo],
    search_terms: &str,
    threshold: f64,
    match_signature: bool,
    exact_only: bool,
) -> (
    Vec<(FunctionInfo, MatchKind)>,
    Vec<(StructInfo, MatchKind)>,
    Vec<(EnumInfo, MatchKind)>,
    bool,
) {
    let terms: Vec<String> = search_terms
        .split('|')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();


    let mut fallback_fired = false;
    let fns = match_kind(
        functions,
        &terms,
        search_terms,
        threshold,
        match_signature,
        exact_only,
        &mut fallback_fired,
        |f| f.name.as_str(),
        |f| f.signature.as_str(),
        |f| f.file_path.as_str(),
        |f| f.start_line,
    );
    let structs_out = match_kind(
        structs,
        &terms,
        search_terms,
        threshold,
        match_signature,
        exact_only,
        &mut fallback_fired,
        |s| s.name.as_str(),
        |s| s.signature.as_str(),
        |s| s.file_path.as_str(),
        |s| s.start_line,
    );
    let enums_out = match_kind(
        enums,
        &terms,
        search_terms,
        threshold,
        match_signature,
        exact_only,
        &mut fallback_fired,
        |e| e.name.as_str(),
        |e| e.signature.as_str(),
        |e| e.file_path.as_str(),
        |e| e.start_line,
    );


    let any_hits = !fns.is_empty() || !structs_out.is_empty() || !enums_out.is_empty();
    let fallback = fallback_fired && any_hits;

    (fns, structs_out, enums_out, fallback)
}

fn match_kind<T: Clone>(
    items: &[T],
    terms: &[String],
    full_query: &str,
    threshold: f64,
    match_signature: bool,
    exact_only: bool,
    fallback_fired: &mut bool,
    name_of: impl Fn(&T) -> &str,
    sig_of: impl Fn(&T) -> &str,
    path_of: impl Fn(&T) -> &str,
    start_line_of: impl Fn(&T) -> usize,
) -> Vec<(T, MatchKind)> {


    if exact_only {
        return items
            .iter()
            .filter(|it| {
                let name = name_of(it).to_lowercase();
                terms.iter().any(|t| t == &name)
            })
            .cloned()
            .map(|it| (it, MatchKind::Name))
            .collect();
    }


    let effective_threshold = effective_threshold(full_query, threshold);
    let mut scored: Vec<(f64, T)> = items
        .iter()
        .filter_map(|it| {
            let name = name_of(it);
            let best = terms
                .iter()
                .filter_map(|t| name_score(t, name, effective_threshold))
                .reduce(f64::max);
            best.map(|score| (score, it.clone()))
        })
        .collect();


    if scored.is_empty() && effective_threshold > threshold {
        let rescued: Vec<(f64, T)> = items
            .iter()
            .filter_map(|it| {
                let name = name_of(it);
                let best = terms
                    .iter()
                    .filter_map(|t| name_score(t, name, threshold))
                    .reduce(f64::max);
                best.map(|score| (score, it.clone()))
            })
            .collect();
        if !rescued.is_empty() {
            *fallback_fired = true;
            scored = rescued;
        }
    }
    let mut name_results: Vec<(T, MatchKind)> = if !scored.is_empty() {
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));


        let multi_term = terms.len() > 1;
        if !multi_term && scored.first().map(|(s, _)| *s) == Some(1.0) {
            scored
                .into_iter()
                .filter(|(s, _)| *s == 1.0)
                .map(|(_, it)| (it, MatchKind::Name))
                .collect()
        } else {
            scored
                .into_iter()
                .map(|(_, it)| (it, MatchKind::Name))
                .collect()
        }
    } else {
        Vec::new()
    };
    if !match_signature {
        return name_results;
    }


    let mut seen: std::collections::HashSet<(String, String, usize)> = name_results
        .iter()
        .map(|(it, _)| {
            (
                name_of(it).to_string(),
                path_of(it).to_string(),
                start_line_of(it),
            )
        })
        .collect();
    for it in items.iter() {
        let key = (
            name_of(it).to_string(),
            path_of(it).to_string(),
            start_line_of(it),
        );
        if seen.contains(&key) {
            continue;
        }
        let sig_hit = terms.iter().any(|t| fuzzy_match(t, sig_of(it), threshold));
        let path_hit = terms.iter().any(|t| fuzzy_match(t, path_of(it), threshold));
        if sig_hit {
            name_results.push((it.clone(), MatchKind::Sig));
            seen.insert(key);
        } else if path_hit {
            name_results.push((it.clone(), MatchKind::Path));
            seen.insert(key);
        }
    }
    name_results
}


/// Score `query` against a symbol `name` and return the score if it meets `threshold`.
///
/// Priority order: exact match (1.0) > prefix match (≥ 0.95) > substring match (≥ 0.85)
/// > edit-distance similarity.  Returns `None` when below `threshold`.
pub fn name_score(query: &str, name: &str, threshold: f64) -> Option<f64> {
    let q = query.to_lowercase();
    let n = name.to_lowercase();
    if n.is_empty() && q.is_empty() {
        return Some(1.0);
    }
    if n == q {
        return Some(1.0);
    }
    let score = if !n.is_empty() && n.starts_with(&q) {
        0.95 + (q.len() as f64 / n.len() as f64) * 0.05
    } else if !n.is_empty() && n.contains(&q) {
        0.85 + (q.len() as f64 / n.len() as f64) * 0.05
    } else {
        fuzzy_similarity(&q, &n)
    };
    if score >= threshold { Some(score) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn func(name: &str, signature: &str) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: signature.to_string(),
            file_path: format!("src/{name}.rs"),
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

    fn s(name: &str) -> StructInfo {
        StructInfo {
            name: name.to_string(),
            signature: format!("pub struct {name}"),
            file_path: format!("src/{name}.rs"),
            start_line: 1,
            end_line: 1,
            is_pub: true,
            generics: vec![],
            fields: vec![],
            kind: "struct".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    fn e(name: &str) -> EnumInfo {
        EnumInfo {
            name: name.to_string(),
            signature: format!("pub enum {name}"),
            file_path: format!("src/{name}.rs"),
            start_line: 1,
            end_line: 1,
            is_pub: true,
            generics: vec![],
            variants: vec![],
            kind: "enum".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    #[test]
    fn edit_distance_handles_identical_strings() {
        assert_eq!(edit_distance("hello", "hello"), 0);
    }

    #[test]
    fn edit_distance_handles_empty_inputs() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn edit_distance_counts_substitutions_and_indels() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("abc", "abd"), 1);
    }

    #[test]
    fn edit_distance_supports_unicode_chars() {
        assert_eq!(edit_distance("café", "cafe"), 1);
    }


    /// Reference 2D Levenshtein implementation, used only by the
    /// rolling-row parity test below.
    fn edit_distance_2d(s1: &str, s2: &str) -> usize {
        let s1_chars: Vec<char> = s1.chars().collect();
        let s2_chars: Vec<char> = s2.chars().collect();
        let m = s1_chars.len();
        let n = s2_chars.len();
        let mut dp = vec![vec![0usize; n + 1]; m + 1];
        for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
            row[0] = i;
        }
        for (j, cell) in dp[0].iter_mut().enumerate().take(n + 1) {
            *cell = j;
        }
        for i in 1..=m {
            for j in 1..=n {
                if s1_chars[i - 1] == s2_chars[j - 1] {
                    dp[i][j] = dp[i - 1][j - 1];
                } else {
                    dp[i][j] = 1 + std::cmp::min(
                        std::cmp::min(dp[i - 1][j], dp[i][j - 1]),
                        dp[i - 1][j - 1],
                    );
                }
            }
        }
        dp[m][n]
    }

    #[test]
    fn edit_distance_rolling_row_matches_2d_reference() {
        let pairs = [
            ("", ""),
            ("", "abc"),
            ("abc", ""),
            ("kitten", "sitting"),
            ("flaw", "lawn"),
            ("intention", "execution"),
            ("café", "cafe"),
            ("rustgraph", "rustacean"),
            ("a", "b"),
            ("abcdefg", "abcdefg"),
            ("xyzzy", "fubar"),
        ];
        for (a, b) in pairs {
            assert_eq!(
                edit_distance(a, b),
                edit_distance_2d(a, b),
                "mismatch for ({a:?}, {b:?})",
            );
        }
    }

    #[test]
    fn fuzzy_match_returns_true_for_substring() {
        assert!(fuzzy_match("hello", "hello_world", 0.99));
    }

    #[test]
    fn fuzzy_match_is_case_insensitive() {
        assert!(fuzzy_match("HELLO", "hello", 0.99));
    }

    #[test]
    fn fuzzy_match_respects_threshold() {
        assert!(!fuzzy_match("xyz", "abcdef", 0.5));
    }

    #[test]
    fn fuzzy_match_handles_empty_strings() {
        assert!(fuzzy_match("", "", 0.5));
    }

    #[test]
    fn fuzzy_similarity_perfect_for_substring() {
        assert_eq!(fuzzy_similarity("foo", "foobar"), 1.0);
    }

    #[test]
    fn fuzzy_similarity_returns_zero_for_disjoint() {
        let sim = fuzzy_similarity("xyz", "abc");
        assert!(sim < 0.5);
    }

    #[test]
    fn fuzzy_similarity_handles_empty() {
        assert_eq!(fuzzy_similarity("", ""), 1.0);
    }

    #[test]
    fn search_items_filters_functions_by_name() {
        let funcs = vec![func("hello", "fn hello"), func("world", "fn world")];
        let (matched_fns, _, _) = search_items(&funcs, &[], &[], "hello", 0.7);
        assert_eq!(matched_fns.len(), 1);
        assert_eq!(matched_fns[0].name, "hello");
    }

    #[test]
    fn search_items_supports_pipe_separator() {
        let funcs = vec![
            func("alpha", "fn alpha"),
            func("beta", "fn beta"),
            func("gamma", "fn gamma"),
        ];
        let (matched_fns, _, _) = search_items(&funcs, &[], &[], "alpha | beta", 0.7);
        assert_eq!(matched_fns.len(), 2);
    }

    #[test]
    fn name_score_exact_beats_prefix_beats_substring() {
        let exact = name_score("load", "load", 0.85).expect("exact");
        let prefix = name_score("load", "load_jsonl", 0.85).expect("prefix");
        let substr = name_score("load", "preload_data", 0.85).expect("substr");
        assert_eq!(exact, 1.0);
        assert!(prefix < exact && prefix > substr, "prefix={prefix} substr={substr}");
        assert!(substr >= 0.85, "substr={substr}");
    }

    #[test]
    fn name_score_none_when_below_threshold() {
        assert!(name_score("xyz", "abcdef", 0.85).is_none());
    }

    #[test]
    fn name_score_threshold_zero_admits_disjoint() {

        assert!(name_score("qrstuv", "alpha", 0.0).is_some());
    }

    #[test]
    fn search_items_exact_name_fast_path_suppresses_substring() {


        let funcs = vec![
            func("load", "fn load"),
            func("load_jsonl", "fn load_jsonl"),
            func("preload", "fn preload"),
            func("unrelated", "fn unrelated"),
        ];
        let (matched, _, _) = search_items(&funcs, &[], &[], "load", 0.85);
        let names: Vec<&str> = matched.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["load"], "fast path should return exact-only: {names:?}");
    }

    #[test]
    fn search_items_substring_returned_when_no_exact_match() {


        let funcs = vec![
            func("load", "fn load"),
            func("load_jsonl", "fn load_jsonl"),
        ];
        let (matched, _, _) = search_items(&funcs, &[], &[], "loa", 0.85);
        let names: Vec<&str> = matched.iter().map(|f| f.name.as_str()).collect();

        assert_eq!(matched.len(), 2, "R26-W3 fallback should rescue prefix/substring hits: {names:?}");
        for n in &["load", "load_jsonl"] {
            assert!(names.contains(n), "expected `{n}` in fallback hits: {names:?}");
        }
    }

    #[test]
    fn regression_short_query_exact_name_fast_path() {


        let funcs = vec![
            func("new", "fn new"),
            func("newest_jsonl", "fn newest_jsonl"),
            func("new_surgery_id", "fn new_surgery_id"),
            func("preload_new", "fn preload_new"),
        ];
        let (matched, _, _) = search_items(&funcs, &[], &[], "new", 0.85);
        let names: Vec<&str> = matched.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["new"], "short-query precision violated: {names:?}");
    }

    #[test]
    fn regression_short_query_returns_empty_when_no_exact_match() {


        let funcs = vec![
            func("handle_get", "fn handle_get"),
            func("target_files", "fn target_files"),
            func("session_budget_overhead", "fn session_budget_overhead"),
        ];
        let (matched, _, _) = search_items(&funcs, &[], &[], "get", 0.85);
        let names: Vec<&str> = matched.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(matched.len(), 3, "R26-W3 fallback rescues substring hits at 0.85: {names:?}");
    }

    #[test]
    fn search_items_filters_structs_and_enums() {
        let structs = vec![s("Foo"), s("Bar")];
        let enums = vec![e("Color"), e("Shape")];
        let (_, matched_structs, matched_enums) =
            search_items(&[], &structs, &enums, "Color", 0.7);
        assert_eq!(matched_structs.len(), 0);
        assert_eq!(matched_enums.len(), 1);
    }
}
