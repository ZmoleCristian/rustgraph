//! File-reading helpers: line caching and function-name position lookup.

use std::collections::HashMap;
use std::fs;

/// Locate the identifier position of `function_name` inside `file_lines`.
///
/// Scans up to 40 lines starting at `start_line` (1-based), strips inline comments,
/// and returns `(line_index_0_based, utf16_column)` of the first match, or `None`.
pub(crate) fn find_function_name_position(
    file_lines: &[String],
    start_line: usize,
    function_name: &str,
) -> Option<(usize, usize)> {
    if file_lines.is_empty() {
        return None;
    }
    let start_idx = start_line
        .saturating_sub(1)
        .min(file_lines.len().saturating_sub(1));
    let end_idx = (start_idx + 40).min(file_lines.len());

    for (line_idx, file_line) in file_lines.iter().enumerate().take(end_idx).skip(start_idx) {
        let code = file_line.split("//").next().unwrap_or("");
        let mut search_from = 0usize;

        while search_from < code.len() {
            let Some(found_at) = code[search_from..].find("fn ") else {
                break;
            };
            let fn_kw_idx = search_from + found_at;
            let mut name_start = fn_kw_idx + 3;
            while let Some(ch) = code[name_start..].chars().next() {
                if !ch.is_whitespace() {
                    break;
                }
                name_start += ch.len_utf8();
                if name_start >= code.len() {
                    break;
                }
            }
            if name_start >= code.len() {
                break;
            }

            let ident = code[name_start..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<String>();
            if ident == function_name {
                let character = code[..name_start].encode_utf16().count();
                return Some((line_idx, character));
            }

            search_from = fn_kw_idx + 3;
        }
    }

    None
}

/// Read `file_path` into a `Vec<String>` of lines, caching the result.
///
/// On the first call for a given path the file is read from disk and stored in
/// `file_lines_cache`. Subsequent calls return the cached slice without I/O.
/// Returns `None` if the file cannot be read.
pub(crate) fn read_file_lines_cached<'a>(
    file_path: &str,
    file_lines_cache: &'a mut HashMap<String, Vec<String>>,
) -> Option<&'a Vec<String>> {
    if !file_lines_cache.contains_key(file_path) {
        let content = fs::read_to_string(file_path).ok()?;
        let lines = content
            .lines()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        file_lines_cache.insert(file_path.to_string(), lines);
    }
    file_lines_cache.get(file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_from(content: &str) -> Vec<String> {
        content.lines().map(|l| l.to_string()).collect()
    }

    #[test]
    fn find_function_name_position_returns_none_for_empty_lines() {
        let pos = find_function_name_position(&[], 1, "foo");
        assert_eq!(pos, None);
    }

    #[test]
    fn find_function_name_position_locates_simple_fn() {
        let lines = lines_from("pub fn foo(a: u32) {}");
        let (line_idx, char_idx) = find_function_name_position(&lines, 1, "foo").unwrap();
        assert_eq!(line_idx, 0);
        assert_eq!(char_idx, 7);
    }

    #[test]
    fn find_function_name_position_skips_comments() {
        let lines = lines_from("// fn fake_in_comment() {}\nfn real_fn() {}");
        let (line_idx, _) = find_function_name_position(&lines, 1, "real_fn").unwrap();
        assert_eq!(line_idx, 1);
    }

    #[test]
    fn find_function_name_position_returns_none_for_missing_name() {
        let lines = lines_from("fn one() {}\nfn two() {}");
        assert!(find_function_name_position(&lines, 1, "missing").is_none());
    }

    #[test]
    fn find_function_name_position_supports_multiple_fns_on_same_line() {
        let lines = lines_from("fn a() {} fn b() {}");
        let (line_idx, _col) = find_function_name_position(&lines, 1, "b").unwrap();
        assert_eq!(line_idx, 0);
    }

    #[test]
    fn read_file_lines_cached_uses_cache_after_first_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("foo.rs");
        fs::write(&path, "line1\nline2\n").expect("write");

        let mut cache = HashMap::new();
        let path_str = path.to_string_lossy().to_string();
        {
            let lines = read_file_lines_cached(&path_str, &mut cache).expect("first read");
            assert_eq!(lines.len(), 2);
        }

        fs::remove_file(&path).expect("rm");
        let lines2 = read_file_lines_cached(&path_str, &mut cache).expect("cached read");
        assert_eq!(lines2.len(), 2);
    }

    #[test]
    fn read_file_lines_cached_returns_none_for_missing_file() {
        let mut cache = HashMap::new();
        assert!(read_file_lines_cached("/nonexistent/zzz.rs", &mut cache).is_none());
    }
}
