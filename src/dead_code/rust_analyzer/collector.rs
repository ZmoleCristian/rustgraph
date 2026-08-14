//! Reference-count collector that drives the rust-analyzer LSP client.

use super::super::model::{
    RUST_ANALYZER_BIN, ReferenceCountMaps, ReferenceCounts, collect_cfg_test_ranges,
    find_function_name_position, is_test_path, line_in_ranges, path_to_file_uri,
    read_file_lines_cached,
};
use super::client::RustAnalyzerLspClient;
use crate::FunctionInfo;
use std::collections::{HashMap, HashSet};
use std::path::Path;

type DynError = Box<dyn std::error::Error>;

fn function_reference_lookup(
    func: &FunctionInfo,
    file_lines_cache: &mut HashMap<String, Vec<String>>,
) -> Option<(usize, usize)> {
    let file_lines = read_file_lines_cached(&func.file_path, file_lines_cache)?;
    find_function_name_position(file_lines, func.start_line, &func.name)
}

fn is_test_reference(
    ref_file: &str,
    ref_line: usize,
    test_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
) -> bool {
    if is_test_path(ref_file) {
        return true;
    }

    let ranges = test_ranges_cache
        .entry(ref_file.to_string())
        .or_insert_with(|| collect_cfg_test_ranges(ref_file));
    line_in_ranges(ref_line, ranges)
}

fn count_reference(
    counts: &mut ReferenceCountMaps,
    function_key: &(String, usize),
    ref_file: String,
    ref_line: usize,
    ref_col: usize,
    dedup: &mut HashSet<(String, usize, usize, String, usize)>,
    test_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
) {
    let dedup_key = (
        function_key.0.clone(),
        function_key.1,
        ref_col,
        ref_file.clone(),
        ref_line,
    );
    if !dedup.insert(dedup_key) {
        return;
    }

    let target_counts = if is_test_reference(&ref_file, ref_line, test_ranges_cache) {
        &mut counts.1
    } else {
        &mut counts.0
    };
    *target_counts.entry(function_key.clone()).or_default() += 1;
}

/// Query rust-analyzer for all references to each function in `candidates`.
///
/// Starts a rust-analyzer process rooted at `workspace_root`, opens each source
/// file once, issues `textDocument/references` requests, and partitions the results
/// into non-test and test-only counts using `test_ranges_cache`.
/// Returns a pair `(non_test_counts, test_counts)` keyed by `(file_path, start_line)`.
pub fn collect_rust_analyzer_reference_counts(
    candidates: &[&FunctionInfo],
    workspace_root: &Path,
    test_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
    file_lines_cache: &mut HashMap<String, Vec<String>>,
) -> Result<ReferenceCountMaps, DynError> {
    let mut counts = (ReferenceCounts::new(), ReferenceCounts::new());
    let mut dedup: HashSet<(String, usize, usize, String, usize)> = HashSet::new();
    let mut opened_files: HashSet<String> = HashSet::new();

    let mut client = RustAnalyzerLspClient::start(RUST_ANALYZER_BIN, workspace_root)?;

    for func in candidates {
        let Some((line_idx_zero_based, character_utf16)) =
            function_reference_lookup(func, file_lines_cache)
        else {
            continue;
        };

        let file_uri = path_to_file_uri(Path::new(&func.file_path))?;
        client.open_document(&func.file_path, &file_uri, &mut opened_files)?;

        let refs = client.references(&file_uri, line_idx_zero_based, character_utf16)?;
        let function_key = (func.file_path.clone(), func.start_line);
        for (ref_file, ref_line, ref_col) in refs {
            count_reference(
                &mut counts,
                &function_key,
                ref_file,
                ref_line,
                ref_col,
                &mut dedup,
                test_ranges_cache,
            );
        }
    }

    client.shutdown();
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_test_reference_recognizes_test_paths() {
        let mut cache: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        assert!(is_test_reference("/repo/tests/x.rs", 1, &mut cache));
        assert!(is_test_reference(
            "/repo/src/parser/tests.rs",
            1,
            &mut cache
        ));
        assert!(!is_test_reference("/repo/src/lib.rs", 1, &mut cache));
    }

    #[test]
    fn count_reference_increments_non_test_counts() {
        let mut counts: ReferenceCountMaps = (ReferenceCounts::new(), ReferenceCounts::new());
        let mut dedup = HashSet::new();
        let mut test_cache = HashMap::new();
        let key = ("/repo/src/lib.rs".to_string(), 5usize);

        count_reference(
            &mut counts,
            &key,
            "/repo/src/use_site.rs".to_string(),
            10,
            4,
            &mut dedup,
            &mut test_cache,
        );

        assert_eq!(counts.0.get(&key).copied(), Some(1));
        assert!(counts.1.is_empty());
    }

    #[test]
    fn count_reference_increments_test_counts_for_test_files() {
        let mut counts: ReferenceCountMaps = (ReferenceCounts::new(), ReferenceCounts::new());
        let mut dedup = HashSet::new();
        let mut test_cache = HashMap::new();
        let key = ("/repo/src/lib.rs".to_string(), 5usize);

        count_reference(
            &mut counts,
            &key,
            "/repo/tests/integration.rs".to_string(),
            3,
            0,
            &mut dedup,
            &mut test_cache,
        );

        assert!(counts.0.is_empty());
        assert_eq!(counts.1.get(&key).copied(), Some(1));
    }

    #[test]
    fn count_reference_dedupes_repeated_calls() {
        let mut counts: ReferenceCountMaps = (ReferenceCounts::new(), ReferenceCounts::new());
        let mut dedup = HashSet::new();
        let mut test_cache = HashMap::new();
        let key = ("/repo/src/lib.rs".to_string(), 5usize);

        for _ in 0..3 {
            count_reference(
                &mut counts,
                &key,
                "/repo/src/use_site.rs".to_string(),
                10,
                4,
                &mut dedup,
                &mut test_cache,
            );
        }
        assert_eq!(counts.0.get(&key).copied(), Some(1));
    }

    #[test]
    fn function_reference_lookup_returns_none_for_missing_file() {
        let mut cache = HashMap::new();
        let func = FunctionInfo {
            name: "x".to_string(),
            signature: "fn x()".to_string(),
            file_path: "/no/such/file.rs".to_string(),
            start_line: 1,
            end_line: 2,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: Vec::new(),
            parameters: Vec::new(),
            return_type: None,
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        };
        assert!(function_reference_lookup(&func, &mut cache).is_none());
    }
}
