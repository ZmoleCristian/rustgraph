use serde::Serialize;

use super::super::changed::{ChangedRanges, fn_was_changed};
use super::super::modes::{AnalyzeSelection, FindRequest};
use super::super::project::ProjectData;
use super::super::symbol_id::WithSymbolId;
use crate::cli::Args;
use crate::{
    ConstInfo, EnumInfo, FunctionInfo, MatchKind, StructInfo, TypeDeclInfo, format_cfg_annotation,
    search_items_exact_with_kinds, search_items_with_kinds_and_fallback,
};

use super::switchboard::{READ_TOOL_HINT, write_string_output};

fn suggestion_pool(
    project: &ProjectData,
    selection: AnalyzeSelection,
) -> Vec<(String, &'static str)> {
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
    if selection.show_consts() {
        for c in &project.consts {
            pool.push((c.name.clone(), "const"));
        }
    }
    for t in &project.type_decls {
        let is_trait = t.kind == "trait";
        if (is_trait && selection.show_traits()) || (!is_trait && selection.show_aliases()) {
            pool.push((t.name.clone(), if is_trait { "trait" } else { "alias" }));
        }
    }
    pool
}

fn collect_suggestions(
    query: &str,
    project: &ProjectData,
    selection: AnalyzeSelection,
    limit: usize,
    requested_threshold: f64,
) -> Vec<(String, &'static str, f64)> {
    let eff = crate::effective_threshold(query, requested_threshold);
    score_suggestions(query, project, selection, limit, Some(eff))
}

