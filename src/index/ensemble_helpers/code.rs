use crate::index::{CodeLine, CodeSnippet, resolve_read_path};
use std::fs;

/// Read lines `start_line..=end_line` (1-based, inclusive) from `file_path`
/// and return them as a [`CodeSnippet`].
///
/// The end line is clamped to the actual file length.  Returns an error if
/// the file cannot be read.
pub fn extract_code_snippet(
    file_path: &str,
    start_line: usize,
    end_line: usize,
) -> Result<CodeSnippet, Box<dyn std::error::Error>> {
    let resolved = resolve_read_path(file_path);
    let content = fs::read_to_string(&resolved)?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        return Ok(CodeSnippet {
            file_path: file_path.to_string(),
            start_line,
            end_line,
            lines: Vec::new(),
        });
    }

    let start_idx = start_line.saturating_sub(1).min(lines.len());
    let end_line = end_line.max(start_line);
    let end_idx = end_line.min(lines.len());

    let mut out_lines = Vec::new();
    for (offset, line) in lines[start_idx..end_idx].iter().enumerate() {
        out_lines.push(CodeLine {
            line: start_line + offset,
            text: (*line).to_string(),
        });
    }

    Ok(CodeSnippet {
        file_path: file_path.to_string(),
        start_line,
        end_line: end_idx,
        lines: out_lines,
    })
}

/// Parse a `path:line` or `path#Lline` query string into its path and line
/// number components.  Also accepts the symbol-id triple `path:line:name`
/// (with optional `fn:` prefix), dropping the trailing name.  Returns `None`
/// when the query does not match any format or the line portion is not a
/// valid integer.
pub fn parse_file_line_query(query: &str) -> Option<(String, usize)> {
    let query = query.strip_prefix("fn:").unwrap_or(query);

    if let Some((path, line_str)) = query.rsplit_once("#L")
        && let Ok(line) = line_str.trim().parse::<usize>()
    {
        let path = path.trim();
        if !path.is_empty() {
            return Some((path.to_string(), line));
        }
    }

    if let Some((path, line_str)) = query.rsplit_once(':')
        && let Ok(line) = line_str.trim().parse::<usize>()
    {
        let path = path.trim();
        if !path.is_empty() {
            return Some((path.to_string(), line));
        }
    }

    if let Some((path, line, _name)) = parse_symbol_id_query(query) {
        return Some((path, line));
    }

    None
}

/// Parse a function symbol-id query `path:line:name` (with optional `fn:`
/// prefix), as emitted by ambiguity errors and `--json` output, into its
/// path, line, and name components.
pub fn parse_symbol_id_query(query: &str) -> Option<(String, usize, String)> {
    let query = query.strip_prefix("fn:").unwrap_or(query);

    let (rest, name) = query.rsplit_once(':')?;
    let name = name.trim();
    let (path, line_str) = rest.rsplit_once(':')?;
    let line = line_str.trim().parse::<usize>().ok()?;
    let path = path.trim();
    if path.is_empty() || name.is_empty() {
        return None;
    }
    Some((path.to_string(), line, name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(content.as_bytes()).expect("write");

        let leaked = dir.keep();
        leaked.join(name)
    }

    #[test]
    fn extract_code_snippet_returns_lines_in_range() {
        let path = write_temp_file("snip.rs", "line1\nline2\nline3\nline4\nline5");
        let snip = extract_code_snippet(path.to_str().unwrap(), 2, 4).expect("snippet");
        assert_eq!(snip.lines.len(), 3);
        assert_eq!(snip.lines[0].line, 2);
        assert_eq!(snip.lines[0].text, "line2");
        assert_eq!(snip.lines[2].text, "line4");
        assert_eq!(snip.start_line, 2);
        assert_eq!(snip.end_line, 4);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_code_snippet_clamps_end_to_file_length() {
        let path = write_temp_file("snip2.rs", "a\nb\nc");
        let snip = extract_code_snippet(path.to_str().unwrap(), 2, 100).expect("snippet");
        assert_eq!(snip.lines.len(), 2);
        assert_eq!(snip.end_line, 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_code_snippet_returns_empty_for_empty_file() {
        let path = write_temp_file("empty.rs", "");
        let snip = extract_code_snippet(path.to_str().unwrap(), 1, 5).expect("snippet");
        assert!(snip.lines.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_code_snippet_reports_error_when_file_missing() {
        assert!(extract_code_snippet("/nonexistent/path/zzz.rs", 1, 5).is_err());
    }

    #[test]
    fn parse_file_line_query_handles_hash_l_form() {
        assert_eq!(
            parse_file_line_query("src/lib.rs#L12"),
            Some(("src/lib.rs".to_string(), 12))
        );
    }

    #[test]
    fn parse_file_line_query_handles_colon_form() {
        assert_eq!(
            parse_file_line_query("src/lib.rs:42"),
            Some(("src/lib.rs".to_string(), 42))
        );
    }

    #[test]
    fn parse_file_line_query_returns_none_without_line_number() {
        assert_eq!(parse_file_line_query("just_a_name"), None);
    }

    #[test]
    fn parse_file_line_query_returns_none_for_unparseable_line() {
        assert_eq!(parse_file_line_query("src/lib.rs:abc"), None);
    }

    #[test]
    fn parse_file_line_query_returns_none_for_empty_path() {
        assert_eq!(parse_file_line_query(":12"), None);
    }

    #[test]
    fn parse_file_line_query_accepts_symbol_id_triple() {
        assert_eq!(
            parse_file_line_query("src/session/cli.rs:90:run_cli"),
            Some(("src/session/cli.rs".to_string(), 90))
        );
    }

    #[test]
    fn parse_symbol_id_query_parses_triple() {
        assert_eq!(
            parse_symbol_id_query("src/session/cli.rs:90:run_cli"),
            Some(("src/session/cli.rs".to_string(), 90, "run_cli".to_string()))
        );
    }

    #[test]
    fn parse_symbol_id_query_strips_fn_prefix() {
        assert_eq!(
            parse_symbol_id_query("fn:src/a.rs:12:foo"),
            Some(("src/a.rs".to_string(), 12, "foo".to_string()))
        );
    }

    #[test]
    fn parse_symbol_id_query_rejects_plain_name_and_module_paths() {
        assert_eq!(parse_symbol_id_query("run_cli"), None);
        assert_eq!(parse_symbol_id_query("std::fs"), None);
        assert_eq!(parse_symbol_id_query("Type::method"), None);
        assert_eq!(parse_symbol_id_query("src/a.rs:12"), None);
    }
}
