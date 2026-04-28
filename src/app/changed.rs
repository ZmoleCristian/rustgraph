//! Git-based changed-line range detection used by the `--changed` flag.
//!
//! Queries `git diff` between a ref and HEAD to build a per-file map of
//! modified line ranges, which callers use to filter analysis results.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Map from relative file path to a list of inclusive `(start_line, end_line)` ranges
/// that were added or modified since the reference commit.
pub type ChangedRanges = HashMap<String, Vec<(usize, usize)>>;

/// Builds a [`ChangedRanges`] map by running `git diff <git_ref>..HEAD` inside
/// `project_root`. Returns an error if git is unavailable or the ref is invalid.
pub fn build_changed_ranges(
    project_root: &Path,
    git_ref: &str,
) -> Result<ChangedRanges, Box<dyn std::error::Error>> {

    let rev_parse = Command::new("git")
        .args(["rev-parse", "--verify", git_ref])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("failed to invoke `git` (is it installed?): {}", e))?;
    if !rev_parse.status.success() {
        let stderr = String::from_utf8_lossy(&rev_parse.stderr).trim().to_string();


        if stderr.contains("not a git repository") {
            return Err(format!(
                "not a git repository: '{}' — `--changed` requires git",
                project_root.display()
            )
            .into());
        }
        return Err(format!(
            "git ref '{}' does not exist (git rev-parse failed: {})",
            git_ref, stderr
        )
        .into());
    }


    let names = Command::new("git")
        .args(["diff", "--name-only", &format!("{}..HEAD", git_ref)])
        .current_dir(project_root)
        .output()
        .map_err(|e| format!("failed to run `git diff --name-only`: {}", e))?;
    if !names.status.success() {
        let stderr = String::from_utf8_lossy(&names.stderr).trim().to_string();
        return Err(format!("git diff --name-only failed: {}", stderr).into());
    }

    let stdout = String::from_utf8_lossy(&names.stdout);
    let rust_files: Vec<&str> = stdout
        .lines()
        .filter(|l| l.ends_with(".rs"))
        .filter(|l| !l.is_empty())
        .collect();

    let mut map: ChangedRanges = HashMap::new();
    for file in rust_files {
        let diff = Command::new("git")
            .args([
                "diff",
                "--unified=0",
                &format!("{}..HEAD", git_ref),
                "--",
                file,
            ])
            .current_dir(project_root)
            .output()
            .map_err(|e| format!("failed to run `git diff` for {}: {}", file, e))?;
        if !diff.status.success() {

            continue;
        }
        let diff_text = String::from_utf8_lossy(&diff.stdout);
        let ranges = parse_hunk_ranges(&diff_text);
        if !ranges.is_empty() {
            map.insert(file.to_string(), ranges);
        }
    }

    Ok(map)
}


fn parse_hunk_ranges(diff_text: &str) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for line in diff_text.lines() {
        if !line.starts_with("@@") {
            continue;
        }


        let plus_idx = match line.find('+') {
            Some(i) => i,
            None => continue,
        };
        let after_plus = &line[plus_idx + 1..];

        let end = after_plus.find(' ').unwrap_or(after_plus.len());
        let token = &after_plus[..end];
        let (start_str, count_str) = match token.split_once(',') {
            Some((s, c)) => (s, c),
            None => (token, "1"),
        };
        let start: usize = match start_str.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let count: usize = count_str.parse().unwrap_or(1);
        if count == 0 {


            ranges.push((start, start));
        } else {
            ranges.push((start, start + count - 1));
        }
    }
    ranges
}


/// Returns `true` if the function spanning `[start_line, end_line]` in `file_path`
/// overlaps any changed range in `changed`. Performs suffix-based path matching to
/// handle relative vs. absolute path discrepancies.
pub fn fn_was_changed(
    file_path: &str,
    start_line: usize,
    end_line: usize,
    changed: &ChangedRanges,
) -> bool {

    if let Some(ranges) = changed.get(file_path) {
        return ranges.iter().any(|(s, e)| ranges_overlap(*s, *e, start_line, end_line));
    }


    for (key, ranges) in changed {
        if file_path.ends_with(key)
            || key.ends_with(file_path)
            || file_path.ends_with(&format!("/{}", key))
            || key.ends_with(&format!("/{}", file_path))
        {
            if ranges.iter().any(|(s, e)| ranges_overlap(*s, *e, start_line, end_line)) {
                return true;
            }
        }
    }
    false
}


