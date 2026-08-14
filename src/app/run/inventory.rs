use serde::Serialize;

use super::super::modes::AnalyzeSelection;
use super::super::project::ProjectData;
use super::super::render::print_inventory_text;
use super::super::symbol_id::WithSymbolId;
use crate::cli::Args;
use crate::{ConstInfo, EnumInfo, FunctionInfo, StructInfo, TypeDeclInfo};

use super::switchboard::write_string_output;

/// Implements `rustgraph inventory` — dumps all parsed functions, structs, and enums.
///
/// Applies `--public-only` filtering and the `AnalyzeSelection` kind filter (`--func`,
/// `--struct`, `--enum`). In JSON mode each item is wrapped with a stable `symbol_id`; in text
/// mode delegates to `print_inventory_text` with optional color output.
pub fn run(
    args: &Args,
    mut project: ProjectData,
    selection: AnalyzeSelection,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.public_only {
        project.functions.retain(|f| f.is_pub);
        project.structs.retain(|s| s.is_pub);
        project.enums.retain(|e| e.is_pub);
        project.consts.retain(|c| c.is_pub);
        project.type_decls.retain(|t| t.is_pub);
    }

    if args.json {
        #[derive(Serialize)]
        struct InventoryOutput {
            functions: Vec<WithSymbolId<FunctionInfo>>,
            structs: Vec<WithSymbolId<StructInfo>>,
            enums: Vec<WithSymbolId<EnumInfo>>,
            consts: Vec<WithSymbolId<ConstInfo>>,
            type_decls: Vec<WithSymbolId<TypeDeclInfo>>,
        }

        let payload = serde_json::to_string_pretty(&InventoryOutput {
            functions: if selection.show_functions() {
                project
                    .functions
                    .into_iter()
                    .map(WithSymbolId::wrap_fn)
                    .collect()
            } else {
                Vec::new()
            },
            structs: if selection.show_structs() {
                project
                    .structs
                    .into_iter()
                    .map(WithSymbolId::wrap_struct)
                    .collect()
            } else {
                Vec::new()
            },
            enums: if selection.show_enums() {
                project
                    .enums
                    .into_iter()
                    .map(WithSymbolId::wrap_enum)
                    .collect()
            } else {
                Vec::new()
            },
            consts: if selection.show_consts() {
                project
                    .consts
                    .into_iter()
                    .map(WithSymbolId::wrap_const)
                    .collect()
            } else {
                Vec::new()
            },
            type_decls: project
                .type_decls
                .into_iter()
                .filter(|t| {
                    if t.kind == "trait" {
                        selection.show_traits()
                    } else {
                        selection.show_aliases()
                    }
                })
                .map(WithSymbolId::wrap_type_decl)
                .collect(),
        })?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {
        print_inventory_text(
            &project.functions,
            &project.structs,
            &project.enums,
            &project.consts,
            &project.type_decls,
            selection,
            !args.no_color && args.color,
        );
    }

    Ok(())
}
