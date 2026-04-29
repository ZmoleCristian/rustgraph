//! Source-slice helpers: extract lines from a file or look up a symbol by ID.

use super::symbols::make_named_symbol_id;
use crate::api::{FileSlice, SourceSlice};
use crate::function_id;
use crate::project::ProjectData;
use std::fs;
use std::path::Path;

/// Read `path` and return the content of lines `start_line..=end_line`.
///
/// Both bounds are clamped to valid line numbers. Line numbers are 1-based.
/// `byte_start` and `byte_end` are byte offsets into the original file
/// (CRLF-aware: a `\r\n` terminator counts as two bytes). The returned
/// `content` joins lines with `\n` regardless of the file's terminator
/// convention, so for CRLF inputs `content.len()` may be smaller than
/// `byte_end - byte_start`.
pub fn file_slice_for_lines(
    path: &Path,
    start_line: usize,
    end_line: usize,
) -> Result<FileSlice, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let line_spans = compute_line_spans(&content);
    let line_count = line_spans.len();
    let safe_start = start_line.max(1).min(line_count.max(1));
    let safe_end = end_line.max(safe_start).min(line_count.max(safe_start));


    let (byte_start, byte_end) = if line_count == 0 {
        (0usize, 0usize)
    } else {
        let (start_off, _) = line_spans[safe_start - 1];
        let (_, end_off) = line_spans[safe_end - 1];
        (start_off, end_off)
    };


    let mut selected = String::new();
    for (idx, &(s, e)) in line_spans.iter().enumerate() {
        let line_no = idx + 1;
        if line_no < safe_start || line_no > safe_end {
            continue;
        }
        if !selected.is_empty() {
            selected.push('\n');
        }
        selected.push_str(&content[s..e]);
    }

    Ok(FileSlice {
        file_path: path.to_string_lossy().to_string(),
        start_line: safe_start,
        end_line: safe_end,
        byte_start,
        byte_end,
        content: selected,
    })
}

/// Compute `(content_start, content_end)` byte offsets for every line in
/// `content`. The end offset excludes the trailing line terminator (`\n`
/// or `\r\n`). Bare `\r` is **not** treated as a line terminator, matching
/// the behaviour of `str::lines()`.
fn compute_line_spans(content: &str) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut spans = Vec::new();
    let mut line_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            let content_end = if i > line_start && bytes[i - 1] == b'\r' { i - 1 } else { i };
            spans.push((line_start, content_end));
            line_start = i + 1;
        }
        i += 1;
    }
    if line_start < bytes.len() {

        spans.push((line_start, bytes.len()));
    }
    spans
}

/// Wrap [`file_slice_for_lines`] in a [`SourceSlice`] with an optional symbol ID annotation.
pub fn source_slice_for_lines(
    path: &Path,
    start_line: usize,
    end_line: usize,
    symbol_id: Option<String>,
) -> Result<SourceSlice, Box<dyn std::error::Error>> {
    let slice = file_slice_for_lines(path, start_line, end_line)?;
    Ok(SourceSlice { symbol_id, slice })
}