/// Score `query` against every symbol name and return the top `limit` by similarity.
///
/// `below` excludes scores at or above the given threshold (the no-match path
/// only wants near-misses); `None` keeps everything including exact 1.0 hits
/// (the whitespace-tripwire path wants the real target surfaced).
fn score_suggestions(
    query: &str,
    project: &ProjectData,
    selection: AnalyzeSelection,
    limit: usize,
    below: Option<f64>,
) -> Vec<(String, &'static str, f64)> {
    let mut scored: Vec<(String, &'static str, f64)> = suggestion_pool(project, selection)
        .into_iter()
        .map(|(n, k)| {
            let s = crate::fuzzy_similarity(query, &n);
            (n, k, s)
        })
        .filter(|(_, _, s)| *s >= 0.5 && below.is_none_or(|b| *s < b))
        .collect();
    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    scored.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    scored.truncate(limit);
    scored
}

/// Implements `rustgraph find <query>` — fuzzy (or exact) symbol search across functions,
/// structs, enums, and consts.
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
    // Identifiers never contain whitespace; a spaced query means the caller is
    // treating find like a phrase/keyword search. Scoring the whole phrase
    // through the fuzzy matcher only produces garbage suggestions, so refuse
    // early and steer — including the best matches for the longest token,
    // which is almost always the identifier the caller meant.
    if let Some(spaced) = request
        .query
        .split('|')
        .map(str::trim)
        .find(|t| t.contains(char::is_whitespace))
    {
        let token = spaced
            .split_whitespace()
            .max_by_key(|t| t.len())
            .unwrap_or(spaced);
        let mut msg = format!(
            "query '{}' contains whitespace — find matches single identifiers, not phrases.\n\
             use '<a>|<b>' for OR, a kind filter (--func / --struct / --enum / --const / --trait / --alias), \
             or `rustgraph grep '<phrase>'` for text search",
            request.query
        );
        let suggestions = score_suggestions(token, project, request.selection, 5, None);
        if !suggestions.is_empty() {
            msg.push_str(&format!("\nclosest symbols to '{}':", token));
            for (name, kind, score) in &suggestions {
                msg.push_str(&format!(
                    "\n  - {} [{}]  (similarity {:.2})",
                    name, kind, score
                ));
            }
        }
        return Err(msg.into());
    }

    let (functions, structs, enums, consts, type_decls, short_query_fallback) = if request.exact {
        let (fns, structs_out, enums_out, consts_out, type_decls_out) =
            search_items_exact_with_kinds(
                &project.functions,
                &project.structs,
                &project.enums,
                &project.consts,
                &project.type_decls,
                &request.query,
            );
        (
            fns,
            structs_out,
            enums_out,
            consts_out,
            type_decls_out,
            false,
        )
    } else {
        search_items_with_kinds_and_fallback(
            &project.functions,
            &project.structs,
            &project.enums,
            &project.consts,
            &project.type_decls,
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
    let consts = if request.selection.show_consts() {
        if args.public_only {
            consts.into_iter().filter(|(c, _)| c.is_pub).collect()
        } else {
            consts
        }
    } else {
        Vec::new()
    };
    let type_decls: Vec<(TypeDeclInfo, MatchKind)> = type_decls
        .into_iter()
        .filter(|(t, _)| {
            let shown = if t.kind == "trait" {
                request.selection.show_traits()
            } else {
                request.selection.show_aliases()
            };
            shown && (!args.public_only || t.is_pub)
        })
        .collect();

    let (functions, structs, enums, consts, type_decls) = if let Some(needle) = &request.in_path {
        (
            functions
                .into_iter()
                .filter(|(f, _)| f.file_path.contains(needle.as_str()))
                .collect::<Vec<_>>(),
            structs
                .into_iter()
                .filter(|(s, _)| s.file_path.contains(needle.as_str()))
                .collect::<Vec<_>>(),
            enums
                .into_iter()
                .filter(|(e, _)| e.file_path.contains(needle.as_str()))
                .collect::<Vec<_>>(),
            consts
                .into_iter()
                .filter(|(c, _)| c.file_path.contains(needle.as_str()))
                .collect::<Vec<_>>(),
            type_decls
                .into_iter()
                .filter(|(t, _)| t.file_path.contains(needle.as_str()))
                .collect::<Vec<_>>(),
        )
    } else {
        (functions, structs, enums, consts, type_decls)
    };

    let (functions, structs, enums, consts, type_decls) = if let Some(ranges) = changed {
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
            consts
                .into_iter()
                .filter(|(c, _)| fn_was_changed(&c.file_path, c.start_line, c.end_line, ranges))
                .collect::<Vec<_>>(),
            type_decls
                .into_iter()
                .filter(|(t, _)| fn_was_changed(&t.file_path, t.start_line, t.end_line, ranges))
                .collect::<Vec<_>>(),
        )
    } else {
        (functions, structs, enums, consts, type_decls)
    };

    let total_hits =
        functions.len() + structs.len() + enums.len() + consts.len() + type_decls.len();

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
                .count()
            + consts
                .iter()
                .filter(|(_, k)| matches!(k, MatchKind::Name))
                .count()
            + type_decls
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
        let (
            relaxed_fns_raw,
            relaxed_structs_raw,
            relaxed_enums_raw,
            relaxed_consts_raw,
            relaxed_types_raw,
            _,
        ) = search_items_with_kinds_and_fallback(
            &project.functions,
            &project.structs,
            &project.enums,
            &project.consts,
            &project.type_decls,
            &request.query,
            0.7,
            args.match_signature,
        );

        let relaxed_fns: Vec<_> = if request.selection.show_functions() {
            let mut v = relaxed_fns_raw;
            if args.exclude_tests {
                v.retain(|(f, _)| !f.is_test);
            }
            if args.public_only {
                v.retain(|(f, _)| f.is_pub);
            }
            v
        } else {
            Vec::new()
        };
        let relaxed_structs: Vec<_> = if request.selection.show_structs() {
            let mut v = relaxed_structs_raw;
            if args.public_only {
                v.retain(|(s, _)| s.is_pub);
            }
            v
        } else {
            Vec::new()
        };
        let relaxed_enums: Vec<_> = if request.selection.show_enums() {
            let mut v = relaxed_enums_raw;
            if args.public_only {
                v.retain(|(e, _)| e.is_pub);
            }
            v
        } else {
            Vec::new()
        };
        let relaxed_consts: Vec<_> = if request.selection.show_consts() {
            let mut v = relaxed_consts_raw;
            if args.public_only {
                v.retain(|(c, _)| c.is_pub);
            }
            v
        } else {
            Vec::new()
        };
        let relaxed_types: Vec<_> = relaxed_types_raw
            .into_iter()
            .filter(|(t, _)| {
                let shown = if t.kind == "trait" {
                    request.selection.show_traits()
                } else {
                    request.selection.show_aliases()
                };
                shown && (!args.public_only || t.is_pub)
            })
            .collect();

        let relaxed_total = relaxed_fns.len()
            + relaxed_structs.len()
            + relaxed_enums.len()
            + relaxed_consts.len()
            + relaxed_types.len();
        if relaxed_total > 0 {
            let cap = 5usize;
            let (disp_fns, disp_structs, disp_enums, disp_consts, disp_types) = truncate_in_order(
                relaxed_fns,
                relaxed_structs,
                relaxed_enums,
                relaxed_consts,
                relaxed_types,
                cap,
            );
            let shown = disp_fns.len()
                + disp_structs.len()
                + disp_enums.len()
                + disp_consts.len()
                + disp_types.len();
            let note = format!(
                "note: 0 hits at threshold 0.85; relaxed to 0.7 and found {} candidate(s) (top {} shown). Pass --threshold X to control.",
                relaxed_total,
                shown.min(relaxed_total)
            );

            eprintln!("{}", note);
            if !args.json {
                let terms = exact_terms(&request.query);
                let mut relaxed_out = String::new();
                relaxed_out.push_str(&note);
                relaxed_out.push('\n');
                render_fn_lines(&disp_fns, request.show_ids, Some(&terms), &mut relaxed_out);
                render_struct_lines(
                    &disp_structs,
                    request.show_ids,
                    Some(&terms),
                    &mut relaxed_out,
                );
                render_enum_lines(
                    &disp_enums,
                    request.show_ids,
                    Some(&terms),
                    &mut relaxed_out,
                );
                render_const_lines(
                    &disp_consts,
                    request.show_ids,
                    Some(&terms),
                    &mut relaxed_out,
                );
                render_type_decl_lines(
                    &disp_types,
                    request.show_ids,
                    Some(&terms),
                    &mut relaxed_out,
                );
                relaxed_out.push_str(&format!("\n{}\n", READ_TOOL_HINT));
                write_string_output(args.output.as_deref(), relaxed_out.trim_end_matches('\n'))?;
            } else {
                #[derive(Serialize)]
                struct RelaxedOutput {
                    query: String,
                    threshold: f64,
                    relaxed_threshold: f64,
                    note: String,
                    functions: Vec<TaggedHit<WithSymbolId<FunctionInfo>>>,
                    structs: Vec<TaggedHit<WithSymbolId<StructInfo>>>,
                    enums: Vec<TaggedHit<WithSymbolId<EnumInfo>>>,
                    consts: Vec<TaggedHit<WithSymbolId<ConstInfo>>>,
                    type_decls: Vec<TaggedHit<WithSymbolId<TypeDeclInfo>>>,
                }
                let terms = exact_terms(&request.query);
                let payload = serde_json::to_string_pretty(&RelaxedOutput {
                    query: request.query.clone(),
                    threshold: request.threshold,
                    relaxed_threshold: 0.7,
                    note,
                    functions: disp_fns
                        .into_iter()
                        .map(|(f, k)| {
                            let score = Some(best_name_score(&f.name, &terms));
                            TaggedHit::wrap(WithSymbolId::wrap_fn(f), k, score)
                        })
                        .collect(),
                    structs: disp_structs
                        .into_iter()
                        .map(|(s, k)| {
                            let score = Some(best_name_score(&s.name, &terms));
                            TaggedHit::wrap(WithSymbolId::wrap_struct(s), k, score)
                        })
                        .collect(),
                    enums: disp_enums
                        .into_iter()
                        .map(|(e, k)| {
                            let score = Some(best_name_score(&e.name, &terms));
                            TaggedHit::wrap(WithSymbolId::wrap_enum(e), k, score)
                        })
                        .collect(),
                    consts: disp_consts
                        .into_iter()
                        .map(|(c, k)| {
                            let score = Some(best_name_score(&c.name, &terms));
                            TaggedHit::wrap(WithSymbolId::wrap_const(c), k, score)
                        })
                        .collect(),
                    type_decls: disp_types
                        .into_iter()
                        .map(|(t, k)| {
                            let score = Some(best_name_score(&t.name, &terms));
                            TaggedHit::wrap(WithSymbolId::wrap_type_decl(t), k, score)
                        })
                        .collect(),
                })?;
                write_string_output(args.output.as_deref(), &payload)?;
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
                    consts: Vec<TaggedHit<WithSymbolId<ConstInfo>>>,
                    type_decls: Vec<TaggedHit<WithSymbolId<TypeDeclInfo>>>,
                }
                let payload = serde_json::to_string_pretty(&EmptyOutput {
                    query: request.query.clone(),
                    threshold: request.threshold,
                    functions: Vec::new(),
                    structs: Vec::new(),
                    enums: Vec::new(),
                    consts: Vec::new(),
                    type_decls: Vec::new(),
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
                "no match for query '{}' (threshold {:.2}); lower the threshold or check spelling",
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
    let (display_functions, display_structs, display_enums, display_consts, display_types) =
        if truncated {
            truncate_in_order(
                functions.clone(),
                structs.clone(),
                enums.clone(),
                consts.clone(),
                type_decls.clone(),
                max_results,
            )
        } else {
            (
                functions.clone(),
                structs.clone(),
                enums.clone(),
                consts.clone(),
                type_decls.clone(),
            )
        };

    let query_terms = exact_terms(&request.query);
    let (fn_exact, fn_fuzzy) =
        partition_by_exact_name(display_functions, &query_terms, |f| f.name.as_str());
    let (struct_exact, struct_fuzzy) =
        partition_by_exact_name(display_structs, &query_terms, |s| s.name.as_str());
    let (enum_exact, enum_fuzzy) =
        partition_by_exact_name(display_enums, &query_terms, |e| e.name.as_str());
    let (const_exact, const_fuzzy) =
        partition_by_exact_name(display_consts, &query_terms, |c| c.name.as_str());
    let (type_exact, type_fuzzy) =
        partition_by_exact_name(display_types, &query_terms, |t| t.name.as_str());
    let any_exact = !fn_exact.is_empty()
        || !struct_exact.is_empty()
        || !enum_exact.is_empty()
        || !const_exact.is_empty()
        || !type_exact.is_empty();
    let any_fuzzy = !fn_fuzzy.is_empty()
        || !struct_fuzzy.is_empty()
        || !enum_fuzzy.is_empty()
        || !const_fuzzy.is_empty()
        || !type_fuzzy.is_empty();

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
            consts: Vec<TaggedHit<WithSymbolId<ConstInfo>>>,
            type_decls: Vec<TaggedHit<WithSymbolId<TypeDeclInfo>>>,
            functions_exact: Vec<TaggedHit<WithSymbolId<FunctionInfo>>>,
            functions_fuzzy: Vec<TaggedHit<WithSymbolId<FunctionInfo>>>,
            structs_exact: Vec<TaggedHit<WithSymbolId<StructInfo>>>,
            structs_fuzzy: Vec<TaggedHit<WithSymbolId<StructInfo>>>,
            enums_exact: Vec<TaggedHit<WithSymbolId<EnumInfo>>>,
            enums_fuzzy: Vec<TaggedHit<WithSymbolId<EnumInfo>>>,
            consts_exact: Vec<TaggedHit<WithSymbolId<ConstInfo>>>,
            consts_fuzzy: Vec<TaggedHit<WithSymbolId<ConstInfo>>>,
            type_decls_exact: Vec<TaggedHit<WithSymbolId<TypeDeclInfo>>>,
            type_decls_fuzzy: Vec<TaggedHit<WithSymbolId<TypeDeclInfo>>>,
        }

        let score_of = |name: &str| best_name_score(name, &query_terms);
        let wrap_fns = |hits: Vec<(FunctionInfo, MatchKind)>,
                        fuzzy: bool|
         -> Vec<TaggedHit<WithSymbolId<FunctionInfo>>> {
            hits.into_iter()
                .map(|(f, k)| {
                    let score = fuzzy.then(|| score_of(&f.name));
                    TaggedHit::wrap(WithSymbolId::wrap_fn(f), k, score)
                })
                .collect()
        };
        let wrap_structs = |hits: Vec<(StructInfo, MatchKind)>,
                            fuzzy: bool|
         -> Vec<TaggedHit<WithSymbolId<StructInfo>>> {
            hits.into_iter()
                .map(|(s, k)| {
                    let score = fuzzy.then(|| score_of(&s.name));
                    TaggedHit::wrap(WithSymbolId::wrap_struct(s), k, score)
                })
                .collect()
        };
        let wrap_enums = |hits: Vec<(EnumInfo, MatchKind)>,
                          fuzzy: bool|
         -> Vec<TaggedHit<WithSymbolId<EnumInfo>>> {
            hits.into_iter()
                .map(|(e, k)| {
                    let score = fuzzy.then(|| score_of(&e.name));
                    TaggedHit::wrap(WithSymbolId::wrap_enum(e), k, score)
                })
                .collect()
        };
        let wrap_consts = |hits: Vec<(ConstInfo, MatchKind)>,
                           fuzzy: bool|
         -> Vec<TaggedHit<WithSymbolId<ConstInfo>>> {
            hits.into_iter()
                .map(|(c, k)| {
                    let score = fuzzy.then(|| score_of(&c.name));
                    TaggedHit::wrap(WithSymbolId::wrap_const(c), k, score)
                })
                .collect()
        };
        let wrap_types = |hits: Vec<(TypeDeclInfo, MatchKind)>,
                          fuzzy: bool|
         -> Vec<TaggedHit<WithSymbolId<TypeDeclInfo>>> {
            hits.into_iter()
                .map(|(t, k)| {
                    let score = fuzzy.then(|| score_of(&t.name));
                    TaggedHit::wrap(WithSymbolId::wrap_type_decl(t), k, score)
                })
                .collect()
        };

        let mut functions_all = wrap_fns(fn_exact.clone(), false);
        functions_all.extend(wrap_fns(fn_fuzzy.clone(), true));
        let mut structs_all = wrap_structs(struct_exact.clone(), false);
        structs_all.extend(wrap_structs(struct_fuzzy.clone(), true));
        let mut enums_all = wrap_enums(enum_exact.clone(), false);
        enums_all.extend(wrap_enums(enum_fuzzy.clone(), true));
        let mut consts_all = wrap_consts(const_exact.clone(), false);
        consts_all.extend(wrap_consts(const_fuzzy.clone(), true));
        let mut types_all = wrap_types(type_exact.clone(), false);
        types_all.extend(wrap_types(type_fuzzy.clone(), true));

        let payload = serde_json::to_string_pretty(&FindOutput {
            query: request.query.clone(),
            threshold: if request.exact {
                None
            } else {
                Some(request.threshold)
            },
            exact: request.exact,
            functions: functions_all,
            structs: structs_all,
            enums: enums_all,
            consts: consts_all,
            type_decls: types_all,
            functions_exact: wrap_fns(fn_exact, false),
            functions_fuzzy: wrap_fns(fn_fuzzy, true),
            structs_exact: wrap_structs(struct_exact, false),
            structs_fuzzy: wrap_structs(struct_fuzzy, true),
            enums_exact: wrap_enums(enum_exact, false),
            enums_fuzzy: wrap_enums(enum_fuzzy, true),
            consts_exact: wrap_consts(const_exact, false),
            consts_fuzzy: wrap_consts(const_fuzzy, true),
            type_decls_exact: wrap_types(type_exact, false),
            type_decls_fuzzy: wrap_types(type_fuzzy, true),
        })?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {
        let header_mode = if request.exact {
            "(exact mode)".to_string()
        } else {
            format!("(threshold {:.2})", request.threshold)
        };
        let trait_count = type_decls.iter().filter(|(t, _)| t.kind == "trait").count();
        let alias_count = type_decls.len() - trait_count;
        eprintln!(
            "rustgraph find '{}' {}: {} fn, {} struct, {} enum, {} const, {} trait, {} alias",
            request.query,
            header_mode,
            functions.len(),
            structs.len(),
            enums.len(),
            consts.len(),
            trait_count,
            alias_count
        );
        let mut out = String::new();

        // A fuzzy hit must never wear an exact hit's clothes: when only fuzzy
        // matches exist, say so up front and tag each line with its score.
        let render_both = any_exact && any_fuzzy;
        if render_both {
            out.push_str("== Exact name matches ==\n");
        } else if any_fuzzy && !request.exact {
            out.push_str(&format!(
                "note: no exact name match for '{}'; nearest fuzzy matches:\n",
                request.query
            ));
        }
        render_fn_lines(&fn_exact, request.show_ids, None, &mut out);
        render_struct_lines(&struct_exact, request.show_ids, None, &mut out);
        render_enum_lines(&enum_exact, request.show_ids, None, &mut out);
        render_const_lines(&const_exact, request.show_ids, None, &mut out);
        render_type_decl_lines(&type_exact, request.show_ids, None, &mut out);
        if render_both {
            out.push_str("\n== Fuzzy matches ==\n");
        }
        render_fn_lines(&fn_fuzzy, request.show_ids, Some(&query_terms), &mut out);
        render_struct_lines(
            &struct_fuzzy,
            request.show_ids,
            Some(&query_terms),
            &mut out,
        );
        render_enum_lines(&enum_fuzzy, request.show_ids, Some(&query_terms), &mut out);
        render_const_lines(&const_fuzzy, request.show_ids, Some(&query_terms), &mut out);
        render_type_decl_lines(&type_fuzzy, request.show_ids, Some(&query_terms), &mut out);
        if truncated {
            out.push_str(&format!(
                "(showing {} of {}; use --max-results to expand)\n",
                max_results, total_hits
            ));
        }
        if total_hits > 0 {
            out.push_str(&format!("\n{}\n", READ_TOOL_HINT));
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

fn best_name_score(name: &str, terms: &[String]) -> f64 {
    terms
        .iter()
        .filter_map(|t| crate::name_score(t, name, 0.0))
        .fold(0.0, f64::max)
}

/// Render the match-kind tag, appending the fuzzy similarity score when the
/// hit is a non-exact name match (`[name ~0.89]`). Sig/path tags pass through.
fn render_tag(kind: MatchKind, name: &str, fuzzy_terms: Option<&[String]>) -> String {
    match (kind, fuzzy_terms) {
        (MatchKind::Name, Some(terms)) => {
            format!("[name ~{:.2}]", best_name_score(name, terms))
        }
        _ => kind.tag().to_string(),
    }
}

fn render_fn_lines(
    hits: &[(FunctionInfo, MatchKind)],
    show_ids: bool,
    fuzzy_terms: Option<&[String]>,
    out: &mut String,
) {
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
            render_tag(*kind, &f.name, fuzzy_terms),
            f.signature.lines().next().unwrap_or(f.name.as_str()),
            cfg_suffix,
            id_suffix,
        ));
    }
}

fn render_struct_lines(
    hits: &[(StructInfo, MatchKind)],
    show_ids: bool,
    fuzzy_terms: Option<&[String]>,
    out: &mut String,
) {
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
            render_tag(*kind, &s.name, fuzzy_terms),
            s.name,
            cfg_suffix,
            id_suffix,
        ));
    }
}

