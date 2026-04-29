//! String-literal-aware stripping of trailing `//` line comments.
//!
//! The naive `line.split("//").next()` approach incorrectly truncates lines
//! that contain `//` inside a string literal (e.g. `register("https://x.com")`).
//! [`strip_line_comment`] walks the line character-by-character and only
//! treats `//` as a comment marker when it is NOT inside a `"..."` string
//! literal or a `'..'` char literal, while honoring escape sequences.
//!
//! This helper is intentionally lexer-light: it does not understand raw
//! strings (`r"..."`, `r#"..."#`) or byte strings, but it correctly handles
//! the common cases that produced bug reports against the previous naive
//! splitter (URLs and `//` substrings inside ordinary `"..."` literals).
//!
//! Returns a borrowed slice of the input — the comment is stripped via a
//! prefix slice, so no allocation is performed.

/// Return the prefix of `line` that is not part of a `//` line comment.
///
/// `//` outside of a string or char literal terminates the prefix; inside
/// a literal it is preserved. Trailing whitespace before the comment marker
/// is preserved (callers that need trimming should `.trim_end()` afterwards).
///
/// The function handles the standard `\\` and `\"` (and `\'`) escape
/// sequences but does not interpret raw strings — see module docs.
pub(super) fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_string = false;
    let mut in_char = false;
    let mut escape = false;

    while i < bytes.len() {
        let b = bytes[i];

        if escape {
            escape = false;
            i += 1;
            continue;
        }

        if in_string {
            match b {
                b'\\' => escape = true,
                b'"' => in_string = false,
                _ => {}
            }
            i += 1;
            continue;
        }

        if in_char {
            match b {
                b'\\' => escape = true,
                b'\'' => in_char = false,
                _ => {}
            }
            i += 1;
            continue;
        }

        match b {
            b'"' => {
                in_string = true;
                i += 1;
            }
            b'\'' => {
                in_char = true;
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                return &line[..i];
            }
            _ => i += 1,
        }
    }

    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_preserves_double_slash_inside_string_literal() {
        let input = r#"let x = "//"; foo()"#;
        assert_eq!(strip_line_comment(input), input);
    }

    #[test]
    fn strip_preserves_url_and_double_slash_string_literal() {
        let input = r#"let x = "https://example.com"; foo("//")"#;
        assert_eq!(strip_line_comment(input), input);
    }

    #[test]
    fn strip_removes_real_trailing_comment() {
        let input = "foo() // real comment";
        assert_eq!(strip_line_comment(input), "foo() ");
    }

    #[test]
    fn strip_preserves_char_literal_with_slash() {
        let input = "let x = '/'; foo()";
        assert_eq!(strip_line_comment(input), input);
    }

    #[test]
    fn strip_handles_escaped_quote_then_strips_comment() {
        let input = r#"let x = "with \" inside"; // comment"#;
        let expected = r#"let x = "with \" inside"; "#;
        assert_eq!(strip_line_comment(input), expected);
    }

    #[test]
    fn strip_passes_through_line_without_comment() {
        let input = "fn run() { helper(value); }";
        assert_eq!(strip_line_comment(input), input);
    }

    #[test]
    fn strip_returns_empty_when_line_is_only_comment() {
        let input = "// nothing else";
        assert_eq!(strip_line_comment(input), "");
    }

    #[test]
    fn strip_preserves_url_call_site_args() {
        // Regression: the bug report's canonical example.
        let input = r#"register("https://example.com")"#;
        assert_eq!(strip_line_comment(input), input);
    }

    #[test]
    fn strip_handles_escaped_backslash_in_string() {
        // Backslash-backslash should consume only the second backslash; the
        // following quote correctly closes the string.
        let input = r#"let x = "ends\\"; // c"#;
        let expected = r#"let x = "ends\\"; "#;
        assert_eq!(strip_line_comment(input), expected);
    }

    #[test]
    fn strip_handles_single_slash_outside_string() {
        let input = "let q = a / b; foo()";
        assert_eq!(strip_line_comment(input), input);
    }
}