/// Look up `symbol_id` in `project` and return its source slice.
///
/// Searches functions first, then structs, then enums. Returns an error if the
/// symbol is not found.
pub fn source_slice_for_symbol(
    project: &ProjectData,
    symbol_id: &str,
) -> Result<SourceSlice, Box<dyn std::error::Error>> {


    if let Some(function) = project
        .functions
        .iter()
        .find(|function| function_id(function) == symbol_id)
    {
        let resolved = crate::index::resolve_read_path(&function.file_path);
        return source_slice_for_lines(
            &resolved,
            function.start_line,
            function.end_line,
            Some(symbol_id.to_string()),
        );
    }

    if let Some(struct_info) = project.structs.iter().find(|item| {
        make_named_symbol_id("struct", &item.file_path, item.start_line, &item.name) == symbol_id
    }) {
        let resolved = crate::index::resolve_read_path(&struct_info.file_path);
        return source_slice_for_lines(
            &resolved,
            struct_info.start_line,
            struct_info.end_line,
            Some(symbol_id.to_string()),
        );
    }

    if let Some(enum_info) = project.enums.iter().find(|item| {
        make_named_symbol_id("enum", &item.file_path, item.start_line, &item.name) == symbol_id
    }) {
        let resolved = crate::index::resolve_read_path(&enum_info.file_path);
        return source_slice_for_lines(
            &resolved,
            enum_info.start_line,
            enum_info.end_line,
            Some(symbol_id.to_string()),
        );
    }

    Err(format!("symbol not found: {symbol_id}").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write<'a>(path: &'a Path, content: &str) -> &'a Path {
        fs::write(path, content).expect("write");
        path
    }

    #[test]
    fn file_slice_for_lines_clamps_to_file_bounds() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        write(&path, "alpha\nbeta\ngamma\n");
        let slice = file_slice_for_lines(&path, 1, 100).expect("slice");
        assert_eq!(slice.start_line, 1);
        assert_eq!(slice.end_line, 3);
        assert!(slice.content.contains("alpha"));
        assert!(slice.content.contains("gamma"));
    }

    #[test]
    fn file_slice_for_lines_single_line_range() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        write(&path, "alpha\nbeta\ngamma\n");
        let slice = file_slice_for_lines(&path, 2, 2).expect("slice");
        assert_eq!(slice.content, "beta");
        assert_eq!(slice.start_line, 2);
        assert_eq!(slice.end_line, 2);
    }

    #[test]
    fn file_slice_for_lines_zero_start_clamps_to_one() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        write(&path, "alpha\nbeta\n");
        let slice = file_slice_for_lines(&path, 0, 1).expect("slice");
        assert_eq!(slice.start_line, 1);
        assert_eq!(slice.end_line, 1);
    }

    #[test]
    fn file_slice_for_lines_end_before_start_clamps_to_start() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        write(&path, "alpha\nbeta\ngamma\n");
        let slice = file_slice_for_lines(&path, 3, 1).expect("slice");
        assert_eq!(slice.start_line, 3);
        assert_eq!(slice.end_line, 3);
    }

    #[test]
    fn file_slice_byte_indices_track_content_length() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        write(&path, "abc\nxyz\n");
        let slice = file_slice_for_lines(&path, 2, 2).expect("slice");
        assert_eq!(slice.byte_start, "abc\n".len());
        assert_eq!(slice.byte_end, slice.byte_start + slice.content.len());
    }

    #[test]
    fn file_slice_handles_crlf_byte_offsets_correctly() {

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("crlf.rs");
        write(&path, "alpha\r\nbeta\r\ngamma\r\n");




        let slice = file_slice_for_lines(&path, 2, 2).expect("slice");
        assert_eq!(slice.start_line, 2);
        assert_eq!(slice.end_line, 2);
        assert_eq!(slice.content, "beta");
        assert_eq!(slice.byte_start, "alpha\r\n".len(), "byte_start must skip CRLF terminator");
        assert_eq!(slice.byte_end, "alpha\r\n".len() + "beta".len());


        let slice2 = file_slice_for_lines(&path, 1, 2).expect("slice");
        assert_eq!(slice2.start_line, 1);
        assert_eq!(slice2.end_line, 2);
        assert_eq!(slice2.content, "alpha\nbeta");
        assert_eq!(slice2.byte_start, 0);
        assert_eq!(slice2.byte_end, "alpha\r\nbeta".len());
    }

    #[test]
    fn file_slice_handles_mixed_lf_and_crlf() {

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("mixed.rs");
        write(&path, "one\ntwo\r\nthree\n");
        let slice = file_slice_for_lines(&path, 3, 3).expect("slice");
        assert_eq!(slice.content, "three");
        assert_eq!(slice.byte_start, "one\ntwo\r\n".len());
        assert_eq!(slice.byte_end, slice.byte_start + "three".len());
    }

    #[test]
    fn source_slice_for_lines_carries_optional_symbol_id() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        write(&path, "x\n");
        let slice = source_slice_for_lines(&path, 1, 1, Some("sym".into())).expect("slice");
        assert_eq!(slice.symbol_id.as_deref(), Some("sym"));
    }

    #[test]
    fn source_slice_for_symbol_returns_err_for_unknown_id() {
        let project = ProjectData::default();
        let err = source_slice_for_symbol(&project, "missing").err().unwrap();
        assert!(err.to_string().contains("symbol not found"));
    }
}
