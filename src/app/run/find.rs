use serde::Serialize;

use super::super::changed::{ChangedRanges, fn_was_changed};
use super::super::modes::{AnalyzeSelection, FindRequest};
use super::super::project::ProjectData;
use super::super::symbol_id::WithSymbolId;
use crate::cli::Args;
use crate::{
    EnumInfo, FunctionInfo, MatchKind, StructInfo, format_cfg_annotation,
    search_items_exact_with_kinds, search_items_with_kinds_and_fallback,
};

use super::switchboard::write_string_output;


fn collect_suggestions(
    query: &str,
    project: &ProjectData,
    selection: AnalyzeSelection,
    limit: usize,
    requested_threshold: f64,
) -> Vec<(String, &'static str, f64)> {
    let mut pool: Vec<(String, &'static str)> = Vec::new();
    if selection.show_functions() {
        for f in &project.functions {
            pool.push((f.name.clone(), "fn"));
        }
    }
    if selection.show_structs() {
        for s in &project.structs {
            pool.push((s.name.clone(), "struct"));
        }
    }
    if selection.show_enums() {
        for e in &project.enums {
            pool.push((e.name.clone(), "enum"));
        }
    }
    let eff = crate::effective_threshold(query, requested_threshold);
    let mut scored: Vec<(String, &'static str, f64)> = pool
        .into_iter()
        .map(|(n, k)| {
            let s = crate::fuzzy_similarity(query, &n);
            (n, k, s)
        })
        .filter(|(_, _, s)| *s >= 0.5 && *s < eff)
        .collect();
    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    scored.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    scored.truncate(limit);
    scored
}

/// Implements `rustgraph find <query>` — fuzzy (or exact) symbol search across functions, structs, and enums.
///
/// Applies threshold filtering, optional path/changed constraints, `--public-only`, and
/// `--exclude-tests`. On zero hits at the default threshold (0.85), relaxes to 0.70 and emits
/// suggestions. Returns an error with "did you mean" hints when no match is found.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: FindRequest,
    changed: Option<&ChangedRanges>,
) -> Result<(), Box<dyn std::error::Error>> {


    let (functions, structs, enums, short_query_fallback) = if request.exact {
        let (fns, structs_out, enums_out) = search_items_exact_with_kinds(
            &project.functions,
            &project.structs,
            &project.enums,
            &request.query,
        );
        (fns, structs_out, enums_out, false)
    } else {
        search_items_with_kinds_and_fallback(
            &project.functions,
            &project.structs,
            &project.enums,
            &request.query,
            request.threshold,
            args.match_signature,
        )
    };

    let functions: Vec<(FunctionInfo, MatchKind)> = if request.selection.show_functions() {
        let mut funcs = functions;
        if args.exclude_tests {
            funcs.retain(|(f, _)| !f.is_test);
        }


        if args.public_only {
            funcs.retain(|(f, _)| f.is_pub);
        }
        funcs
    } else {
        Vec::new()
    };
    let structs = if request.selection.show_structs() {
        if args.public_only {
            structs.into_iter().filter(|(s, _)| s.is_pub).collect()
        } else {
            structs
        }
    } else {
        Vec::new()
    };
    let enums = if request.selection.show_enums() {
        if args.public_only {
            enums.into_iter().filter(|(e, _)| e.is_pub).collect()
        } else {
            enums
        }
    } else {
        Vec::new()
    };


    let (functions, structs, enums) = if let Some(needle) = &request.in_path {
        (
            functions.into_iter().filter(|(f, _)| f.file_path.contains(needle.as_str())).collect::<Vec<_>>(),
            structs.into_iter().filter(|(s, _)| s.file_path.contains(needle.as_str())).collect::<Vec<_>>(),
            enums.into_iter().filter(|(e, _)| e.file_path.contains(needle.as_str())).collect::<Vec<_>>(),
        )
    } else {
        (functions, structs, enums)
    };


    let (functions, structs, enums) = if let Some(ranges) = changed {
        (
            functions
                .into_iter()
                .filter(|(f, _)| fn_was_changed(&f.file_path, f.start_line, f.end_line, ranges))
                .collect::<Vec<_>>(),
            structs
                .into_iter()
                .filter(|(s, _)| fn_was_changed(&s.file_path, s.start_line, s.end_line, ranges))
                .collect::<Vec<_>>(),
            enums
                .into_iter()
                .filter(|(e, _)| fn_was_changed(&e.file_path, e.start_line, e.end_line, ranges))
                .collect::<Vec<_>>(),
        )
    } else {
        (functions, structs, enums)
    };

    let total_hits = functions.len() + structs.len() + enums.len();


    if short_query_fallback && total_hits > 0 {
        eprintln!(
            "note: no exact match for '{}' (≤3-char strict tier); falling back to substring/prefix at threshold {:.2}",
            request.query, request.threshold
        );
    }


    if args.match_signature && total_hits > 0 {
        let name_hits = functions
            .iter()
            .filter(|(_, k)| matches!(k, MatchKind::Name))
            .count()
            + structs
                .iter()
                .filter(|(_, k)| matches!(k, MatchKind::Name))
                .count()
            + enums
                .iter()
                .filter(|(_, k)| matches!(k, MatchKind::Name))
                .count();
        let sig_path_hits = total_hits - name_hits;
        if name_hits == 0 {

            eprintln!(
                "note: 0 name matches; showing {} signature/path matches (--match-signature)",
                sig_path_hits
            );
        } else if sig_path_hits > 0 {


            eprintln!(
                "note: {} name + {} sig/path matches (--match-signature)",
                name_hits, sig_path_hits
            );
        }

    }


    if total_hits == 0
        && !request.exact
        && changed.is_none()
        && request.threshold == 0.85
        && request.query.chars().count() >= 4
    {
        let (relaxed_fns_raw, relaxed_structs_raw, relaxed_enums_raw, _) = search_items_with_kinds_and_fallback(
            &project.functions,
            &project.structs,
            &project.enums,
            &request.query,
            0.7,
            args.match_signature,
        );

        let relaxed_fns: Vec<_> = if request.selection.show_functions() {
            let mut v = relaxed_fns_raw;
            if args.exclude_tests { v.retain(|(f, _)| !f.is_test); }
            if args.public_only { v.retain(|(f, _)| f.is_pub); }
            v
        } else { Vec::new() };
        let relaxed_structs: Vec<_> = if request.selection.show_structs() {
            let mut v = relaxed_structs_raw;
            if args.public_only { v.retain(|(s, _)| s.is_pub); }
            v
        } else { Vec::new() };
        let relaxed_enums: Vec<_> = if request.selection.show_enums() {
            let mut v = relaxed_enums_raw;
            if args.public_only { v.retain(|(e, _)| e.is_pub); }
            v
        } else { Vec::new() };

        let relaxed_total = relaxed_fns.len() + relaxed_structs.len() + relaxed_enums.len();
        if relaxed_total > 0 {

            let cap = 5usize;
            let (disp_fns, disp_structs, disp_enums) =
                truncate_in_order(relaxed_fns, relaxed_structs, relaxed_enums, cap);
            let shown = disp_fns.len() + disp_structs.len() + disp_enums.len();
            let note = format!(
                "note: 0 hits at threshold 0.85; relaxed to 0.7 and found {} candidate(s) (top {} shown). Pass --threshold X to control.",
                relaxed_total, shown.min(relaxed_total)
            );

            eprintln!("{}", note);
            if !args.json {
                let mut relaxed_out = String::new();
                relaxed_out.push_str(&note);
                relaxed_out.push('\n');
                render_fn_lines(&disp_fns, request.show_ids, &mut relaxed_out);
                render_struct_lines(&disp_structs, request.show_ids, &mut relaxed_out);
                render_enum_lines(&disp_enums, request.show_ids, &mut relaxed_out);
                write_string_output(args.output.as_deref(), relaxed_out.trim_end_matches('\n'))?;
            }
            return Ok(());
        }
    }

    if total_hits == 0 {


        if changed.is_some() {
            if args.json {
                #[derive(Serialize)]
                struct EmptyOutput {
                    query: String,
                    threshold: f64,
                    functions: Vec<TaggedHit<WithSymbolId<FunctionInfo>>>,
                    structs: Vec<TaggedHit<WithSymbolId<StructInfo>>>,
                    enums: Vec<TaggedHit<WithSymbolId<EnumInfo>>>,
                }
                let payload = serde_json::to_string_pretty(&EmptyOutput {
                    query: request.query.clone(),
                    threshold: request.threshold,
                    functions: Vec::new(),
                    structs: Vec::new(),
                    enums: Vec::new(),
                })?;
                write_string_output(args.output.as_deref(), &payload)?;
            } else {
                eprintln!(
                    "rustgraph find '{}' --changed: 0 changed symbol(s) matched",
                    request.query
                );
                write_string_output(args.output.as_deref(), "")?;
            }
            return Ok(());
        }


        let mut msg = if request.exact {
            format!(
                "no match for query '{}' (exact mode); check spelling or drop --exact for fuzzy matching",
                request.query
            )
        } else {
            format!(
                "no match for query '{}' (threshold {:.2}); try lowering --search-threshold or check spelling",
                request.query, request.threshold
            )
        };
        let suggestions = collect_suggestions(
            &request.query,
            project,
            request.selection,
            5,
            request.threshold,
        );
        if !suggestions.is_empty() {
            msg.push_str("\ndid you mean:");
            for (name, kind, score) in &suggestions {
                msg.push_str(&format!(
                    "\n  - {} [{}]  (similarity {:.2})",
                    name, kind, score
                ));
            }
        }
        return Err(msg.into());
    }


    let max_results = request.max_results;
    let truncated = max_results != 0 && total_hits > max_results;
    let (display_functions, display_structs, display_enums) = if truncated {
        truncate_in_order(
            functions.clone(),
            structs.clone(),
            enums.clone(),
            max_results,
        )
    } else {
        (functions.clone(), structs.clone(), enums.clone())
    };


    let query_terms = exact_terms(&request.query);
    let (fn_exact, fn_fuzzy) =
        partition_by_exact_name(display_functions, &query_terms, |f| f.name.as_str());
    let (struct_exact, struct_fuzzy) =
        partition_by_exact_name(display_structs, &query_terms, |s| s.name.as_str());
    let (enum_exact, enum_fuzzy) =
        partition_by_exact_name(display_enums, &query_terms, |e| e.name.as_str());
    let any_exact = !fn_exact.is_empty() || !struct_exact.is_empty() || !enum_exact.is_empty();
    let any_fuzzy = !fn_fuzzy.is_empty() || !struct_fuzzy.is_empty() || !enum_fuzzy.is_empty();

    if args.json {


        #[derive(Serialize)]
        struct FindOutput {
            query: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            threshold: Option<f64>,
            #[serde(skip_serializing_if = "std::ops::Not::not")]
            exact: bool,
            functions: Vec<TaggedHit<WithSymbolId<FunctionInfo>>>,
            structs: Vec<TaggedHit<WithSymbolId<StructInfo>>>,
            enums: Vec<TaggedHit<WithSymbolId<EnumInfo>>>,
            functions_exact: Vec<TaggedHit<WithSymbolId<FunctionInfo>>>,
            functions_fuzzy: Vec<TaggedHit<WithSymbolId<FunctionInfo>>>,
            structs_exact: Vec<TaggedHit<WithSymbolId<StructInfo>>>,
            structs_fuzzy: Vec<TaggedHit<WithSymbolId<StructInfo>>>,
            enums_exact: Vec<TaggedHit<WithSymbolId<EnumInfo>>>,
            enums_fuzzy: Vec<TaggedHit<WithSymbolId<EnumInfo>>>,
        }


        let fns_combined: Vec<(FunctionInfo, MatchKind)> =
            fn_exact.iter().chain(fn_fuzzy.iter()).cloned().collect();
        let structs_combined: Vec<(StructInfo, MatchKind)> = struct_exact
            .iter()
            .chain(struct_fuzzy.iter())
            .cloned()
            .collect();
        let enums_combined: Vec<(EnumInfo, MatchKind)> = enum_exact
            .iter()
            .chain(enum_fuzzy.iter())
            .cloned()
            .collect();
        let payload = serde_json::to_string_pretty(&FindOutput {
            query: request.query.clone(),
            threshold: if request.exact {
                None
            } else {
                Some(request.threshold)
            },
            exact: request.exact,
            functions: fns_combined
                .into_iter()
                .map(|(f, k)| TaggedHit::wrap(WithSymbolId::wrap_fn(f), k))
                .collect(),
            structs: structs_combined
                .into_iter()
                .map(|(s, k)| TaggedHit::wrap(WithSymbolId::wrap_struct(s), k))
                .collect(),
            enums: enums_combined
                .into_iter()
                .map(|(e, k)| TaggedHit::wrap(WithSymbolId::wrap_enum(e), k))
                .collect(),
            functions_exact: fn_exact
                .clone()
                .into_iter()
                .map(|(f, k)| TaggedHit::wrap(WithSymbolId::wrap_fn(f), k))
                .collect(),
            functions_fuzzy: fn_fuzzy
                .clone()
                .into_iter()
                .map(|(f, k)| TaggedHit::wrap(WithSymbolId::wrap_fn(f), k))
                .collect(),
            structs_exact: struct_exact
                .clone()
                .into_iter()
                .map(|(s, k)| TaggedHit::wrap(WithSymbolId::wrap_struct(s), k))
                .collect(),
            structs_fuzzy: struct_fuzzy
                .clone()
                .into_iter()
                .map(|(s, k)| TaggedHit::wrap(WithSymbolId::wrap_struct(s), k))
                .collect(),
            enums_exact: enum_exact
                .clone()
                .into_iter()
                .map(|(e, k)| TaggedHit::wrap(WithSymbolId::wrap_enum(e), k))
                .collect(),
            enums_fuzzy: enum_fuzzy
                .clone()
                .into_iter()
                .map(|(e, k)| TaggedHit::wrap(WithSymbolId::wrap_enum(e), k))
                .collect(),
        })?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {


        let header_mode = if request.exact {
            "(exact mode)".to_string()
        } else {
            format!("(threshold {:.2})", request.threshold)
        };
        eprintln!(
            "rustgraph find '{}' {}: {} fn, {} struct, {} enum",
            request.query,
            header_mode,
            functions.len(),
            structs.len(),
            enums.len()
        );
        let mut out = String::new();


        let render_both = any_exact && any_fuzzy;
        if render_both {
            out.push_str("== Exact name matches ==\n");
        }
        render_fn_lines(&fn_exact, request.show_ids, &mut out);
        render_struct_lines(&struct_exact, request.show_ids, &mut out);
        render_enum_lines(&enum_exact, request.show_ids, &mut out);
        if render_both {
            out.push_str("\n== Fuzzy matches ==\n");
        }
        render_fn_lines(&fn_fuzzy, request.show_ids, &mut out);
        render_struct_lines(&struct_fuzzy, request.show_ids, &mut out);
        render_enum_lines(&enum_fuzzy, request.show_ids, &mut out);
        if truncated {
            out.push_str(&format!(
                "(showing {} of {}; use --max-results to expand)\n",
                max_results, total_hits
            ));
        }
        write_string_output(args.output.as_deref(), out.trim_end_matches('\n'))?;
    }


    if !request.exact && total_hits > 10 && request.threshold < 0.95 {
        let suggested = if total_hits > 25 { 0.99 } else { 0.95 };
        eprintln!(
            "note: {} matches at threshold {:.2}; --search-threshold {:.2} for stricter",
            total_hits, request.threshold, suggested
        );
    }

    Ok(())
}


fn exact_terms(query: &str) -> Vec<String> {
    query
        .split('|')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}


fn partition_by_exact_name<T: Clone>(
    items: Vec<(T, MatchKind)>,
    terms: &[String],
    name_of: impl Fn(&T) -> &str,
) -> (Vec<(T, MatchKind)>, Vec<(T, MatchKind)>) {
    let mut exact = Vec::new();
    let mut fuzzy = Vec::new();
    for (item, kind) in items {
        let name = name_of(&item).to_lowercase();
        if terms.iter().any(|t| t == &name) {
            exact.push((item, kind));
        } else {
            fuzzy.push((item, kind));
        }
    }
    (exact, fuzzy)
}

fn render_fn_lines(hits: &[(FunctionInfo, MatchKind)], show_ids: bool, out: &mut String) {
    for (f, kind) in hits {


        let id_suffix = if show_ids {
            format!(" [id: {}]", crate::function_id(f))
        } else {
            String::new()
        };


        let cfg_suffix = format_cfg_annotation(&f.cfg_attrs)
            .map(|s| format!(" {}", s))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}:{}-{} {} - {}{}{}\n",
            f.file_path,
            f.start_line,
            f.end_line,
            kind.tag(),
            f.signature.lines().next().unwrap_or(f.name.as_str()),
            cfg_suffix,
            id_suffix,
        ));
    }
}

