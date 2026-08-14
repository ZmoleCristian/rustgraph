use colored::*;

/// Apply ANSI color coding to a Rust type string based on its category
/// (Result, Option, Vec, Box, String/str, reference, path, named type,
/// or primitive).
pub fn colorize_type(type_str: &str) -> String {
    let type_str = type_str.trim();

    if type_str.starts_with("Result") {
        type_str.bright_green().bold().to_string()
    } else if type_str.starts_with("Option") {
        type_str.bright_cyan().bold().to_string()
    } else if type_str.starts_with("Vec") {
        type_str.bright_purple().bold().to_string()
    } else if type_str.starts_with("Box") {
        type_str.bright_red().bold().to_string()
    } else if type_str.starts_with("String") || type_str.starts_with("&str") {
        type_str.bright_yellow().bold().to_string()
    } else if type_str.starts_with("&") {
        type_str.cyan().bold().to_string()
    } else if type_str.contains("::") {
        type_str.bright_magenta().bold().to_string()
    } else if type_str.chars().next().is_some_and(|c| c.is_uppercase()) {
        type_str.green().bold().to_string()
    } else {
        type_str.white().to_string()
    }
}

/// Colorize a `name: Type` parameter string, coloring the name in bright blue
/// and the type via [`colorize_type`].  Falls back to `colorize_type` when no
/// colon separator is present.
pub fn colorize_parameter(param: &str) -> String {
    let parts: Vec<&str> = param.split(':').collect();
    if parts.len() == 2 {
        let name = parts[0].trim().bright_blue().bold();
        let type_part = colorize_type(parts[1]);
        format!("{}: {}", name, type_part)
    } else {
        colorize_type(param)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' {
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next == 'm' {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn colorize_type_preserves_underlying_text() {
        for variant in [
            "Result<()>",
            "Option<u32>",
            "Vec<u8>",
            "Box<T>",
            "String",
            "&str",
            "&mut T",
            "module::Type",
            "T",
            "lowercase",
        ] {
            let out = colorize_type(variant);
            let stripped = strip_ansi(&out);
            assert_eq!(stripped.trim(), variant);
        }
    }

    #[test]
    fn colorize_type_trims_whitespace_from_input() {
        let out = colorize_type("  Vec<u8>  ");
        let stripped = strip_ansi(&out);
        assert_eq!(stripped.trim(), "Vec<u8>");
    }

    #[test]
    fn colorize_parameter_splits_name_and_type() {
        let out = colorize_parameter("count: usize");
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("count"));
        assert!(stripped.contains("usize"));
        assert!(stripped.contains(": "));
    }

    #[test]
    fn colorize_parameter_falls_back_to_colorize_type_when_no_colon() {
        let out = colorize_parameter("usize");
        assert_eq!(strip_ansi(&out).trim(), "usize");
    }

    #[test]
    fn colorize_parameter_handles_multiple_colons_via_type_only() {
        let out = colorize_parameter("a::b::C");
        assert_eq!(strip_ansi(&out).trim(), "a::b::C");
    }
}
