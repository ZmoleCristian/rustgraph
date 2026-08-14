//! Argument splitting that respects nesting of `()`, `[]`, and `{}`.

/// Split `body` on top-level commas, returning one trimmed string per argument.
///
/// Commas inside parentheses, brackets, or braces are not treated as
/// separators. A trailing comma produces no empty entry.
pub(crate) fn split_top_level_call_args(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;

    for ch in body.chars() {
        match ch {
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(ch);
            }
            '[' => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                current.push(ch);
            }
            '{' => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_empty_returns_empty() {
        assert!(split_top_level_call_args("").is_empty());
        assert!(split_top_level_call_args("   ").is_empty());
    }

    #[test]
    fn split_single_arg_returns_single_element() {
        assert_eq!(
            split_top_level_call_args("alpha"),
            vec!["alpha".to_string()]
        );
    }

    #[test]
    fn split_multiple_simple_args() {
        assert_eq!(
            split_top_level_call_args("a, b, c"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn split_trims_whitespace_around_each_arg() {
        assert_eq!(
            split_top_level_call_args("  a  ,  b , c   "),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn split_preserves_commas_inside_brackets() {
        assert_eq!(
            split_top_level_call_args("[a, b], c"),
            vec!["[a, b]".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn split_preserves_commas_inside_braces() {
        assert_eq!(
            split_top_level_call_args("{a: 1, b: 2}, c"),
            vec!["{a: 1, b: 2}".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn split_preserves_commas_inside_nested_calls() {
        assert_eq!(
            split_top_level_call_args("inner(a, b), outer(c)"),
            vec!["inner(a, b)".to_string(), "outer(c)".to_string()]
        );
    }

    #[test]
    fn split_with_unbalanced_closing_does_not_panic() {
        let result = split_top_level_call_args(") a, b");
        assert_eq!(result, vec![") a".to_string(), "b".to_string()]);
    }

    #[test]
    fn split_handles_trailing_comma_skipping_empty_part() {
        assert_eq!(
            split_top_level_call_args("a, b,"),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn split_handles_mixed_brackets_braces_parens() {
        let args = split_top_level_call_args(
            "state, router(get(handler)), Config { key: vec![one, two] }, tail",
        );
        assert_eq!(
            args,
            vec![
                "state",
                "router(get(handler))",
                "Config { key: vec![one, two] }",
                "tail"
            ]
        );
    }
}
