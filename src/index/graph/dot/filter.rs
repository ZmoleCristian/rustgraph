pub(crate) fn should_include_external_call(callee: &str) -> bool {
    if let Some(method) = callee.strip_prefix("receiver.") {
        !matches!(
            method,
            "to_string"
                | "clone"
                | "len"
                | "is_empty"
                | "iter"
                | "collect"
                | "map"
                | "filter"
                | "push"
                | "pop"
                | "bold"
                | "italic"
                | "dimmed"
                | "bright_green"
                | "bright_red"
                | "bright_blue"
                | "bright_yellow"
                | "bright_cyan"
                | "bright_magenta"
                | "bright_purple"
                | "white"
                | "cyan"
                | "green"
        )
    } else if callee.contains("::") {
        callee.starts_with("std::")
            || callee.starts_with("serde")
            || callee.starts_with("syn::")
            || callee.contains("::new")
            || callee.contains("::parse")
            || callee.contains("::read")
            || callee.contains("::write")
            || !callee.contains(".")
    } else if callee.ends_with('!') {
        !matches!(callee, "format!" | "println!" | "eprintln!")
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_trivial_receiver_methods() {
        assert!(!should_include_external_call("receiver.to_string"));
        assert!(!should_include_external_call("receiver.clone"));
        assert!(!should_include_external_call("receiver.iter"));
        assert!(!should_include_external_call("receiver.collect"));
    }

    #[test]
    fn includes_unknown_receiver_methods() {
        assert!(should_include_external_call("receiver.do_thing"));
    }

    #[test]
    fn includes_std_paths() {
        assert!(should_include_external_call("std::fs::read"));
    }

    #[test]
    fn includes_constructors_and_parsers() {
        assert!(should_include_external_call("Foo::new"));
        assert!(should_include_external_call("Bar::parse_input"));
    }

    #[test]
    fn excludes_simple_format_macros() {
        assert!(!should_include_external_call("format!"));
        assert!(!should_include_external_call("println!"));
        assert!(!should_include_external_call("eprintln!"));
    }

    #[test]
    fn includes_other_macros() {
        assert!(should_include_external_call("vec!"));
        assert!(should_include_external_call("custom!"));
    }

    #[test]
    fn includes_simple_function_names() {
        assert!(should_include_external_call("local_function"));
    }
}