fn render_struct_lines(hits: &[(StructInfo, MatchKind)], show_ids: bool, out: &mut String) {
    for (s, kind) in hits {
        let id_suffix = if show_ids {
            format!(" [id: {}]", crate::struct_symbol_id(s))
        } else {
            String::new()
        };
        let cfg_suffix = format_cfg_annotation(&s.cfg_attrs)
            .map(|s| format!(" {}", s))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}:{}-{} {} - struct {}{}{}\n",
            s.file_path,
            s.start_line,
            s.end_line,
            kind.tag(),
            s.name,
            cfg_suffix,
            id_suffix,
        ));
    }
}

fn render_enum_lines(hits: &[(EnumInfo, MatchKind)], show_ids: bool, out: &mut String) {
    for (e, kind) in hits {
        let id_suffix = if show_ids {
            format!(" [id: {}]", crate::enum_symbol_id(e))
        } else {
            String::new()
        };
        let cfg_suffix = format_cfg_annotation(&e.cfg_attrs)
            .map(|s| format!(" {}", s))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}:{}-{} {} - enum {}{}{}\n",
            e.file_path,
            e.start_line,
            e.end_line,
            kind.tag(),
            e.name,
            cfg_suffix,
            id_suffix,
        ));
    }
}


fn truncate_in_order(
    fns: Vec<(FunctionInfo, MatchKind)>,
    structs: Vec<(StructInfo, MatchKind)>,
    enums: Vec<(EnumInfo, MatchKind)>,
    cap: usize,
) -> (
    Vec<(FunctionInfo, MatchKind)>,
    Vec<(StructInfo, MatchKind)>,
    Vec<(EnumInfo, MatchKind)>,
) {
    let mut remaining = cap;
    let take_fns = remaining.min(fns.len());
    remaining = remaining.saturating_sub(take_fns);
    let take_structs = remaining.min(structs.len());
    remaining = remaining.saturating_sub(take_structs);
    let take_enums = remaining.min(enums.len());
    (
        fns.into_iter().take(take_fns).collect(),
        structs.into_iter().take(take_structs).collect(),
        enums.into_iter().take(take_enums).collect(),
    )
}