fn render_enum_lines(
    hits: &[(EnumInfo, MatchKind)],
    show_ids: bool,
    fuzzy_terms: Option<&[String]>,
    out: &mut String,
) {
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
            render_tag(*kind, &e.name, fuzzy_terms),
            e.name,
            cfg_suffix,
            id_suffix,
        ));
    }
}

fn render_type_decl_lines(
    hits: &[(TypeDeclInfo, MatchKind)],
    show_ids: bool,
    fuzzy_terms: Option<&[String]>,
    out: &mut String,
) {
    for (t, kind) in hits {
        let id_suffix = if show_ids {
            format!(" [id: {}]", crate::type_decl_symbol_id(t))
        } else {
            String::new()
        };
        let cfg_suffix = format_cfg_annotation(&t.cfg_attrs)
            .map(|s| format!(" {}", s))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}:{}-{} {} - {}{}{}\n",
            t.file_path,
            t.start_line,
            t.end_line,
            render_tag(*kind, &t.name, fuzzy_terms),
            t.signature,
            cfg_suffix,
            id_suffix,
        ));
    }
}

fn render_const_lines(
    hits: &[(ConstInfo, MatchKind)],
    show_ids: bool,
    fuzzy_terms: Option<&[String]>,
    out: &mut String,
) {
    for (c, kind) in hits {
        let id_suffix = if show_ids {
            format!(" [id: {}]", crate::const_symbol_id(c))
        } else {
            String::new()
        };
        let cfg_suffix = format_cfg_annotation(&c.cfg_attrs)
            .map(|s| format!(" {}", s))
            .unwrap_or_default();
        out.push_str(&format!(
            "{}:{}-{} {} - {}{}{}\n",
            c.file_path,
            c.start_line,
            c.end_line,
            render_tag(*kind, &c.name, fuzzy_terms),
            c.signature,
            cfg_suffix,
            id_suffix,
        ));
    }
}

