use super::super::modes::SliceRequest;
use super::super::project::ProjectData;
use crate::api::{SearchResults, SourceSlice, SymbolRef};
use crate::cli::Args;
use crate::query;

use super::switchboard::write_string_output;


enum LineBoundsCheck {
    Ok,
    Clamped { requested: (usize, usize), file_len: usize, clamped: (usize, usize) },
    OutOfRange { requested: (usize, usize), file_len: usize },
}


fn check_line_bounds(path: &std::path::Path, start: usize, end: usize) -> LineBoundsCheck {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,


        Err(_) => return LineBoundsCheck::Ok,
    };
    let file_len = content.lines().count();
    if file_len == 0 {


        return LineBoundsCheck::Ok;
    }
    if start > file_len {
        return LineBoundsCheck::OutOfRange {
            requested: (start, end),
            file_len,
        };
    }
    if end > file_len {
        return LineBoundsCheck::Clamped {
            requested: (start, end),
            file_len,
            clamped: (start, file_len),
        };
    }
    LineBoundsCheck::Ok
}


fn report_line_bounds(
    path: &std::path::Path,
    check: LineBoundsCheck,
) -> Result<(), Box<dyn std::error::Error>> {
    match check {
        LineBoundsCheck::Ok => Ok(()),
        LineBoundsCheck::Clamped { requested, file_len, clamped } => {
            eprintln!(
                "note: clamped {}-{} to {}-{} (file {} has {} lines)",
                requested.0,
                requested.1,
                clamped.0,
                clamped.1,
                path.display(),
                file_len
            );
            Ok(())
        }
        LineBoundsCheck::OutOfRange { requested, file_len } => Err(format!(
            "requested lines {}-{} are past EOF (file {} has {} lines)",
            requested.0,
            requested.1,
            path.display(),
            file_len
        )
        .into()),
    }
}

/// Implements `rustgraph slice <query>` — extracts the exact source text of one symbol or an
/// explicit line range from a file.
///
/// Resolution priority: `--symbol-id` → `--file --start-line --end-line` → `path:line-range`
/// query syntax → `path:line` (enclosing symbol lookup) → fuzzy name search. The `--around N`
/// flag expands any resolved range by N lines in each direction. Returns an error when the query
/// is ambiguous or when the requested lines are past EOF.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: SliceRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut output = resolve_slice(args, project, request)?;


    if !args.absolute_paths {
        let rel = super::super::relativize_for_display(
            &output.slice.slice.file_path,
            &args.path,
            &args.also,
        );
        output.slice.slice.file_path = rel;
    }
    if args.json {
        let payload = serde_json::to_string_pretty(&output)?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {
        write_string_output(args.output.as_deref(), &render_slice_text(&output))?;
    }
    Ok(())
}

/// Combined output of a slice query: the resolved symbol reference (if any) and the extracted
/// source range.
#[derive(serde::Serialize)]
struct SliceOutput {
    symbol: Option<SymbolRef>,
    slice: SourceSlice,
}