/// JSON envelope that pairs a search hit with its `match_kind` discriminant (`"name"`, `"signature"`, or `"path"`).
#[derive(Debug, Serialize)]
struct TaggedHit<T: Serialize> {
    match_kind: &'static str,
    #[serde(flatten)]
    inner: T,
}

impl<T: Serialize> TaggedHit<T> {
    fn wrap(inner: T, kind: MatchKind) -> Self {
        Self {
            match_kind: kind.as_str(),
            inner,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::modes::AnalyzeSelection;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn run_find(path: &Path, request: FindRequest) -> Result<String, Box<dyn std::error::Error>> {
        use crate::cli::Args;
        use clap::Parser;
        let path_arg = path.to_string_lossy().to_string();
        let args = Args::try_parse_from(["rustgraph", "--path", &path_arg, "--json"])?;
        let project = ProjectData::load(&args.path, args.include_ignored);
        let captured_path = path.join("__find_out.txt");
        let mut owned_args = args;
        owned_args.output = Some(captured_path.clone());
        run(&owned_args, &project, request, None)?;
        Ok(fs::read_to_string(&captured_path)?)
    }

    #[test]
    fn find_default_threshold_matches_exact_name() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "pub fn alpha() {}\npub fn alphabet() {}\npub fn beta() {}\n",
        )
        .unwrap();
        let req = FindRequest {
            query: "alpha".to_string(),
            threshold: 0.85,
            selection: AnalyzeSelection::All,
            max_results: 50,
            show_ids: false,
            exact: false,
            in_path: None,
        };
        let out = run_find(dir.path(), req).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let names: Vec<String> = v["functions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"alpha".to_string()), "got: {:?}", names);
        assert!(!names.contains(&"beta".to_string()), "got: {:?}", names);
    }

    #[test]
    fn find_func_selection_excludes_structs() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "pub fn target_fn() {}\npub struct Target {}\n",
        )
        .unwrap();
        let req = FindRequest {
            query: "target".to_string(),
            threshold: 0.5,
            selection: AnalyzeSelection::Functions,
            max_results: 50,
            show_ids: false,
            exact: false,
            in_path: None,
        };
        let out = run_find(dir.path(), req).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(!v["functions"].as_array().unwrap().is_empty());
        assert!(v["structs"].as_array().unwrap().is_empty());
    }
}