fn truncate_in_order(
    fns: Vec<(FunctionInfo, MatchKind)>,
    structs: Vec<(StructInfo, MatchKind)>,
    enums: Vec<(EnumInfo, MatchKind)>,
    consts: Vec<(ConstInfo, MatchKind)>,
    type_decls: Vec<(TypeDeclInfo, MatchKind)>,
    cap: usize,
) -> (
    Vec<(FunctionInfo, MatchKind)>,
    Vec<(StructInfo, MatchKind)>,
    Vec<(EnumInfo, MatchKind)>,
    Vec<(ConstInfo, MatchKind)>,
    Vec<(TypeDeclInfo, MatchKind)>,
) {
    let mut remaining = cap;
    let take_fns = remaining.min(fns.len());
    remaining = remaining.saturating_sub(take_fns);
    let take_structs = remaining.min(structs.len());
    remaining = remaining.saturating_sub(take_structs);
    let take_enums = remaining.min(enums.len());
    remaining = remaining.saturating_sub(take_enums);
    let take_consts = remaining.min(consts.len());
    remaining = remaining.saturating_sub(take_consts);
    let take_types = remaining.min(type_decls.len());
    (
        fns.into_iter().take(take_fns).collect(),
        structs.into_iter().take(take_structs).collect(),
        enums.into_iter().take(take_enums).collect(),
        consts.into_iter().take(take_consts).collect(),
        type_decls.into_iter().take(take_types).collect(),
    )
}

