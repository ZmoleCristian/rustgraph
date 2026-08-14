/// Extract all identifier tokens from `text`, filtering out Rust keywords and
/// tokens that start with a digit.  Returns a sorted, deduplicated list.
pub(super) fn extract_identifier_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();

    for ch in text.chars().chain(std::iter::once(' ')) {
        if ch.is_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else if !cur.is_empty() {
            if cur
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && cur != "mut"
                && cur != "let"
                && cur != "ref"
                && cur != "self"
                && cur != "true"
                && cur != "false"
            {
                out.push(cur.clone());
            }
            cur.clear();
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Return the variable name on the left-hand side of a `let [mut] var = callee_base(...)` assignment,
/// or `None` if the line is not a simple let-binding into `callee_base`.
pub(super) fn extract_assigned_var_from_call_line(
    line_text: &str,
    callee_base: &str,
) -> Option<String> {
    let marker = format!("{callee_base}(");
    let call_pos = line_text.find(&marker)?;
    let lhs = line_text[..call_pos]
        .rsplit_once('=')
        .map(|(left, _)| left.trim())?;
    let lhs = lhs
        .trim_start_matches("let ")
        .trim_start_matches("mut ")
        .trim();
    let var = lhs
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|segment| !segment.is_empty())?;
    if var
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
    {
        Some(var.to_string())
    } else {
        None
    }
}

/// Return the raw argument string inside `callee_base(...)` on `line_text`,
/// or `None` if the marker or closing parenthesis is absent.
pub(super) fn extract_call_arg_segment(line_text: &str, callee_base: &str) -> Option<String> {
    let marker = format!("{callee_base}(");
    let start = line_text.find(&marker)? + marker.len();
    let rest = &line_text[start..];
    let end = rest.rfind(')')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_identifier_list_returns_dedup_sorted_words() {
        let out = extract_identifier_list("foo, bar, foo");
        assert_eq!(out, vec!["bar".to_string(), "foo".to_string()]);
    }

    #[test]
    fn extract_identifier_list_filters_keywords_and_literals() {
        let out = extract_identifier_list("let mut foo = ref bar; self.x; true; false");
        assert!(out.contains(&"foo".to_string()));
        assert!(out.contains(&"bar".to_string()));
        assert!(out.contains(&"x".to_string()));
        assert!(!out.contains(&"let".to_string()));
        assert!(!out.contains(&"mut".to_string()));
        assert!(!out.contains(&"ref".to_string()));
        assert!(!out.contains(&"self".to_string()));
        assert!(!out.contains(&"true".to_string()));
        assert!(!out.contains(&"false".to_string()));
    }

    #[test]
    fn extract_identifier_list_filters_numeric_starts() {
        let out = extract_identifier_list("1abc, _ok");
        assert!(out.contains(&"_ok".to_string()));
        assert!(!out.iter().any(|s| s.starts_with('1')));
    }

    #[test]
    fn extract_assigned_var_from_call_line_handles_let_binding() {
        let out = extract_assigned_var_from_call_line("    let foo = build(arg);", "build");
        assert_eq!(out, Some("foo".to_string()));
    }

    #[test]
    fn extract_assigned_var_from_call_line_handles_let_mut_binding() {
        let out = extract_assigned_var_from_call_line("    let mut foo = build(arg);", "build");
        assert_eq!(out, Some("foo".to_string()));
    }

    #[test]
    fn extract_assigned_var_from_call_line_returns_none_when_no_assignment() {
        assert_eq!(
            extract_assigned_var_from_call_line("    build(x);", "build"),
            None
        );
    }

    #[test]
    fn extract_assigned_var_from_call_line_returns_none_when_marker_missing() {
        assert_eq!(
            extract_assigned_var_from_call_line("let x = noop();", "build"),
            None
        );
    }

    #[test]
    fn extract_call_arg_segment_returns_inner_args() {
        let out = extract_call_arg_segment("call(foo, bar);", "call");
        assert_eq!(out, Some("foo, bar".to_string()));
    }

    #[test]
    fn extract_call_arg_segment_handles_no_args() {
        let out = extract_call_arg_segment("call();", "call");
        assert_eq!(out, Some(String::new()));
    }

    #[test]
    fn extract_call_arg_segment_returns_none_when_marker_missing() {
        let out = extract_call_arg_segment("other(x)", "call");
        assert_eq!(out, None);
    }
}
