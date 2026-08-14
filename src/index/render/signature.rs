use super::color::colorize_parameter;
use super::color::colorize_type;
use crate::ConstInfo;
use crate::EnumInfo;
use crate::FunctionInfo;
use crate::StructInfo;
use crate::TypeDeclInfo;
use colored::*;

/// Format a trait declaration or type alias as a one-line display string.
///
/// Without color: `path:start-end - <signature>`.
/// With color: file location dimmed, then a colored keyword/name line.
pub fn format_type_decl_signature(decl: &TypeDeclInfo, use_color: bool) -> String {
    if !use_color {
        return format!(
            "{}:{}-{} - {}",
            decl.file_path, decl.start_line, decl.end_line, decl.signature
        );
    }

    let file_info = format!("{}:{}-{}", decl.file_path, decl.start_line, decl.end_line)
        .dimmed()
        .italic();

    let mut signature_parts = Vec::new();

    if decl.is_pub {
        signature_parts.push("pub".bright_green().bold().to_string());
    }

    let keyword = if decl.kind == "trait" {
        "trait"
    } else {
        "type"
    };
    signature_parts.push(keyword.bright_blue().bold().to_string());
    signature_parts.push(decl.name.bright_yellow().bold().underline().to_string());

    if let Some(rhs) = &decl.rhs {
        signature_parts.push(format!("= {}", colorize_type(rhs)));
    }

    format!(
        "{} {} {}",
        file_info,
        "→".bright_white().bold(),
        signature_parts.join(" ")
    )
}

/// Format a const/static as a one-line display string.
///
/// Without color: `path:start-end - <signature>`.
/// With color: file location dimmed, then a colored keyword/name/type line.
pub fn format_const_signature(const_info: &ConstInfo, use_color: bool) -> String {
    if !use_color {
        return format!(
            "{}:{}-{} - {}",
            const_info.file_path, const_info.start_line, const_info.end_line, const_info.signature
        );
    }

    let file_info = format!(
        "{}:{}-{}",
        const_info.file_path, const_info.start_line, const_info.end_line
    )
    .dimmed()
    .italic();

    let mut signature_parts = Vec::new();

    if const_info.is_pub {
        signature_parts.push("pub".bright_green().bold().to_string());
    }

    let keyword = if const_info.kind.starts_with("static") {
        const_info.kind.as_str()
    } else {
        "const"
    };
    signature_parts.push(keyword.bright_magenta().bold().to_string());
    signature_parts.push(
        const_info
            .name
            .bright_yellow()
            .bold()
            .underline()
            .to_string(),
    );
    signature_parts.push(format!(": {}", colorize_type(&const_info.ty)));

    format!(
        "{} {} {}",
        file_info,
        "→".bright_white().bold(),
        signature_parts.join(" ")
    )
}

/// Format a struct as a one-line display string.
///
/// Without color: `path:start-end - struct Name`.
/// With color: file location dimmed, then a colored keyword/name/fields line.
pub fn format_struct_signature(struct_info: &StructInfo, use_color: bool) -> String {
    if !use_color {
        return format!(
            "{}:{}-{} - struct {}",
            struct_info.file_path, struct_info.start_line, struct_info.end_line, struct_info.name
        );
    }

    let file_info = format!(
        "{}:{}-{}",
        struct_info.file_path, struct_info.start_line, struct_info.end_line
    )
    .dimmed()
    .italic();

    let mut signature_parts = Vec::new();

    if struct_info.is_pub {
        signature_parts.push("pub".bright_green().bold().to_string());
    }

    signature_parts.push("struct".bright_blue().bold().to_string());
    signature_parts.push(
        struct_info
            .name
            .bright_yellow()
            .bold()
            .underline()
            .to_string(),
    );

    if !struct_info.generics.is_empty() {
        let colored_generics: Vec<String> = struct_info
            .generics
            .iter()
            .map(|g| colorize_type(g))
            .collect();
        let generics = format!("<{}>", colored_generics.join(", "));
        signature_parts.push(generics.bright_magenta().to_string());
    }

    if !struct_info.fields.is_empty() {
        let colored_fields: Vec<String> = struct_info
            .fields
            .iter()
            .map(|f| colorize_parameter(f))
            .collect();
        let fields = format!("{{ {} }}", colored_fields.join(", "));
        signature_parts.push(fields);
    }

    format!(
        "{} {} {}",
        file_info,
        "→".bright_white().bold(),
        signature_parts.join(" ")
    )
}