/// JSON envelope that pairs a search hit with its `match_kind` discriminant (`"name"`,
/// `"signature"`, or `"path"`) and, for fuzzy name hits, the similarity score.
#[derive(Debug, Serialize)]
struct TaggedHit<T: Serialize> {
    match_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    match_score: Option<f64>,
    #[serde(flatten)]
    inner: T,
}

impl<T: Serialize> TaggedHit<T> {
    fn wrap(inner: T, kind: MatchKind, score: Option<f64>) -> Self {
        Self {
            match_kind: kind.as_str(),
            match_score: score,
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

    fn request(query: &str) -> FindRequest {
        FindRequest {
            query: query.to_string(),
            threshold: 0.85,
            selection: AnalyzeSelection::ALL,
            max_results: 50,
            show_ids: false,
            exact: false,
            in_path: None,
        }
    }

    #[test]
    fn find_default_threshold_matches_exact_name() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "pub fn alpha() {}\npub fn alphabet() {}\npub fn beta() {}\n",
        )
        .unwrap();
        let out = run_find(dir.path(), request("alpha")).unwrap();
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
        let mut req = request("target");
        req.threshold = 0.5;
        req.selection = AnalyzeSelection::from_flags(true, false, false, false, false, false);
        let out = run_find(dir.path(), req).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(!v["functions"].as_array().unwrap().is_empty());
        assert!(v["structs"].as_array().unwrap().is_empty());
    }

