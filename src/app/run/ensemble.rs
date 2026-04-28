use super::super::modes::EnsembleRequest;
use super::super::project::ProjectData;
use super::super::render::{print_ensemble_text, print_ensemble_text_summary_compact};
use super::super::symbol_id::inject_symbol_ids;
use crate::FunctionEnsembleOptions;
use crate::api::EnsemblePreset;
use crate::api::EnsembleView;
use crate::build_function_ensemble;
use crate::cli::Args;

use super::switchboard::write_string_output;


/// Resolved numeric limits that control how much data `build_function_ensemble` fetches.
///
/// Derived from an `EnsemblePreset` (`Quick` / `Balanced` / `Deep`) and not exposed directly in
/// the CLI — callers should use `resolve_config` to obtain an instance.
#[derive(Clone, Debug)]
pub struct EnsembleConfig {
    max_results: usize,
    max_call_sites: usize,
    call_depth: usize,
    max_related: usize,
    max_lifecycle_paths: usize,
    lifecycle_max_functions: usize,
}

/// Implements `rustgraph ensemble <fn>` — builds a full context bundle for a function including
/// its source, call-sites, struct dependencies, lifecycle paths, dataflow summary, and I/O
/// boundaries.
///
/// The output sections and depth limits are controlled by `--preset` (`quick` / `balanced` /
/// `deep`) and `--view` / `--section`. Ambiguous matches require `--all` or `--symbol-id` to
/// proceed. Returns an error when the query names a struct or enum rather than a function.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: EnsembleRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(kind) = exact_non_function_kind(&request.query, project) {


        let article = if kind == "enum" { "an" } else { "a" };
        return Err(format!(
            "'{}' is {} {}, not a function. Try `rustgraph refs {}` or `rustgraph usages {}` to find references.",
            request.query, article, kind, request.query, request.query
        )
        .into());
    }

    let lifecycle_types = request.lifecycle_types.as_ref().map(|raw| {
        raw.split('|')
            .map(|entry| entry.trim())
            .filter(|entry| !entry.is_empty())
            .map(|entry| entry.to_string())
            .collect::<Vec<_>>()
    });

    let config = resolve_config(request.preset);
    let sections = if !request.sections.is_empty() {

        request
            .sections
            .iter()
            .flat_map(|s| s.split(',').map(|t| t.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        resolve_sections(request.view)
    };
    let analysis = project.to_analysis();

    let resolved_query = if let Some(id) = &request.symbol_id {
        if id.starts_with("struct:") || id.starts_with("enum:") {
            return Err(format!(
                "ensemble operates on functions only; '{}' is a {}. Use `slice --symbol-id {}` instead.",
                id,
                if id.starts_with("struct:") { "struct" } else { "enum" },
                id
            )
            .into());
        }
        let s = id.strip_prefix("fn:").unwrap_or(id);
        if let Some(last) = s.rfind(':') {
            s[..last].to_string()
        } else {
            s.to_string()
        }
    } else {
        request.query.clone()
    };

    let mut ensemble = build_function_ensemble(
        &analysis,
        &resolved_query,
        args.search_threshold,
        FunctionEnsembleOptions {
            max_results: config.max_results,
            max_call_sites: config.max_call_sites,
            call_depth: config.call_depth,
            max_related: config.max_related,
            max_lifecycle_paths: config.max_lifecycle_paths,
            lifecycle_max_functions: config.lifecycle_max_functions,
            lifecycle_types: lifecycle_types.as_deref(),
            ensemble_sections: Some(&sections),
            match_signature: args.match_signature,
        },
    )?;

    if let Some(needle) = &request.target_in {
        let before = ensemble.matches.len();
        ensemble.matches.retain(|m| m.info.file_path.contains(needle));
        if ensemble.matches.is_empty() && before > 0 {
            eprintln!(
                "hint: --target-in '{}' filtered out all {} match(es); try a shorter substring",
                needle, before
            );
        }
    }

    if args.exclude_tests {


        let test_ids: std::collections::HashSet<String> = project
            .functions
            .iter()
            .filter(|f| f.is_test)
            .map(crate::function_id)
            .collect();
        let test_names: std::collections::HashSet<String> = project
            .functions
            .iter()
            .filter(|f| f.is_test)
            .map(|f| f.name.clone())
            .collect();
        for m in ensemble.matches.iter_mut() {
            let n = &mut m.neighborhood;
            n.upstream.retain(|r| !test_ids.contains(&r.id));
            n.downstream.retain(|r| !test_ids.contains(&r.id));
            n.upstream_total = n.upstream.len();
            n.downstream_total = n.downstream.len();

            m.call_sites
                .retain(|cs| !cs.caller_name.as_ref().is_some_and(|n| test_names.contains(n)));
            m.call_sites_total = m.call_sites.len();

            m.io_boundaries.retain(|b| !test_names.contains(&b.function_name));
        }
    }


    if request.no_boundaries {
        for m in ensemble.matches.iter_mut() {
            m.io_boundaries.clear();
        }
    } else if request.boundaries_min_score > 0.0 {
        let floor = request.boundaries_min_score;
        for m in ensemble.matches.iter_mut() {
            m.io_boundaries.retain(|b| b.confidence >= floor);
        }
    }

    if ensemble.matches.is_empty() {

        if let Some(hint) = super::callers::type_redirect_hint(project, &request.query, args.search_threshold) {
            return Err(hint.into());
        }
        return Err(format!(
            "no ensemble matches for '{}'; check spelling, try `find {}`, or lower --search-threshold",
            request.query, request.query
        )
        .into());
    } else if ensemble.matches.len() > request.ambiguity_cap && !request.all {

        let n = ensemble.matches.len();
        let mut msg = format!(
            "ambiguous: {} matches for '{}'. Disambiguate with --symbol-id or --target-in:",
            n, request.query
        );
        for m in &ensemble.matches {
            let sym_id = format!("{}:{}:{}", m.info.file_path, m.info.start_line, m.info.name);
            msg.push_str(&format!("\n  {}  --symbol-id '{}'", sym_id, sym_id));
            if n <= 5 {
                let preview = super::callers::read_body_preview_pub(&args.path, &m.info.file_path, m.info.start_line, 3);
                for line in &preview {
                    msg.push_str(&format!("\n    | {}", line));
                }
            }
        }
        if n > 5 {
            msg.push_str(&format!(
                "\n(pass `--all` to process all {} or use --target-in <PATH> / path.rs:LINE to narrow)",
                n
            ));
        }
        return Err(msg.into());
    } else if ensemble.matches.len() > 1 {
        eprintln!(
            "note: query '{}' matched {} functions — ensemble built for all of them:",
            request.query,
            ensemble.matches.len()
        );
        for m in &ensemble.matches {
            eprintln!("  - {}:{} {}", m.info.file_path, m.info.start_line, m.info.name);
        }
    }

    if args.json {
        let mut value = serde_json::to_value(&ensemble)?;
        inject_symbol_ids(&mut value);
        let payload = serde_json::to_string_pretty(&value)?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {


        let is_default_summary = request.sections.is_empty()
            && matches!(request.view.unwrap_or(EnsembleView::Summary), EnsembleView::Summary);
        if let Some(hint) = section_override_hint(&request.sections) {
            print!("{}", hint);
        } else if request.view.is_none() {
            if let Some(marker) = summary_view_footer(&sections) {
                print!("{}", marker);
            }
        }
        if is_default_summary {
            print_ensemble_text_summary_compact(&ensemble);
        } else {
            print_ensemble_text(&ensemble);
        }
    }

    Ok(())
}

fn exact_non_function_kind(query: &str, project: &ProjectData) -> Option<&'static str> {
    let trimmed = query.trim();

    if trimmed.contains(':') {
        return None;
    }
    let function_match = project.functions.iter().any(|f| f.name == trimmed);
    if function_match {
        return None;
    }
    if project.structs.iter().any(|s| s.name == trimmed) {
        return Some("struct");
    }
    if project.enums.iter().any(|e| e.name == trimmed) {
        return Some("enum");
    }
    None
}

fn resolve_config(preset: Option<EnsemblePreset>) -> EnsembleConfig {
    match preset.unwrap_or(EnsemblePreset::Balanced) {
        EnsemblePreset::Quick => EnsembleConfig {
            max_results: 3,
            max_call_sites: 80,
            call_depth: 1,
            max_related: 25,
            max_lifecycle_paths: 8,
            lifecycle_max_functions: 80,
        },
        EnsemblePreset::Balanced => EnsembleConfig {
            max_results: 5,
            max_call_sites: 200,
            call_depth: 2,
            max_related: 60,
            max_lifecycle_paths: 20,
            lifecycle_max_functions: 120,
        },
        EnsemblePreset::Deep => EnsembleConfig {
            max_results: 10,
            max_call_sites: 0,
            call_depth: 3,
            max_related: 120,
            max_lifecycle_paths: 40,
            lifecycle_max_functions: 280,
        },
    }
}

fn resolve_sections(view: Option<EnsembleView>) -> Vec<String> {


    match view.unwrap_or(EnsembleView::Summary) {


        EnsembleView::Summary => vec![
            "structs".to_string(),
            "call-sites".to_string(),
            "neighborhood".to_string(),
            "dataflow".to_string(),
        ],
        EnsembleView::Usage => vec!["call-sites".to_string(), "neighborhood".to_string()],
        EnsembleView::Flow => vec![
            "neighborhood".to_string(),
            "lifecycle".to_string(),
            "dataflow".to_string(),
            "boundaries".to_string(),
        ],
        EnsembleView::Full => vec!["all".to_string()],
    }
}


fn summary_view_footer(sections: &[String]) -> Option<String> {
    let normalized: Vec<String> = sections
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();


    if normalized.iter().any(|s| s == "all") || normalized.is_empty() {
        return None;
    }
    let has = |name: &str| normalized.iter().any(|s| s == name);
    let optional = ["code", "lifecycle", "neighborhood", "boundaries", "dataflow"];
    let all_present = optional.iter().all(|name| has(name));
    if all_present {
        return None;
    }


    let mut block = String::from("[summary view — sections hidden:\n");
    if !has("code") {
        block.push_str("  --view full      adds: code body + lifecycle + boundaries\n");
    }
    if !(has("neighborhood") && has("lifecycle") && has("dataflow") && has("boundaries")) {
        block.push_str("  --view flow      adds: lifecycle + boundaries (drops structs + call-sites)\n");
    }
    block.push_str("  --section <name> cherry-pick: code | structs | call-sites | neighborhood | lifecycle | dataflow | boundaries]\n");
    Some(block)
}


fn section_override_hint(raw_sections: &[String]) -> Option<String> {
    if raw_sections.is_empty() {
        return None;
    }
    let normalized: Vec<String> = raw_sections
        .iter()
        .flat_map(|s| s.split(',').map(|t| t.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect();
    if normalized.is_empty() {
        return None;
    }
    Some(format!("[showing: {}]\n", normalized.join(", ")))
}
