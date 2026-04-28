use serde::Serialize;

use super::super::modes::AnalyzeSelection;
use super::super::project::ProjectData;
use super::super::render::print_inventory_text;
use super::super::symbol_id::WithSymbolId;
use crate::cli::Args;
use crate::{EnumInfo, FunctionInfo, StructInfo};

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
    }

    if args.json {
        #[derive(Serialize)]
        struct InventoryOutput {
            functions: Vec<WithSymbolId<FunctionInfo>>,
            structs: Vec<WithSymbolId<StructInfo>>,
            enums: Vec<WithSymbolId<EnumInfo>>,
        }

        let payload = serde_json::to_string_pretty(&InventoryOutput {
            functions: if selection.show_functions() {
                project.functions.into_iter().map(WithSymbolId::wrap_fn).collect()
            } else {
                Vec::new()
            },
            structs: if selection.show_structs() {
                project.structs.into_iter().map(WithSymbolId::wrap_struct).collect()
            } else {
                Vec::new()
            },
            enums: if selection.show_enums() {
                project.enums.into_iter().map(WithSymbolId::wrap_enum).collect()
            } else {
                Vec::new()
            },
        })?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {
        print_inventory_text(
            &project.functions,
            &project.structs,
            &project.enums,
            selection,
            !args.no_color && args.color,
        );
    }

    Ok(())
}