    #[test]
    fn find_screaming_snake_const_beats_pascal_struct_homograph() {
        // The redstring field-report scenario: `find KIND_META` must return the
        // const as an exact hit, not silently substitute the KindMeta struct.
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("entity.rs"),
            "pub struct KindMeta { pub label: &'static str }\n\
             pub const KIND_META: [KindMeta; 1] = [KindMeta { label: \"x\" }];\n",
        )
        .unwrap();
        let out = run_find(dir.path(), request("KIND_META")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let exact_consts = v["consts_exact"].as_array().unwrap();
        assert_eq!(exact_consts.len(), 1, "const must be an exact hit: {}", out);
        assert_eq!(exact_consts[0]["name"].as_str(), Some("KIND_META"));
        // The struct may still appear, but only in the fuzzy partition with a score.
        for s in v["structs"].as_array().unwrap() {
            assert!(
                s["match_score"].is_number(),
                "fuzzy struct hit must carry match_score: {}",
                s
            );
        }
        assert!(v["structs_exact"].as_array().unwrap().is_empty());
    }

    #[test]
    fn find_whitespace_query_errors_with_steering() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "pub const KIND_META: u8 = 1;\n").unwrap();
        let err = run_find(dir.path(), request("KIND_META static"))
            .err()
            .expect("whitespace query must error");
        let msg = err.to_string();
        assert!(msg.contains("contains whitespace"), "got: {msg}");
        assert!(
            msg.contains("KIND_META [const]"),
            "tripwire must surface the exact symbol: {msg}"
        );
        assert!(
            !msg.contains("--search-threshold"),
            "no dead-end flag hints: {msg}"
        );
    }
}
