//! Source-context extraction for individual call sites.
//!
//! Reads file content on demand (with a per-call cache) and returns a window of
//! surrounding lines around a matched call-site line.

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A single source line included in call-site context output.
#[derive(Debug, Clone, Serialize)]
pub struct ContextLine {
    /// 1-based line number within the file.
    pub line: usize,
    /// Raw text of the source line.
    pub text: String,
    /// Whether this line is the matched call site (vs. surrounding context).
    pub is_match: bool,
}

pub(super) fn context_lines_for_call(
    file_path: &str,
    line: usize,
    before_context: usize,
    after_context: usize,
    file_lines_cache: &mut HashMap<String, Vec<String>>,
    primary_root: Option<&Path>,
    also_roots: &[PathBuf],
) -> Vec<ContextLine> {
    if before_context == 0 && after_context == 0 {
        return Vec::new();
    }

    if !file_lines_cache.contains_key(file_path) {
        let resolved = resolve_for_read(file_path, primary_root, also_roots);
        let lines = fs::read_to_string(&resolved)
            .ok()
            .map(|content| {
                content
                    .lines()
                    .map(|line| line.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        file_lines_cache.insert(file_path.to_string(), lines);
    }

    let Some(lines) = file_lines_cache.get(file_path) else {
        return Vec::new();
    };
    if lines.is_empty() {
        return Vec::new();
    }

    let match_idx = line.saturating_sub(1).min(lines.len().saturating_sub(1));
    let start_idx = match_idx.saturating_sub(before_context);
    let end_idx = (match_idx + after_context).min(lines.len().saturating_sub(1));

    (start_idx..=end_idx)
        .map(|line_idx| ContextLine {
            line: line_idx + 1,
            text: lines[line_idx].clone(),
            is_match: line_idx == match_idx,
        })
        .collect()
}

fn resolve_for_read(file_path: &str, primary: Option<&Path>, also: &[PathBuf]) -> PathBuf {
    let p = Path::new(file_path);
    if p.is_absolute() && p.exists() {
        return p.to_path_buf();
    }
    if let Some(primary) = primary {
        let pj = primary.join(p);
        if pj.exists() {
            return pj;
        }
    }
    if let Some((label, rest)) = file_path.split_once('/') {
        for r in also {
            let basename = r.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if basename == label {
                let candidate = r.join(rest);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }

    if let Some(root) = crate::index::resolve_root_hint() {
        let direct = root.join(p);
        if direct.exists() {
            return direct;
        }
    }
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(content: &str) -> std::path::PathBuf {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("file.rs");
        std::fs::write(&path, content).expect("write");
        let leaked = dir.keep();
        leaked.join("file.rs")
    }

    #[test]
    fn context_lines_for_call_returns_empty_when_zero_context() {
        let path = write_temp("a\nb\nc\n");
        let mut cache = HashMap::new();
        let lines = context_lines_for_call(path.to_str().unwrap(), 2, 0, 0, &mut cache, None, &[]);
        assert!(lines.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn context_lines_for_call_returns_window_around_match() {
        let path = write_temp("one\ntwo\nthree\nfour\nfive\n");
        let mut cache = HashMap::new();
        let lines = context_lines_for_call(path.to_str().unwrap(), 3, 1, 1, &mut cache, None, &[]);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].line, 2);
        assert!(!lines[0].is_match);
        assert_eq!(lines[1].line, 3);
        assert!(lines[1].is_match);
        assert_eq!(lines[2].line, 4);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn context_lines_for_call_clamps_at_file_boundaries() {
        let path = write_temp("a\nb\nc\n");
        let mut cache = HashMap::new();
        let lines = context_lines_for_call(path.to_str().unwrap(), 1, 5, 5, &mut cache, None, &[]);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].line, 1);
        assert!(lines[0].is_match);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn context_lines_for_call_returns_empty_for_missing_file_with_nonzero_ctx() {
        let mut cache = HashMap::new();
        let lines = context_lines_for_call("/nonexistent/file.rs", 1, 1, 1, &mut cache, None, &[]);
        assert!(lines.is_empty());
    }

    #[test]
    fn context_lines_for_call_uses_cache_after_first_load() {
        let path = write_temp("a\nb\nc\n");
        let mut cache = HashMap::new();
        let _ = context_lines_for_call(path.to_str().unwrap(), 1, 1, 1, &mut cache, None, &[]);
        std::fs::remove_file(&path).expect("rm");
        let lines = context_lines_for_call(path.to_str().unwrap(), 2, 0, 1, &mut cache, None, &[]);
        assert_eq!(lines.len(), 2);
    }
}
