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
/// Byte offsets in the returned [`FileSlice`] are relative to the start of
/// the file.
pub fn file_slice_for_lines(
    path: &Path,
    start_line: usize,
    end_line: usize,
) -> Result<FileSlice, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let lines = content.lines().collect::<Vec<_>>();
    let safe_start = start_line.max(1).min(lines.len().max(1));
    let safe_end = end_line.max(safe_start).min(lines.len().max(safe_start));

    let mut byte_start = 0usize;
    for line in lines.iter().take(safe_start.saturating_sub(1)) {
        byte_start += line.len() + 1;
    }

    let mut selected = String::new();
    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        if line_no < safe_start || line_no > safe_end {
            continue;
        }
        if !selected.is_empty() {
            selected.push('\n');
        }
        selected.push_str(line);
    }
    let byte_end = byte_start + selected.len();

    Ok(FileSlice {
        file_path: path.to_string_lossy().to_string(),
        start_line: safe_start,
        end_line: safe_end,
        byte_start,
        byte_end,
        content: selected,
    })
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