/// Format an enum as a one-line display string.
///
/// Without color: `path:start-end - enum Name`.
/// With color: file location dimmed, then a colored keyword/name/variants line
/// with discriminants and tuple-field payloads inlined.
pub fn format_enum_signature(enum_info: &EnumInfo, use_color: bool) -> String {
    if !use_color {
        return format!(
            "{}:{}-{} - enum {}",
            enum_info.file_path, enum_info.start_line, enum_info.end_line, enum_info.name
        );
    }

    let file_info = format!(
        "{}:{}-{}",
        enum_info.file_path, enum_info.start_line, enum_info.end_line
    )
    .dimmed()
    .italic();

    let mut signature_parts = Vec::new();

    if enum_info.is_pub {
        signature_parts.push("pub".bright_green().bold().to_string());
    }

    signature_parts.push("enum".bright_blue().bold().to_string());
    signature_parts.push(
        enum_info
            .name
            .bright_yellow()
            .bold()
            .underline()
            .to_string(),
    );

    if !enum_info.generics.is_empty() {
        let colored_generics: Vec<String> = enum_info
            .generics
            .iter()
            .map(|g| colorize_type(g))
            .collect();
        let generics = format!("<{}>", colored_generics.join(", "));
        signature_parts.push(generics.bright_magenta().to_string());
    }

    if !enum_info.variants.is_empty() {
        let variants: Vec<String> = enum_info
            .variants
            .iter()
            .map(|v| {
                let mut base = v.name.clone();
                if let Some(discriminant) = &v.discriminant {
                    base.push_str(&format!(" = {}", discriminant));
                }
                if !v.fields.is_empty() {
                    base.push('(');
                    base.push_str(&v.fields.join(", "));
                    base.push(')');
                }
                base
            })
            .collect();
        signature_parts.push(format!("{{ {} }}", variants.join(", ")));
    }

    format!(
        "{} {} {}",
        file_info,
        "→".bright_white().bold(),
        signature_parts.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::types::EnumVariantInfo;

    fn make_func() -> FunctionInfo {
        FunctionInfo {
            name: "compute".to_string(),
            signature: "fn compute(x: u32) -> u32".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 10,
            end_line: 12,
            is_async: true,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: vec!["T".to_string()],
            parameters: vec!["x: u32".to_string()],
            return_type: Some("u32".to_string()),
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    fn make_struct() -> StructInfo {
        StructInfo {
            name: "Foo".to_string(),
            signature: "pub struct Foo".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 4,
            is_pub: true,
            generics: vec!["T".to_string()],
            fields: vec!["x: u32".to_string()],
            kind: "struct".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    fn make_enum() -> EnumInfo {
        EnumInfo {
            name: "Kind".to_string(),
            signature: "enum Kind".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 5,
            is_pub: false,
            generics: vec![],
            variants: vec![
                EnumVariantInfo {
                    name: "Alpha".to_string(),
                    fields: vec!["u32".to_string()],
                    discriminant: None,
                },
                EnumVariantInfo {
                    name: "Beta".to_string(),
                    fields: vec![],
                    discriminant: Some("3".to_string()),
                },
            ],
            kind: "enum".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

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
    fn format_function_signature_no_color_uses_legacy_format() {
        let f = make_func();
        let plain = format_function_signature(&f, false);
        assert_eq!(plain, "src/lib.rs:10-12 - fn compute(x: u32) -> u32");
    }

    #[test]
    fn format_function_signature_with_color_includes_signature_pieces() {
        let f = make_func();
        let out = format_function_signature(&f, true);
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("compute"));
        assert!(stripped.contains("pub"));
        assert!(stripped.contains("async"));
        assert!(stripped.contains("fn"));
        assert!(stripped.contains("u32"));
    }

    #[test]
    fn format_struct_signature_no_color_includes_struct_keyword() {
        let s = make_struct();
        let plain = format_struct_signature(&s, false);
        assert_eq!(plain, "src/lib.rs:1-4 - struct Foo");
    }

    #[test]
    fn format_struct_signature_with_color_includes_fields() {
        let s = make_struct();
        let out = format_struct_signature(&s, true);
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("Foo"));
        assert!(stripped.contains("pub"));
        assert!(stripped.contains("struct"));
        assert!(stripped.contains("x: u32"));
    }

    #[test]
    fn format_enum_signature_no_color_includes_enum_keyword() {
        let e = make_enum();
        let plain = format_enum_signature(&e, false);
        assert_eq!(plain, "src/lib.rs:1-5 - enum Kind");
    }

    #[test]
    fn format_enum_signature_with_color_includes_variant_discriminant() {
        let e = make_enum();
        let out = format_enum_signature(&e, true);
        let stripped = strip_ansi(&out);
        assert!(stripped.contains("Beta = 3"));
        assert!(stripped.contains("Alpha(u32)"));
        assert!(stripped.contains("Kind"));
    }

    #[test]
    fn format_function_signature_no_color_handles_no_pub_no_generics() {
        let mut f = make_func();
        f.is_pub = false;
        f.is_async = false;
        f.generics = vec![];
        let plain = format_function_signature(&f, false);
        assert!(plain.contains("compute"));
    }
}

/// Format a function as a one-line display string.
///
/// Without color: `path:start-end - <signature>`.
/// With color: file location dimmed, then a colored modifier/keyword/name/
/// generics/params/return-type line using ANSI codes.
pub fn format_function_signature(func: &FunctionInfo, use_color: bool) -> String {
    if !use_color {
        return format!(
            "{}:{}-{} - {}",
            func.file_path, func.start_line, func.end_line, func.signature
        );
    }

    let file_info = format!("{}:{}-{}", func.file_path, func.start_line, func.end_line)
        .dimmed()
        .italic();

    let mut signature_parts = Vec::new();

    if func.is_pub {
        signature_parts.push("pub".bright_green().bold().to_string());
    }

    if func.is_const {
        signature_parts.push("const".bright_magenta().bold().to_string());
    }

    if func.is_unsafe {
        signature_parts.push("unsafe".bright_red().bold().to_string());
    }

    if func.is_async {
        signature_parts.push("async".bright_cyan().bold().to_string());
    }

    signature_parts.push("fn".bright_blue().bold().to_string());
    signature_parts.push(func.name.bright_yellow().bold().underline().to_string());

    if !func.generics.is_empty() {
        let colored_generics: Vec<String> =
            func.generics.iter().map(|g| colorize_type(g)).collect();
        let generics = format!("<{}>", colored_generics.join(", "));
        signature_parts.push(generics.bright_magenta().to_string());
    }

    let params = if func.parameters.is_empty() {
        "()".white().bold().to_string()
    } else {
        let colored_params: Vec<String> = func
            .parameters
            .iter()
            .map(|p| colorize_parameter(p))
            .collect();
        format!("({})", colored_params.join(", "))
    };
    signature_parts.push(params);

    if let Some(return_type) = &func.return_type {
        signature_parts.push("->".white().bold().to_string());
        signature_parts.push(colorize_type(return_type));
    }

    format!(
        "{} {} {}",
        file_info,
        "→".bright_white().bold(),
        signature_parts.join(" ")
    )
}
