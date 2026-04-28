//! Extracts the raw argument text of a call expression from pre-split source lines.

/// Extract the content inside the parentheses of a call starting at
/// `(marker_line, marker_col)` in `lines`.
///
/// `marker_col` must point to the byte just after the opening `(`. Scans up
/// to 160 additional lines and strips `//` line comments. Returns `None` if
/// the parentheses are never closed or the body is empty.
pub(crate) fn extract_call_args_body(
    lines: &[String],
    marker_line: usize,
    marker_col: usize,
) -> Option<String> {
    let parse_end = (marker_line + 160).min(lines.len());
    let mut body = String::new();
    let mut paren_depth = 1usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut closed = false;

    for (idx, line_text_raw) in lines.iter().enumerate().take(parse_end).skip(marker_line) {
        let line_text = line_text_raw.split("//").next().unwrap_or("");
        let segment = if idx == marker_line {
            let start = super::find_marker::clamp_to_char_boundary(line_text, marker_col);
            if start >= line_text.len() {
                ""
            } else {
                &line_text[start..]
            }
        } else {
            line_text
        };

        for ch in segment.chars() {
            match ch {
                '(' => {
                    paren_depth += 1;
                    body.push(ch);
                }
                ')' => {
                    paren_depth = paren_depth.saturating_sub(1);
                    if paren_depth == 0 {
                        closed = true;
                        break;
                    }
                    body.push(ch);
                }
                '[' => {
                    bracket_depth += 1;
                    body.push(ch);
                }
                ']' => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                    body.push(ch);
                }
                '{' => {
                    brace_depth += 1;
                    body.push(ch);
                }
                '}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                    body.push(ch);
                }
                _ => body.push(ch),
            }
        }

        if closed {
            break;
        }
        body.push('\n');
    }

    if !closed || body.trim().is_empty() {
        return None;
    }

    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extract_call_args_body_single_line() {
        let l = lines("foo(a, b);");

        let body = extract_call_args_body(&l, 0, 4).expect("body");
        assert_eq!(body, "a, b");
    }

    #[test]
    fn extract_call_args_body_balanced_nested_parens() {
        let l = lines("outer(inner(x), y);");
        let body = extract_call_args_body(&l, 0, 6).expect("body");
        assert_eq!(body, "inner(x), y");
    }

    #[test]
    fn extract_call_args_body_returns_none_when_not_closed() {
        let l = lines("foo(unclosed");
        assert!(extract_call_args_body(&l, 0, 4).is_none());
    }

    #[test]
    fn extract_call_args_body_handles_brackets_and_braces_inside() {
        let l = lines("foo(vec![1, 2], Map { k: 1 });");
        let body = extract_call_args_body(&l, 0, 4).expect("body");
        assert_eq!(body, "vec![1, 2], Map { k: 1 }");
    }

    #[test]
    fn extract_call_args_body_handles_multiline() {
        let l = lines("foo(\n    a,\n    b,\n);");
        let body = extract_call_args_body(&l, 0, 4).expect("body");
        assert!(body.contains("a"));
        assert!(body.contains("b"));
    }

    #[test]
    fn extract_call_args_body_strips_line_comments() {
        let l = lines("foo(a, // unsupported comment with )\nb);");
        let body = extract_call_args_body(&l, 0, 4).expect("body");
        assert!(body.contains("a"));
        assert!(body.contains("b"));

    }
}
