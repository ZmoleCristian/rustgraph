//! File-tree scanner: walks a project directory and aggregates per-file parse results.

use super::{CallSite, EnumInfo, FunctionInfo, StructInfo, parse_rust_file};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Aggregated analysis results for an entire project directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectAnalysis {
    /// All functions extracted from every parsed file.
    pub functions: Vec<FunctionInfo>,
    /// All structs extracted from every parsed file.
    pub structs: Vec<StructInfo>,
    /// All enums extracted from every parsed file.
    pub enums: Vec<EnumInfo>,
    /// Caller-id → list of callee names (raw, unresolved).
    pub call_map: HashMap<String, Vec<String>>,
    /// All call sites across every parsed file.
    pub call_sites: Vec<CallSite>,
    /// Number of `.rs` files that were successfully parsed (or attempted).
    pub files_analyzed: usize,
    /// Human-readable error messages for files that failed to parse.
    pub parse_errors: Vec<String>,

    /// Re-export triples `(source_path, exported_name, alias)` collected during parsing.
    #[serde(default)]
    pub reexports: Vec<(String, String, String)>,
}

/// Walk `project_path` and parse every `.rs` file, returning aggregated results.
///
/// When `include_ignored` is `false`, files excluded by `.gitignore` and similar
/// ignore rules are skipped.
pub fn analyze_project(project_path: &Path, include_ignored: bool) -> ProjectAnalysis {
    let rust_files = find_rust_files(project_path, include_ignored);

    let mut all_functions = Vec::new();
    let mut all_structs = Vec::new();
    let mut all_enums = Vec::new();
    let mut all_call_maps: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_call_sites = Vec::new();
    let mut all_reexports: Vec<(String, String, String)> = Vec::new();
    let mut parse_errors = Vec::new();

    for file_path in &rust_files {
        match parse_rust_file(file_path) {
            Ok((
                mut functions,
                mut structs,
                mut enums,
                call_map,
                mut call_sites,
                _aliases,
                mut reexports,
            )) => {
                all_functions.append(&mut functions);
                all_structs.append(&mut structs);
                all_enums.append(&mut enums);
                // `HashMap::extend` would silently overwrite collisions on
                // file_path:line:name keys — concatenate the callee vecs
                // instead so cross-crate `--also` merges retain every edge.
                for (caller_id, mut callees) in call_map {
                    all_call_maps
                        .entry(caller_id)
                        .or_default()
                        .append(&mut callees);
                }
                all_call_sites.append(&mut call_sites);
                all_reexports.append(&mut reexports);
            }
            Err(e) => {
                parse_errors.push(format!("⚠️ {}: {}", file_path.display(), e));
            }
        }
    }

    ProjectAnalysis {
        functions: all_functions,
        structs: all_structs,
        enums: all_enums,
        call_map: all_call_maps,
        call_sites: all_call_sites,
        files_analyzed: rust_files.len(),
        parse_errors,
        reexports: all_reexports,
    }
}

/// Recursively enumerate all `.rs` files under `path`.
///
/// When `include_ignored` is `false`, the walk respects `.gitignore`, `.ignore`,
/// and global git-exclude rules.
pub fn find_rust_files(path: &Path, include_ignored: bool) -> Vec<PathBuf> {
    let mut builder = WalkBuilder::new(path);

    if include_ignored {
        builder.git_ignore(false);
        builder.ignore(false);
        builder.git_exclude(false);
        builder.git_global(false);
    }

    builder
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().is_some_and(|ft| ft.is_file())
                && entry.path().extension().is_some_and(|ext| ext == "rs")
        })
        .map(|entry| entry.path().to_path_buf())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_rust_files_returns_only_rs_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.rs"), "").expect("a");
        std::fs::write(dir.path().join("b.rs"), "").expect("b");
        std::fs::write(dir.path().join("not_rust.txt"), "").expect("txt");

        let mut files = find_rust_files(dir.path(), false);
        files.sort();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| p.extension().and_then(|e| e.to_str()) == Some("rs")));
    }

    #[test]
    fn find_rust_files_respects_gitignore_by_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".gitignore"), "ignored.rs\n").expect("gitignore");
        std::fs::write(dir.path().join("ignored.rs"), "").expect("ignored");
        std::fs::write(dir.path().join("kept.rs"), "").expect("kept");

        let files = find_rust_files(dir.path(), false);
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .map(String::from)
            .collect();
        assert!(names.contains(&"kept.rs".to_string()));
    }

    #[test]
    fn find_rust_files_includes_ignored_when_flag_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".gitignore"), "ignored.rs\n").expect("gitignore");
        std::fs::write(dir.path().join("ignored.rs"), "").expect("ignored");

        let files = find_rust_files(dir.path(), true);
        assert!(files.iter().any(|p| p.ends_with("ignored.rs")));
    }

    #[test]
    fn analyze_project_returns_aggregated_results() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("lib.rs"),
            "pub fn one() {}\npub struct S;\npub enum E { A }\n",
        )
        .expect("write");

        let analysis = analyze_project(dir.path(), false);
        assert!(analysis.files_analyzed >= 1);
        assert_eq!(analysis.functions.len(), 1);
        assert_eq!(analysis.structs.len(), 1);
        assert_eq!(analysis.enums.len(), 1);
        assert!(analysis.parse_errors.is_empty());
    }

    #[test]
    fn analyze_project_records_parse_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("bad.rs"), "fn ( { broken").expect("write");
        let analysis = analyze_project(dir.path(), false);
        assert!(!analysis.parse_errors.is_empty());
    }

    #[test]
    fn analyze_project_handles_empty_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let analysis = analyze_project(dir.path(), false);
        assert_eq!(analysis.files_analyzed, 0);
        assert!(analysis.functions.is_empty());
    }
}