fn resolve_slice(
    args: &Args,
    project: &ProjectData,
    request: SliceRequest,
) -> Result<SliceOutput, Box<dyn std::error::Error>> {
    if let Some(symbol_id) = request.symbol_id.as_deref() {
        let symbol = project_symbol_by_id(project, symbol_id);
        let slice = query::source_slice_for_symbol(project, symbol_id)?;
        return Ok(SliceOutput { symbol, slice });
    }

    if let (Some(file), Some(start_line), Some(end_line)) = (
        request.file.as_deref(),
        request.start_line,
        request.end_line,
    ) {


        let path = if file.is_absolute() {
            file.to_path_buf()
        } else {
            let direct = args.path.join(file);
            if direct.exists() {
                direct
            } else {
                crate::index::resolve_read_path(&file.to_string_lossy())
            }
        };

        let (eff_start, eff_end) = expand_with_around(start_line, end_line, request.around);


        report_line_bounds(&path, check_line_bounds(&path, eff_start, eff_end))?;
        let slice = query::source_slice_for_lines(&path, eff_start, eff_end, None)?;
        return Ok(SliceOutput {
            symbol: None,
            slice,
        });
    }

    let query_text = request
        .query
        .as_deref()
        .ok_or("slice requires a query, --symbol-id, or --file with --start-line/--end-line")?;


    if let Some((path_part, range_str)) = query_text.rsplit_once(':') {


        let (effective_path, effective_range) = if let Some((maybe_path, maybe_line)) =
            path_part.rsplit_once(':')
            && maybe_line.parse::<usize>().is_ok()
            && range_str.parse::<usize>().is_ok()
        {


            (maybe_path, maybe_line)
        } else {
            (path_part, range_str)
        };

        if let Some((start_str, end_str)) = effective_range.split_once('-')
            && let (Ok(start), Ok(end)) = (start_str.parse::<usize>(), end_str.parse::<usize>())
        {
            let resolved = resolve_slice_path(args, project, effective_path);

            let (eff_start, eff_end) = expand_with_around(start, end, request.around);

            report_line_bounds(&resolved, check_line_bounds(&resolved, eff_start, eff_end))?;
            let slice = query::source_slice_for_lines(&resolved, eff_start, eff_end, None)?;
            return Ok(SliceOutput { symbol: None, slice });
        }
        if let Ok(line) = effective_range.parse::<usize>() {


            if let Some(window) = request.around {
                let resolved = resolve_slice_path(args, project, effective_path);
                let start = line.saturating_sub(window).max(1);
                let end = line.saturating_add(window);
                report_line_bounds(&resolved, check_line_bounds(&resolved, start, end))?;
                let slice = query::source_slice_for_lines(&resolved, start, end, None)?;
                return Ok(SliceOutput { symbol: None, slice });
            }

            let mut hits: Vec<&crate::FunctionInfo> = project
                .functions
                .iter()
                .filter(|f| {
                    f.file_path.ends_with(effective_path)
                        && f.start_line <= line
                        && line <= f.end_line
                })
                .collect();
            hits.sort_by_key(|f| std::cmp::Reverse(f.start_line));
            if let Some(f) = hits.first() {
                let id = crate::function_symbol_id(f);
                let symbol = project_symbol_by_id(project, &id);
                let slice = query::source_slice_for_symbol(project, &id)?;
                return Ok(SliceOutput { symbol, slice });
            }


            let mut struct_hits: Vec<&crate::StructInfo> = project
                .structs
                .iter()
                .filter(|s| {
                    s.file_path.ends_with(effective_path)
                        && s.start_line <= line
                        && line <= s.end_line
                })
                .collect();
            struct_hits.sort_by_key(|s| std::cmp::Reverse(s.start_line));
            if let Some(s) = struct_hits.first() {
                let id = crate::struct_symbol_id(s);
                let symbol = project_symbol_by_id(project, &id);
                let slice = query::source_slice_for_symbol(project, &id)?;
                return Ok(SliceOutput { symbol, slice });
            }
            let mut enum_hits: Vec<&crate::EnumInfo> = project
                .enums
                .iter()
                .filter(|e| {
                    e.file_path.ends_with(effective_path)
                        && e.start_line <= line
                        && line <= e.end_line
                })
                .collect();
            enum_hits.sort_by_key(|e| std::cmp::Reverse(e.start_line));
            if let Some(e) = enum_hits.first() {
                let id = crate::enum_symbol_id(e);
                let symbol = project_symbol_by_id(project, &id);
                let slice = query::source_slice_for_symbol(project, &id)?;
                return Ok(SliceOutput { symbol, slice });
            }
        }
    }

    let results = query::search_symbols_with_options(
        project,
        query_text,
        args.search_threshold,
        10,
        args.match_signature,
    );
    let symbol = resolve_symbol(query_text, &results)?;
    let slice = query::source_slice_for_symbol(project, &symbol.symbol_id)?;
    Ok(SliceOutput {
        symbol: Some(symbol),
        slice,
    })
}


fn expand_with_around(start: usize, end: usize, around: Option<usize>) -> (usize, usize) {
    match around {
        Some(window) => (
            start.saturating_sub(window).max(1),
            end.saturating_add(window),
        ),
        None => (start, end),
    }
}

fn resolve_symbol(
    query_text: &str,
    results: &SearchResults,
) -> Result<SymbolRef, Box<dyn std::error::Error>> {
    if results.symbols.is_empty() {
        return Err(format!("no symbol matched query: {query_text}").into());
    }

    let exact_name_matches = results
        .symbols
        .iter()
        .filter(|symbol| symbol.name == query_text)
        .cloned()
        .collect::<Vec<_>>();
    if exact_name_matches.len() == 1 {
        return Ok(exact_name_matches[0].clone());
    }
    if results.symbols.len() == 1 {
        return Ok(results.symbols[0].clone());
    }

    Err(format_ambiguity(query_text, &results.symbols).into())
}


fn resolve_slice_path(args: &Args, project: &ProjectData, path_part: &str) -> std::path::PathBuf {
    let raw = std::path::Path::new(path_part);
    if raw.is_absolute() {
        return raw.to_path_buf();
    }
    let direct = args.path.join(raw);
    if direct.exists() {
        return direct;
    }


    if let Some(hit) = project
        .functions
        .iter()
        .map(|f| f.file_path.as_str())
        .chain(project.structs.iter().map(|s| s.file_path.as_str()))
        .chain(project.enums.iter().map(|e| e.file_path.as_str()))
        .find(|p| p.ends_with(path_part))
    {
        return crate::index::resolve_read_path(hit);
    }
    direct
}

fn project_symbol_by_id(project: &ProjectData, symbol_id: &str) -> Option<SymbolRef> {
    query::search_symbols(project, "", 0.0, 0)
        .symbols
        .into_iter()
        .find(|symbol| symbol.symbol_id == symbol_id)
}

fn format_ambiguity(query_text: &str, symbols: &[SymbolRef]) -> String {
    let mut detail =
        format!("query matched multiple symbols: {query_text}\nuse --symbol-id with one of:\n");
    for symbol in symbols {
        detail.push_str(&format!(
            "- {} [{}] {}:{}-{}\n",
            symbol.symbol_id, symbol.kind, symbol.file_path, symbol.start_line, symbol.end_line
        ));
    }
    detail
}

fn render_slice_text(output: &SliceOutput) -> String {
    let mut rendered = String::new();
    if let Some(symbol) = &output.symbol {
        rendered.push_str(&format!(
            "{} [{}]\n{}\n\n",
            symbol.name, symbol.kind, symbol.symbol_id
        ));
    }
    rendered.push_str(&format!(
        "{}:{}-{}\n\n{}",
        output.slice.slice.file_path,
        output.slice.slice.start_line,
        output.slice.slice.end_line,
        output.slice.slice.content
    ));
    rendered
}
