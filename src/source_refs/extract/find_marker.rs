//! Locating a callee marker (`name(`) in source lines and clamping byte indices
//! to valid UTF-8 character boundaries.

/// Advance `byte_idx` forward until it lands on a UTF-8 character boundary in `text`.
///
/// If `byte_idx` is already aligned, or exceeds `text.len()`, the value is
/// returned as-is (clamped to `text.len()`).
pub(crate) fn clamp_to_char_boundary(text: &str, byte_idx: usize) -> usize {
    let mut idx = byte_idx.min(text.len());
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_to_char_boundary_preserves_aligned_index() {
        assert_eq!(clamp_to_char_boundary("hello", 0), 0);
        assert_eq!(clamp_to_char_boundary("hello", 5), 5);
    }

    #[test]
    fn clamp_to_char_boundary_advances_into_multibyte_chars() {

        let text = "héllo";


        assert_eq!(clamp_to_char_boundary(text, 2), 3);
        assert_eq!(clamp_to_char_boundary(text, 1), 1);
        assert_eq!(clamp_to_char_boundary(text, 3), 3);
    }

    #[test]
    fn clamp_to_char_boundary_clamps_oversize_to_text_length() {
        assert_eq!(clamp_to_char_boundary("abc", 100), 3);
    }

    #[test]
    fn clamp_to_char_boundary_handles_empty_string() {
        assert_eq!(clamp_to_char_boundary("", 0), 0);
        assert_eq!(clamp_to_char_boundary("", 7), 0);
    }

    #[test]
    fn find_marker_in_lines_locates_simple_call() {
        let lines = vec![
            "fn run() {".to_string(),
            "    helper(value);".to_string(),
            "}".to_string(),
        ];
        let pos = find_marker_in_lines(&lines, 2, 0, "helper");

        assert_eq!(pos, Some((1, "    helper(".len())));
    }

    #[test]
    fn find_marker_in_lines_returns_none_when_marker_absent() {
        let lines = vec!["fn run() { other(); }".to_string()];
        let pos = find_marker_in_lines(&lines, 1, 0, "missing");
        assert!(pos.is_none());
    }

    #[test]
    fn find_marker_in_lines_skips_comment_only_match() {
        let lines = vec!["// helper(x);".to_string(), "real(y);".to_string()];
        let pos = find_marker_in_lines(&lines, 1, 0, "helper");

        assert!(pos.is_none());
    }
}

/// Locate the opening `(` of a call to `callee_base` starting near `line`/`column`.
///
/// Searches up to 64 lines from the hint position and skips matches inside
/// `//` line comments. Returns `(line_index, byte_offset_after_paren)` on
/// success, or `None` if the marker is not found.
pub(crate) fn find_marker_in_lines(
    lines: &[String],
    line: usize,
    column: usize,
    callee_base: &str,
) -> Option<(usize, usize)> {
    let marker = format!("{}(", callee_base);
    let start_line = line.saturating_sub(1).min(lines.len().saturating_sub(1));
    let search_end = (start_line + 64).min(lines.len());

    let mut marker_location: Option<(usize, usize)> = None;
    for (idx, line_text_raw) in lines.iter().enumerate().take(search_end).skip(start_line) {
        let line_text = super::strip_comment::strip_line_comment(line_text_raw);
        let search_from = if idx == start_line {
            clamp_to_char_boundary(line_text, column)
        } else {
            0
        };
        let marker_start = if search_from < line_text.len() {
            if let Some(found_at) = line_text[search_from..].find(&marker) {
                search_from + found_at
            } else if let Some(found_at) = line_text.find(&marker) {
                found_at
            } else {
                continue;
            }
        } else if let Some(found_at) = line_text.find(&marker) {
            found_at
        } else {
            continue;
        };
        marker_location = Some((idx, marker_start + marker.len()));
        break;
    }
    marker_location
}