/// Returns `true` if `file_path` appears anywhere in `changed`, using suffix-based
/// path matching to tolerate relative vs. absolute path differences.
pub fn file_was_changed(file_path: &str, changed: &ChangedRanges) -> bool {
    if changed.contains_key(file_path) {
        return true;
    }
    for key in changed.keys() {
        if file_path.ends_with(key)
            || key.ends_with(file_path)
            || file_path.ends_with(&format!("/{}", key))
            || key.ends_with(&format!("/{}", file_path))
        {
            return true;
        }
    }
    false
}

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start <= b_end && b_start <= a_end
}


/// Default git ref used when `--since` is not provided; compares against the previous commit.
pub const DEFAULT_SINCE: &str = "HEAD~1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hunk_ranges_basic_single_hunk() {
        let diff = "diff --git a/foo.rs b/foo.rs\nindex 1234..5678 100644\n--- a/foo.rs\n+++ b/foo.rs\n@@ -10,1 +10,2 @@\n-old\n+new line 1\n+new line 2\n";
        let ranges = parse_hunk_ranges(diff);
        assert_eq!(ranges, vec![(10, 11)]);
    }

    #[test]
    fn parse_hunk_ranges_omitted_count_defaults_to_1() {
        let diff = "@@ -10 +20 @@\n-old\n+new\n";
        let ranges = parse_hunk_ranges(diff);
        assert_eq!(ranges, vec![(20, 20)]);
    }

    #[test]
    fn parse_hunk_ranges_zero_count_is_deletion_at_start() {
        let diff = "@@ -10,1 +10,0 @@\n-deleted\n";
        let ranges = parse_hunk_ranges(diff);
        assert_eq!(ranges, vec![(10, 10)]);
    }

    #[test]
    fn parse_hunk_ranges_multi_hunk() {
        let diff = "@@ -1,1 +1,2 @@\n+a\n+b\n@@ -50,0 +60,3 @@\n+c\n+d\n+e\n";
        let ranges = parse_hunk_ranges(diff);
        assert_eq!(ranges, vec![(1, 2), (60, 62)]);
    }

    #[test]
    fn fn_was_changed_returns_true_on_overlap() {
        let mut map = ChangedRanges::new();
        map.insert("src/foo.rs".to_string(), vec![(10, 20)]);

        assert!(fn_was_changed("src/foo.rs", 5, 15, &map));
        assert!(fn_was_changed("src/foo.rs", 18, 25, &map));
        assert!(fn_was_changed("src/foo.rs", 12, 13, &map));
    }

    #[test]
    fn fn_was_changed_returns_false_when_no_overlap() {
        let mut map = ChangedRanges::new();
        map.insert("src/foo.rs".to_string(), vec![(10, 20)]);
        assert!(!fn_was_changed("src/foo.rs", 1, 9, &map));
        assert!(!fn_was_changed("src/foo.rs", 21, 30, &map));
    }

    #[test]
    fn fn_was_changed_returns_false_when_file_absent() {
        let mut map = ChangedRanges::new();
        map.insert("src/foo.rs".to_string(), vec![(10, 20)]);
        assert!(!fn_was_changed("src/bar.rs", 12, 13, &map));
    }

    #[test]
    fn fn_was_changed_suffix_match_works() {
        let mut map = ChangedRanges::new();
        map.insert("src/foo.rs".to_string(), vec![(10, 20)]);

        assert!(fn_was_changed("crate-name/src/foo.rs", 12, 13, &map));
    }

    #[test]
    fn file_was_changed_basic() {
        let mut map = ChangedRanges::new();
        map.insert("src/foo.rs".to_string(), vec![(10, 20)]);
        assert!(file_was_changed("src/foo.rs", &map));
        assert!(!file_was_changed("src/bar.rs", &map));
    }
}
