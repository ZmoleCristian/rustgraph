//! Entry point that ties together file loading, marker search, body extraction,
//! and argument splitting.

use super::super::args::split_top_level_call_args;
use super::cache::FileLinesCache;
use std::fs;

/// Return the `arg_index`-th argument of a call to `callee_base` found near
/// `line`/`column` in `file_path`.
///
/// Loads `file_path` into `file_lines_cache` on first access. Returns `None`
/// if the file cannot be read, the marker is not found, or `arg_index` is
/// out of range.
pub(crate) fn extract_nth_call_arg_segment_from_file(
    file_path: &str,
    line: usize,
    column: usize,
    callee_base: &str,
    arg_index: usize,
    file_lines_cache: &mut FileLinesCache,
) -> Option<String> {
    if !file_lines_cache.contains_key(file_path) {
        let content = fs::read_to_string(file_path).ok()?;
        let lines = content
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        file_lines_cache.insert(file_path.to_string(), lines);
    }
    let lines = file_lines_cache.get(file_path)?;
    if lines.is_empty() {
        return None;
    }

    let marker_location =
        super::find_marker::find_marker_in_lines(lines, line, column, callee_base)?;
    let (marker_line, marker_col) = marker_location;

    let body = super::extract_body::extract_call_args_body(lines, marker_line, marker_col)?;
    split_top_level_call_args(&body).get(arg_index).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn temp_with(content: &str) -> NamedTempFile {
        let mut tmp = tempfile::Builder::new()
            .suffix(".rs")
            .tempfile()
            .unwrap();
        tmp.write_all(content.as_bytes()).unwrap();
        tmp.flush().unwrap();
        tmp
    }

    #[test]
    fn extract_nth_returns_none_for_missing_file() {
        let mut cache = FileLinesCache::new();
        let result = extract_nth_call_arg_segment_from_file(
            "/no/such/file.rs",
            1,
            0,
            "foo",
            0,
            &mut cache,
        );
        assert!(result.is_none());
    }

    #[test]
    fn extract_nth_returns_none_for_empty_file() {
        let file = temp_with("");
        let mut cache = FileLinesCache::new();
        let result = extract_nth_call_arg_segment_from_file(
            file.path().to_str().unwrap(),
            1,
            0,
            "foo",
            0,
            &mut cache,
        );
        assert!(result.is_none());
    }

    #[test]
    fn extract_nth_returns_specific_arg_index() {
        let file = temp_with("fn run() { foo(alpha, beta, gamma); }\n");
        let mut cache = FileLinesCache::new();
        let arg = extract_nth_call_arg_segment_from_file(
            file.path().to_str().unwrap(),
            1,
            0,
            "foo",
            1,
            &mut cache,
        );
        assert_eq!(arg.as_deref(), Some("beta"));
    }

    #[test]
    fn extract_nth_returns_none_for_out_of_range_index() {
        let file = temp_with("fn run() { foo(alpha); }\n");
        let mut cache = FileLinesCache::new();
        let arg = extract_nth_call_arg_segment_from_file(
            file.path().to_str().unwrap(),
            1,
            0,
            "foo",
            5,
            &mut cache,
        );
        assert!(arg.is_none());
    }

    #[test]
    fn extract_nth_uses_existing_cache_entry() {
        let file = temp_with("fn run() { foo(arg); }\n");
        let path = file.path().to_str().unwrap().to_string();
        let mut cache = FileLinesCache::new();

        cache.insert(path.clone(), vec!["fn run() { foo(custom); }".to_string()]);
        let arg = extract_nth_call_arg_segment_from_file(&path, 1, 0, "foo", 0, &mut cache);
        assert_eq!(arg.as_deref(), Some("custom"));
    }

    #[test]
    fn extract_nth_handles_multiline_call() {
        let file = temp_with(
            "fn route() {\n    router\n        .route(\n            \"/users\",\n            get(crate::handlers::list_users),\n        );\n}\n",
        );
        let mut cache = FileLinesCache::new();
        let arg = extract_nth_call_arg_segment_from_file(
            file.path().to_str().unwrap(),
            2,
            0,
            "get",
            0,
            &mut cache,
        );
        assert_eq!(arg.as_deref(), Some("crate::handlers::list_users"));
    }
}
