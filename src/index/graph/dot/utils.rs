pub(crate) fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn callee_base_name(callee: &str) -> String {
    let without_bang = callee.trim_end_matches('!');
    let after_path = without_bang.rsplit("::").next().unwrap_or(without_bang);
    after_path
        .rsplit('.')
        .next()
        .unwrap_or(after_path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_escape_escapes_backslash_and_quote() {
        assert_eq!(dot_escape("foo"), "foo");
        assert_eq!(dot_escape("foo\\bar"), "foo\\\\bar");
        assert_eq!(dot_escape("hello \"world\""), "hello \\\"world\\\"");
        assert_eq!(dot_escape("\\\""), "\\\\\\\"");
    }

    #[test]
    fn dot_escape_preserves_other_characters() {
        assert_eq!(dot_escape("a:b:c"), "a:b:c");
        assert_eq!(dot_escape("with newline\nrest"), "with newline\nrest");
    }

    #[test]
    fn callee_base_name_strips_macro_bang() {
        assert_eq!(callee_base_name("println!"), "println");
    }

    #[test]
    fn callee_base_name_drops_path_segments() {
        assert_eq!(callee_base_name("std::collections::HashMap"), "HashMap");
        assert_eq!(callee_base_name("crate::api::run"), "run");
    }

    #[test]
    fn callee_base_name_strips_method_receiver() {
        assert_eq!(callee_base_name("self.helper"), "helper");
        assert_eq!(callee_base_name("config.builder.build"), "build");
    }

    #[test]
    fn callee_base_name_handles_path_then_method() {
        assert_eq!(callee_base_name("std::path.join"), "join");
    }

    #[test]
    fn callee_base_name_returns_input_when_no_separator() {
        assert_eq!(callee_base_name("plain"), "plain");
    }
}
