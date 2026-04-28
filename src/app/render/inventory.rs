//! Plain-text renderer for the inventory (symbol listing) subcommand.

use crate::app::modes::AnalyzeSelection;
use crate::{
    EnumInfo, FunctionInfo, StructInfo, format_enum_signature, format_function_signature,
    format_struct_signature,
};
use colored::*;

/// Prints a formatted symbol inventory to stdout, respecting `selection` to filter which
/// kinds are shown, then appends a summary count line.
///
/// When `use_color` is `true`, the count line uses ANSI color codes; otherwise it is plain text.
pub fn print_inventory_text(
    functions: &[FunctionInfo],
    structs: &[StructInfo],
    enums: &[EnumInfo],
    selection: AnalyzeSelection,
    use_color: bool,
) {
    if selection.show_functions() {
        for function in functions {
            println!("{}", format_function_signature(function, use_color));
        }
    }

    if selection.show_structs() {
        for struct_info in structs {
            println!("{}", format_struct_signature(struct_info, use_color));
        }
    }

    if selection.show_enums() {
        for enum_info in enums {
            println!("{}", format_enum_signature(enum_info, use_color));
        }
    }

    let mut summary_parts = Vec::new();

    if selection.show_functions() {
        let summary = if use_color {
            format!(
                "{} {}",
                "Functions:".bold(),
                functions.len().to_string().bright_green()
            )
        } else {
            format!("Functions: {}", functions.len())
        };
        summary_parts.push(summary);
    }

    if selection.show_structs() {
        let summary = if use_color {
            format!(
                "{} {}",
                "Structs:".bold(),
                structs.len().to_string().bright_cyan()
            )
        } else {
            format!("Structs: {}", structs.len())
        };
        summary_parts.push(summary);
    }

    if selection.show_enums() {
        let summary = if use_color {
            format!(
                "{} {}",
                "Enums:".bold(),
                enums.len().to_string().bright_magenta()
            )
        } else {
            format!("Enums: {}", enums.len())
        };
        summary_parts.push(summary);
    }

    if !summary_parts.is_empty() {
        println!("\n{}", summary_parts.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function() -> FunctionInfo {
        FunctionInfo {
            name: "compute".to_string(),
            signature: "fn compute()".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 2,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: Vec::new(),
            parameters: Vec::new(),
            return_type: None,
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    fn struct_info() -> StructInfo {
        StructInfo {
            name: "Foo".to_string(),
            signature: "struct Foo".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 2,
            is_pub: true,
            generics: Vec::new(),
            fields: Vec::new(),
            kind: "struct".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    fn enum_info() -> EnumInfo {
        EnumInfo {
            name: "Bar".to_string(),
            signature: "enum Bar".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 2,
            is_pub: true,
            generics: Vec::new(),
            variants: Vec::new(),
            kind: "enum".to_string(),
            cfg_attrs: Vec::new(),
        }
    }


    #[test]
    fn print_inventory_text_does_not_panic_for_empty_inputs() {
        print_inventory_text(&[], &[], &[], AnalyzeSelection::All, false);
    }

    #[test]
    fn print_inventory_text_does_not_panic_for_each_selection() {
        let funcs = vec![function()];
        let structs = vec![struct_info()];
        let enums = vec![enum_info()];
        for selection in [
            AnalyzeSelection::Functions,
            AnalyzeSelection::Structs,
            AnalyzeSelection::Enums,
            AnalyzeSelection::FunctionsStructs,
            AnalyzeSelection::FunctionsEnums,
            AnalyzeSelection::StructsEnums,
            AnalyzeSelection::All,
        ] {
            print_inventory_text(&funcs, &structs, &enums, selection, false);
        }
    }
}
