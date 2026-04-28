//! Token-scan based reference detectors for direct calls and argument-position references.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::parsing::{extract_call_arg_segment_from_file, normalize_handler_expr};
use crate::dead_code::model::{
    collect_cfg_test_ranges, collect_macro_rules_ranges, is_test_path, line_in_ranges,
};

/// Scan source files for direct call references to candidate function names.
///
/// For each file the function tokenises each source line, identifies
/// `identifier(` patterns whose base name appears in `candidate_name_counts`
/// with a count of exactly 1 (to avoid ambiguity), and classifies the call
/// site as test or non-test.  Lines inside `macro_rules!` blocks are skipped.
/// Returns `(non_test_counts, test_only_counts)` maps from function name to reference count.
pub(crate) fn collect_textual_function_refs(
    rust_files: &[PathBuf],
    candidate_name_counts: &HashMap<String, usize>,
    test_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
    macro_rules_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
    file_lines_cache: &mut HashMap<String, Vec<String>>,
) -> (HashMap<String, usize>, HashMap<String, usize>) {
    let mut non_test = HashMap::new();
    let mut test_only = HashMap::new();
    let candidate_names: std::collections::HashSet<&str> =
        candidate_name_counts.keys().map(|s| s.as_str()).collect();

    for file_path in rust_files {
        let file = file_path.to_string_lossy().to_string();
        if !file_lines_cache.contains_key(&file) {
            let Ok(content) = fs::read_to_string(file_path) else {
                continue;
            };
            let lines = content.lines().map(|l| l.to_string()).collect::<Vec<_>>();
            file_lines_cache.insert(file.clone(), lines);
        }
        let Some(lines) = file_lines_cache.get(&file).cloned() else {
            continue;
        };

        for (idx, raw) in lines.iter().enumerate() {
            let line_no = idx + 1;
            let macro_rules_ranges = macro_rules_ranges_cache
                .entry(file.clone())
                .or_insert_with(|| collect_macro_rules_ranges(&file));
            if line_in_ranges(line_no, macro_rules_ranges) {
                continue;
            }
            let line = raw.split("//").next().unwrap_or("");
            let chars = line.chars().collect::<Vec<_>>();
            let mut i = 0usize;
            while i < chars.len() {
                if !(chars[i].is_alphabetic() || chars[i] == '_') {
                    i += 1;
                    continue;
                }
                let start = i;
                i += 1;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == ':')
                {
                    i += 1;
                }
                let token = chars[start..i].iter().collect::<String>();
                let base = token.rsplit("::").next().unwrap_or("").trim();
                if !candidate_names.contains(base) {
                    continue;
                }
                if candidate_name_counts.get(base).copied().unwrap_or(0) > 1 {
                    continue;
                }
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j >= chars.len() || chars[j] != '(' {
                    continue;
                }
                let prefix = chars[..start].iter().collect::<String>();
                let prev_word = prefix
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .rfind(|s| !s.is_empty())
                    .unwrap_or("");
                if prev_word == "fn" {
                    continue;
                }

                let is_test_ref = if is_test_path(&file) {
                    true
                } else {
                    let ranges = test_ranges_cache
                        .entry(file.clone())
                        .or_insert_with(|| collect_cfg_test_ranges(&file));
                    line_in_ranges(line_no, ranges)
                };
                if is_test_ref {
                    *test_only.entry(base.to_string()).or_default() += 1;
                } else {
                    *non_test.entry(base.to_string()).or_default() += 1;
                }
            }
        }
    }

    (non_test, test_only)
}

#[cfg(test)]
mod textual_tests {
    use super::*;

    fn write_files(specs: &[(&str, &str)]) -> Vec<PathBuf> {
        let dir = tempfile::tempdir().expect("tempdir");
        let leaked = dir.keep();
        let mut paths = Vec::new();
        for (name, content) in specs {
            let p = leaked.join(name);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).expect("mkdirs");
            }
            std::fs::write(&p, content).expect("write");
            paths.push(p);
        }
        paths
    }

    #[test]
    fn collect_textual_function_refs_counts_unique_calls() {
        let files = write_files(&[("src/lib.rs", "fn x() { my_fn(); my_fn(); }\n")]);
        let mut name_counts = HashMap::new();
        name_counts.insert("my_fn".to_string(), 1usize);
        let mut tr = HashMap::new();
        let mut mr = HashMap::new();
        let mut fl = HashMap::new();
        let (non_test, test_only) = collect_textual_function_refs(
            &files,
            &name_counts,
            &mut tr,
            &mut mr,
            &mut fl,
        );
        assert_eq!(non_test.get("my_fn"), Some(&2));
        assert!(test_only.is_empty());
    }

    #[test]
    fn collect_textual_function_refs_skips_duplicate_named_candidates() {

        let files = write_files(&[("src/lib.rs", "fn x() { my_fn(); }\n")]);
        let mut name_counts = HashMap::new();
        name_counts.insert("my_fn".to_string(), 2usize);
        let mut tr = HashMap::new();
        let mut mr = HashMap::new();
        let mut fl = HashMap::new();
        let (non_test, _) = collect_textual_function_refs(
            &files,
            &name_counts,
            &mut tr,
            &mut mr,
            &mut fl,
        );
        assert!(non_test.get("my_fn").is_none());
    }

    #[test]
    fn collect_textual_function_refs_segregates_test_paths() {
        let files = write_files(&[("src/tests/foo.rs", "fn x() { my_fn(); }\n")]);
        let mut name_counts = HashMap::new();
        name_counts.insert("my_fn".to_string(), 1usize);
        let mut tr = HashMap::new();
        let mut mr = HashMap::new();
        let mut fl = HashMap::new();
        let (non_test, test_only) = collect_textual_function_refs(
            &files,
            &name_counts,
            &mut tr,
            &mut mr,
            &mut fl,
        );
        assert!(non_test.is_empty());
        assert_eq!(test_only.get("my_fn"), Some(&1));
    }

    #[test]
    fn collect_textual_function_refs_skips_function_definitions() {
        let files = write_files(&[("src/lib.rs", "fn my_fn() {}\n")]);
        let mut name_counts = HashMap::new();
        name_counts.insert("my_fn".to_string(), 1usize);
        let mut tr = HashMap::new();
        let mut mr = HashMap::new();
        let mut fl = HashMap::new();
        let (non_test, test_only) = collect_textual_function_refs(
            &files,
            &name_counts,
            &mut tr,
            &mut mr,
            &mut fl,
        );
        assert!(non_test.is_empty());
        assert!(test_only.is_empty());
    }

    #[test]
    fn collect_textual_function_argument_refs_finds_handler_in_map() {
        let files = write_files(&[("src/lib.rs", "fn x() { vec.into_iter().map(my_handler); }\n")]);
        let mut name_counts = HashMap::new();
        name_counts.insert("my_handler".to_string(), 1usize);
        let mut tr = HashMap::new();
        let mut mr = HashMap::new();
        let mut fl = HashMap::new();
        let (non_test, _) = collect_textual_function_argument_refs(
            &files,
            &name_counts,
            &mut tr,
            &mut mr,
            &mut fl,
        );
        assert_eq!(non_test.get("my_handler"), Some(&1));
    }

    #[test]
    fn collect_textual_function_argument_refs_finds_handler_in_fold() {
        let files = write_files(&[("src/lib.rs", "fn x() { v.fold(0, my_reducer); }\n")]);
        let mut name_counts = HashMap::new();
        name_counts.insert("my_reducer".to_string(), 1usize);
        let mut tr = HashMap::new();
        let mut mr = HashMap::new();
        let mut fl = HashMap::new();
        let (non_test, _) = collect_textual_function_argument_refs(
            &files,
            &name_counts,
            &mut tr,
            &mut mr,
            &mut fl,
        );
        assert_eq!(non_test.get("my_reducer"), Some(&1));
    }
}

/// Scan source files for candidate function names passed as arguments to known wrapper calls.
///
/// Recognises a fixed set of higher-order wrapper names (`map`, `fold`, `and_then`,
/// `service`, `validator`, etc.) and extracts the Nth argument at each call site.
/// Only names with exactly one candidate definition are counted; ambiguous names are
/// skipped.  Returns `(non_test_counts, test_only_counts)` maps.
pub(crate) fn collect_textual_function_argument_refs(
    rust_files: &[PathBuf],
    candidate_name_counts: &HashMap<String, usize>,
    test_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
    macro_rules_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
    file_lines_cache: &mut HashMap<String, Vec<String>>,
) -> (HashMap<String, usize>, HashMap<String, usize>) {
    let mut non_test = HashMap::new();
    let mut test_only = HashMap::new();
    let wrappers: [(&str, usize); 15] = [
        ("and_then", 0),
        ("or_else", 0),
        ("then", 0),
        ("map", 0),
        ("filter_map", 0),
        ("for_each", 0),
        ("fold", 1),
        ("try_fold", 1),
        ("map_or_else", 1),
        ("from_fn", 0),
        ("from_fn_with_state", 1),
        ("new", 0),
        ("service", 0),
        ("validator", 0),
        ("validator_os", 0),
    ];

    for file_path in rust_files {
        let file = file_path.to_string_lossy().to_string();
        if !file_lines_cache.contains_key(&file) {
            let Ok(content) = fs::read_to_string(file_path) else {
                continue;
            };
            let lines = content.lines().map(|l| l.to_string()).collect::<Vec<_>>();
            file_lines_cache.insert(file.clone(), lines);
        }
        let Some(lines) = file_lines_cache.get(&file).cloned() else {
            continue;
        };

        for (idx, raw) in lines.iter().enumerate() {
            let line_no = idx + 1;
            let macro_rules_ranges = macro_rules_ranges_cache
                .entry(file.clone())
                .or_insert_with(|| collect_macro_rules_ranges(&file));
            if line_in_ranges(line_no, macro_rules_ranges) {
                continue;
            }
            let line = raw.split("//").next().unwrap_or("");

            for (wrapper, arg_index) in wrappers {
                let marker = format!("{}(", wrapper);
                let mut search_from = 0usize;
                while search_from < line.len() {
                    let Some(found_at) = line[search_from..].find(&marker) else {
                        break;
                    };
                    let call_col = search_from + found_at;
                    search_from = call_col + marker.len();

                    let Some(arg_expr) = extract_call_arg_segment_from_file(
                        &file,
                        line_no,
                        call_col,
                        wrapper,
                        arg_index,
                        file_lines_cache,
                    ) else {
                        continue;
                    };
                    let Some((_, base)) = normalize_handler_expr(&arg_expr) else {
                        continue;
                    };
                    if candidate_name_counts.get(&base).copied().unwrap_or(0) != 1 {
                        continue;
                    }

                    let is_test_ref = if is_test_path(&file) {
                        true
                    } else {
                        let ranges = test_ranges_cache
                            .entry(file.clone())
                            .or_insert_with(|| collect_cfg_test_ranges(&file));
                        line_in_ranges(line_no, ranges)
                    };
                    if is_test_ref {
                        *test_only.entry(base).or_default() += 1;
                    } else {
                        *non_test.entry(base).or_default() += 1;
                    }
                }
            }
        }
    }

    (non_test, test_only)
}
