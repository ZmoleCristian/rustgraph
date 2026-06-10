use std::collections::HashSet;

use serde::Serialize;

use super::super::callers::{CallerFunction, CallersReport, build_callers_report, build_callers_report_with_options};
use super::super::changed::{ChangedRanges, fn_was_changed};
use super::super::modes::CallersRequest;
use super::super::project::ProjectData;
use super::super::render::render_callers_text;
use super::super::symbol_id::inject_symbol_ids;
use crate::FunctionInfo;
use crate::cli::Args;
use crate::function_id;
use crate::index::qualifier::{
    candidate_matches_needles, module_path_qualifier, qualifier_for_callee, QualifierNeedles,
    ResolutionStats,
};

use super::switchboard::{READ_TOOL_HINT, write_string_output};


/// Implements `rustgraph callers <fn>` — reverse-dependency walk that reports every function that calls the target.
///
/// Supports qualifier filtering (method receiver or module path), `--depth N` for transitive
/// callers, `--flat` for a deduplicated line-per-caller list, and optional `--callers-in` /
/// `--target-in` path substring filters. Returns an error on ambiguous matches unless `--all`
/// or `--symbol-id` is supplied.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: CallersRequest,
    changed: Option<&ChangedRanges>,
) -> Result<(), Box<dyn std::error::Error>> {


    let resolved_query = if let Some(id) = &request.symbol_id {
        if id.starts_with("struct:") || id.starts_with("enum:") {
            return Err(format!(
                "callers operates on functions only; '{}' is a {}. Use `refs` or `impls` for a {}'s uses instead.",
                id,
                if id.starts_with("struct:") { "struct" } else { "enum" },
                if id.starts_with("struct:") { "struct" } else { "enum" }
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


    enum QualifierFilter {
        None,
        Type(String),
        ModulePath(Vec<String>),
    }
    let (lookup_query, qualifier_filter): (String, QualifierFilter) = if request.symbol_id.is_some() {

        (resolved_query.clone(), QualifierFilter::None)
    } else if let Some(q) = qualifier_for_callee(&resolved_query) {
        let bare = resolved_query
            .rsplit(|c: char| c == '.' || c == ':')
            .next()
            .unwrap_or(resolved_query.as_str())
            .to_string();
        (bare, QualifierFilter::Type(q))
    } else if let Some(segs) = module_path_qualifier(&resolved_query) {


        let bare = resolved_query
            .rsplit("::")
            .next()
            .unwrap_or(resolved_query.as_str())
            .to_string();
        (bare, QualifierFilter::ModulePath(segs))
    } else {
        (resolved_query.clone(), QualifierFilter::None)
    };

    let mut report = build_callers_report_with_options(
        project,
        &lookup_query,
        args.search_threshold,
        request.before_context,
        request.after_context,
        args.match_signature,
        Some(&args.path),
        &args.also,
    )?;


    let mut resolution_stats = ResolutionStats::default();
    let strict_only = args.strict_resolution;


    match &qualifier_filter {
        QualifierFilter::None => {}
        QualifierFilter::Type(q) => {
            let before = report.matches.len();
            let pre_filter_matches = report.matches.clone();
            // Pre-format the qualifier needles once before retain, otherwise
            // `format!` allocates two strings per match in the loop.
            let needles = QualifierNeedles::new(q.as_str());
            report
                .matches
                .retain(|m| candidate_matches_needles(&m.info, &needles));
            if report.matches.is_empty() && before > 0 {
                if strict_only {


                    resolution_stats.strict_suppressed += before;
                    resolution_stats.emit_stderr_note();
                    return Err(format!(
                        "no function matches qualified query '{}': {} candidate(s) shared the bare name '{}' but none belong to type `{}` (--strict-resolution: loose fallback suppressed). Drop the qualifier (just '{}') or remove --strict-resolution to see them all.",
                        resolved_query, before, lookup_query, q, lookup_query
                    )
                    .into());
                }


                report.matches = pre_filter_matches;
                resolution_stats.fell_back_to_loose += before;
            } else {
                resolution_stats.strict += before;
            }
            report.query = resolved_query.clone();
        }
        QualifierFilter::ModulePath(segs) => {


            let before = report.matches.len();
            let useful: Vec<&str> = segs
                .iter()
                .map(|s| s.as_str())
                .filter(|s| !matches!(*s, "crate" | "self" | "super"))
                .collect();
            let mut narrowed_matched = false;
            if let Some(last) = useful.last() {
                let needle_slash = format!("/{}/", last);
                let needle_stem = format!("/{}.", last);
                let narrowed: Vec<_> = report
                    .matches
                    .iter()
                    .filter(|m| {
                        m.info.file_path.contains(&needle_slash)
                            || m.info.file_path.contains(&needle_stem)
                    })
                    .cloned()
                    .collect();
                if !narrowed.is_empty() {
                    report.matches = narrowed;
                    narrowed_matched = true;
                }
            }

            report.query = resolved_query.clone();


            if !narrowed_matched && before > 1 && !useful.is_empty() {
                if strict_only {
                    resolution_stats.strict_suppressed += before;


                    report.matches.clear();
                } else {
                    resolution_stats.fell_back_to_loose += before;
                }
            } else if narrowed_matched {
                resolution_stats.strict += report.matches.len();
            }
        }
    }


    if let Some(needle) = &request.target_in {
        let before = report.matches.len();
        report.matches.retain(|m| m.info.file_path.contains(needle));
        if report.matches.is_empty() && before > 0 {
            eprintln!(
                "hint: --target-in '{}' filtered out all {} match(es); try a shorter substring",
                needle, before
            );
        }
    }


    let pre_filter_caller_counts: Vec<usize> = if request.callers_in.is_some() {
        report.matches.iter().map(|m| m.callers.len()).collect()
    } else {
        Vec::new()
    };
    if let Some(needle) = &request.callers_in {
        for m in report.matches.iter_mut() {
            m.callers.retain(|c| c.info.file_path.contains(needle));


            m.call_sites_total = m.callers.iter().map(|c| c.call_sites.len()).sum();
        }


        let any_emptied: bool = report
            .matches
            .iter()
            .zip(pre_filter_caller_counts.iter())
            .any(|(m, &before)| before > 0 && m.callers.is_empty());
        if any_emptied {
            let total_before: usize = pre_filter_caller_counts.iter().sum();
            let total_after: usize = report.matches.iter().map(|m| m.callers.len()).sum();
            if total_after == 0 && total_before > 0 {
                eprintln!(
                    "hint: --callers-in '{}' dropped all {} caller(s) for {}; try a shorter substring",
                    needle, total_before, request.query
                );
            }
        }
    }
    if args.exclude_tests {


        for m in report.matches.iter_mut() {
            m.callers.retain(|c| !c.info.is_test);
            m.call_sites_total = m.callers.iter().map(|c| c.call_sites.len()).sum();
        }
    }


    if let Some(ranges) = changed {
        for m in report.matches.iter_mut() {
            m.callers.retain(|c| {
                fn_was_changed(&c.info.file_path, c.info.start_line, c.info.end_line, ranges)
            });
            m.call_sites_total = m.callers.iter().map(|c| c.call_sites.len()).sum();
        }
    }

    let total_callers: usize = report.matches.iter().map(|m| m.callers.len()).sum();
    if report.matches.is_empty() {


        if let Some(hint) = type_redirect_hint(project, &request.query, args.search_threshold) {
            return Err(hint.into());
        }
        return Err(format!(
            "no function matches query '{}' (try `rustgraph find {}` or lower --search-threshold)",
            request.query, request.query
        )
        .into());
    } else if report.matches.len() > 1 && !request.all && (request.depth == 0 || request.depth >= 2) {


        let n = report.matches.len();
        let mut msg = format!(
            "ambiguous: {} matches for '{}' (depth={}). Transitive multi-match requires explicit --all. Disambiguate with --symbol-id or --target-in:",
            n, request.query, request.depth
        );
        for m in &report.matches {
            let sym_id = format!("{}:{}:{}", m.info.file_path, m.info.start_line, m.info.name);
            msg.push_str(&format!("\n  {}  --symbol-id '{}'", sym_id, sym_id));
        }
        msg.push_str(&format!(
            "\n(or pass `--all` to process all {} transitively — N×subtree work)",
            n
        ));
        return Err(msg.into());
    } else if report.matches.len() > request.ambiguity_cap && !request.all {


        let n = report.matches.len();
        let mut msg = format!(
            "ambiguous: {} matches for '{}'. Disambiguate with --symbol-id or --target-in:",
            n, request.query
        );
        for m in &report.matches {
            let sym_id = format!("{}:{}:{}", m.info.file_path, m.info.start_line, m.info.name);
            msg.push_str(&format!("\n  {}  --symbol-id '{}'", sym_id, sym_id));
            if n <= 5 {

                let preview = read_body_preview(&args.path, &m.info.file_path, m.info.start_line, 3);
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
    } else if report.matches.len() > 1 {
        eprintln!(
            "note: query '{}' matched {} functions — output covers all of them:",
            request.query,
            report.matches.len()
        );
        for m in &report.matches {
            eprintln!("  - {}:{} {}", m.info.file_path, m.info.start_line, m.info.name);
        }
    }
    if !report.matches.is_empty() && total_callers == 0 {
        let names: Vec<String> = report
            .matches
            .iter()
            .map(|m| format!("{}:{}", m.info.file_path, m.info.start_line))
            .collect();
        eprintln!(
            "found {} function(s) matching '{}' but 0 call sites recorded: {}",
            report.matches.len(),
            request.query,
            names.join(", ")
        );
    }


    if request.flat {
        if args.json {
            return Err("--flat and --json are mutually exclusive; pick one output mode".into());
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut lines: Vec<String> = Vec::new();

        let max_depth = if request.depth == 0 {
            usize::MAX
        } else {
            request.depth
        };

        if request.depth == 1 {

            for m in &report.matches {
                for c in &m.callers {
                    let key = format!("{}:{}:{}", c.info.file_path, c.info.start_line, c.info.name);
                    if seen.insert(key.clone()) {
                        lines.push(key);
                    }
                }
            }
        } else {


            let mut visited: HashSet<String> = HashSet::new();
            for direct_match in &report.matches {
                visited.insert(function_id(&direct_match.info));
            }
            for direct_match in &report.matches {
                for caller in &direct_match.callers {
                    if args.exclude_tests && caller.info.is_test {
                        continue;
                    }
                    if let Some(needle) = request.callers_in.as_deref()
                        && !caller.info.file_path.contains(needle)
                    {
                        continue;
                    }
                    if let Some(ranges) = changed
                        && !fn_was_changed(&caller.info.file_path, caller.info.start_line, caller.info.end_line, ranges)
                    {
                        continue;
                    }
                    walk_flat(
                        project,
                        args.search_threshold,
                        caller,
                        1,
                        max_depth,
                        args.exclude_tests,
                        request.callers_in.as_deref(),
                        changed,
                        &mut visited,
                        &mut seen,
                        &mut lines,
                    )?;
                }
            }
        }

        let payload = lines.join("\n");
        write_string_output(args.output.as_deref(), &payload)?;
        resolution_stats.emit_stderr_note();
        return Ok(());
    }


    if request.depth == 1 {


        let max_results = request.max_results.unwrap_or(50);
        let total_callers_across_matches: usize = report.matches.iter().map(|m| m.callers.len()).sum();
        let truncated = max_results != 0 && total_callers_across_matches > max_results;
        if truncated {
            let mut remaining = max_results;
            for m in report.matches.iter_mut() {
                let keep = remaining.min(m.callers.len());
                m.callers.truncate(keep);
                m.call_sites_total = m.callers.iter().map(|c| c.call_sites.len()).sum();
                remaining = remaining.saturating_sub(keep);
                if remaining == 0 {
                    break;
                }
            }
        }
        if args.json {
            let mut value = serde_json::to_value(&report)?;
            inject_symbol_ids(&mut value);
            let payload = serde_json::to_string_pretty(&value)?;
            write_string_output(args.output.as_deref(), &payload)?;
        } else {

            let mut rendered = render_callers_text(&report);
            if truncated {
                rendered.push_str(&format!(
                    "\n(showing {} of {}; use --max-results to expand)\n",
                    max_results, total_callers_across_matches
                ));
            }
            rendered.push_str(&format!("\n{}\n", READ_TOOL_HINT));
            write_string_output(args.output.as_deref(), rendered.trim_end_matches('\n'))?;
        }

        resolution_stats.emit_stderr_note();
        return Ok(());
    }

    let max_depth = if request.depth == 0 {
        usize::MAX
    } else {
        request.depth
    };
    let transitive = build_transitive(
        project,
        args.search_threshold,
        &report,
        max_depth,
        args.exclude_tests,
        request.before_context,
        request.after_context,
        request.callers_in.as_deref(),
        changed,
    )?;

    if args.json {
        let mut value = serde_json::to_value(&transitive)?;
        inject_symbol_ids(&mut value);
        let payload = serde_json::to_string_pretty(&value)?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {
        let mut rendered = render_transitive_text(&transitive);
        rendered.push_str(&format!("\n{}\n", READ_TOOL_HINT));
        write_string_output(args.output.as_deref(), rendered.trim_end_matches('\n'))?;
    }


    resolution_stats.emit_stderr_note();
    Ok(())
}

use crate::app::callers::CallerSite;

/// One node in the transitive caller tree, holding the function info, its depth from the query
/// target, and recursively resolved callers.
#[derive(Debug, Clone, Serialize)]
struct TransitiveCallerNode {
    info: FunctionInfo,
    depth: usize,
    direct_call_sites: usize,


    call_sites: Vec<CallerSite>,
    callers: Vec<TransitiveCallerNode>,
}

/// A single matched function together with its full transitive caller tree.
#[derive(Debug, Clone, Serialize)]
struct TransitiveMatch {
    info: FunctionInfo,
    callers: Vec<TransitiveCallerNode>,
}

/// Top-level output for a transitive `callers --depth N` query, containing all matched roots and
/// their caller trees.
#[derive(Debug, Clone, Serialize)]
struct TransitiveReport {
    query: String,
    depth_limit: usize,
    matches: Vec<TransitiveMatch>,
}

#[allow(clippy::too_many_arguments)]
fn build_transitive(
    project: &ProjectData,
    threshold: f64,
    direct_report: &CallersReport,
    max_depth: usize,
    exclude_tests: bool,
    before_context: usize,
    after_context: usize,
    callers_in: Option<&str>,
    changed: Option<&ChangedRanges>,
) -> Result<TransitiveReport, Box<dyn std::error::Error>> {
    let mut matches = Vec::new();
    for direct_match in &direct_report.matches {
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(function_id(&direct_match.info));
        let mut callers: Vec<TransitiveCallerNode> = Vec::new();
        for caller in &direct_match.callers {
            if exclude_tests && caller.info.is_test {
                continue;
            }


            if let Some(needle) = callers_in
                && !caller.info.file_path.contains(needle)
            {
                continue;
            }

            if let Some(ranges) = changed
                && !fn_was_changed(&caller.info.file_path, caller.info.start_line, caller.info.end_line, ranges)
            {
                continue;
            }
            walk(project, threshold, caller, 1, max_depth, exclude_tests, before_context, after_context, callers_in, changed, &mut visited, &mut callers)?;
        }
        matches.push(TransitiveMatch {
            info: direct_match.info.clone(),
            callers,
        });
    }
    Ok(TransitiveReport {
        query: direct_report.query.clone(),
        depth_limit: max_depth,
        matches,
    })
}

#[allow(clippy::too_many_arguments)]
fn walk(
    project: &ProjectData,
    threshold: f64,
    caller: &CallerFunction,
    current_depth: usize,
    max_depth: usize,
    exclude_tests: bool,
    before_context: usize,
    after_context: usize,
    callers_in: Option<&str>,
    changed: Option<&ChangedRanges>,
    visited: &mut HashSet<String>,
    out: &mut Vec<TransitiveCallerNode>,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = function_id(&caller.info);
    if !visited.insert(id.clone()) {
        return Ok(());
    }
    let mut node = TransitiveCallerNode {
        info: caller.info.clone(),
        depth: current_depth,
        direct_call_sites: caller.call_sites.len(),
        call_sites: caller.call_sites.clone(),
        callers: Vec::new(),
    };
    if current_depth < max_depth {
        let next_query = format!("{}:{}", caller.info.file_path, caller.info.start_line);

        let next_report = build_callers_report(project, &next_query, threshold, before_context, after_context)?;
        for m in &next_report.matches {

            if function_id(&m.info) != id {
                continue;
            }
            for c in &m.callers {
                if exclude_tests && c.info.is_test {
                    continue;
                }


                if let Some(needle) = callers_in
                    && !c.info.file_path.contains(needle)
                {
                    continue;
                }


                if let Some(ranges) = changed
                    && !fn_was_changed(&c.info.file_path, c.info.start_line, c.info.end_line, ranges)
                {
                    continue;
                }
                walk(project, threshold, c, current_depth + 1, max_depth, exclude_tests, before_context, after_context, callers_in, changed, visited, &mut node.callers)?;
            }
        }
    }
    out.push(node);
    Ok(())
}

fn render_transitive_text(report: &TransitiveReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "rustgraph callers '{}' --depth {} ({} match(es))\n",
        report.query,
        if report.depth_limit == usize::MAX {
            "∞".to_string()
        } else {
            report.depth_limit.to_string()
        },
        report.matches.len()
    ));
    for m in &report.matches {
        out.push_str(&format!(
            "\n● {}  ({}:{})  — {} direct caller(s)\n",
            m.info.name,
            short_path(&m.info.file_path),
            m.info.start_line,
            m.callers.len()
        ));
        let total = m.callers.len();
        for (i, c) in m.callers.iter().enumerate() {
            render_node(c, "", i + 1 == total, &mut out);
        }
    }
    out
}

fn render_node(node: &TransitiveCallerNode, prefix: &str, is_last: bool, out: &mut String) {
    let connector = if is_last { "└── " } else { "├── " };
    out.push_str(&format!(
        "{}{}{}  ({}:{})  [d={}, {} site(s)]\n",
        prefix,
        connector,
        node.info.name,
        short_path(&node.info.file_path),
        node.info.start_line,
        node.depth,
        node.direct_call_sites
    ));
    let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });


    for site in &node.call_sites {
        let trimmed = site.line_text.trim();


        let snippet = truncate_line_text(trimmed, 80);
        out.push_str(&format!(
            "{}    {}:{} - {}\n",
            child_prefix, site.line, site.column, snippet
        ));
    }


    for site in &node.call_sites {
        if site.context_lines.is_empty() {
            continue;
        }
        out.push_str(&format!("{}    [call site @ {}:{}]\n", child_prefix, site.line, site.column));
        for ctx in &site.context_lines {
            let marker = if ctx.is_match { ">" } else { " " };
            out.push_str(&format!(
                "{}    {} {:>6} | {}\n",
                child_prefix,
                marker,
                ctx.line,
                ctx.text.trim_end()
            ));
        }
    }
    let total = node.callers.len();
    for (i, c) in node.callers.iter().enumerate() {
        render_node(c, &child_prefix, i + 1 == total, out);
    }
}


fn truncate_line_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        if i >= max_chars {
            break;
        }
        out.push(c);
    }
    out.push_str("...");
    out
}

fn short_path(p: &str) -> String {
    p.trim_start_matches("./").to_string()
}


/// Reads up to `max_lines` lines from `file_path` starting at `start_line`, used to embed
/// function-body previews in ambiguity-resolution error messages. Public within the crate so
/// `ensemble::run` can reuse it.
pub(crate) fn read_body_preview_pub(root: &std::path::Path, file_path: &str, start_line: usize, max_lines: usize) -> Vec<String> {
    read_body_preview(root, file_path, start_line, max_lines)
}

fn read_body_preview(root: &std::path::Path, file_path: &str, start_line: usize, max_lines: usize) -> Vec<String> {
    let full = if std::path::Path::new(file_path).is_absolute() {
        std::path::PathBuf::from(file_path)
    } else {
        root.join(file_path)
    };
    let content = match std::fs::read_to_string(&full) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let start_idx = start_line.saturating_sub(1);
    content
        .lines()
        .skip(start_idx)
        .take(max_lines)
        .map(|l| l.trim_end().to_string())
        .collect()
}


fn collect_flat_nodes(
    nodes: &[TransitiveCallerNode],
    seen: &mut HashSet<String>,
    lines: &mut Vec<String>,
) {
    for node in nodes {
        let key = format!("{}:{}:{}", node.info.file_path, node.info.start_line, node.info.name);
        if seen.insert(key.clone()) {
            lines.push(key);
        }
        collect_flat_nodes(&node.callers, seen, lines);
    }
}


#[allow(clippy::too_many_arguments)]
fn walk_flat(
    project: &ProjectData,
    threshold: f64,
    caller: &CallerFunction,
    current_depth: usize,
    max_depth: usize,
    exclude_tests: bool,
    callers_in: Option<&str>,
    changed: Option<&ChangedRanges>,
    visited: &mut HashSet<String>,
    seen: &mut HashSet<String>,
    lines: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let id = function_id(&caller.info);
    if !visited.insert(id.clone()) {
        return Ok(());
    }
    let key = format!(
        "{}:{}:{}",
        caller.info.file_path, caller.info.start_line, caller.info.name
    );
    if seen.insert(key.clone()) {
        lines.push(key);
    }
    if current_depth < max_depth {
        let next_query = format!("{}:{}", caller.info.file_path, caller.info.start_line);
        let next_report = build_callers_report(project, &next_query, threshold, 0, 0)?;
        for m in &next_report.matches {
            if function_id(&m.info) != id {
                continue;
            }
            for c in &m.callers {
                if exclude_tests && c.info.is_test {
                    continue;
                }
                if let Some(needle) = callers_in
                    && !c.info.file_path.contains(needle)
                {
                    continue;
                }
                if let Some(ranges) = changed
                    && !fn_was_changed(&c.info.file_path, c.info.start_line, c.info.end_line, ranges)
                {
                    continue;
                }
                walk_flat(
                    project,
                    threshold,
                    c,
                    current_depth + 1,
                    max_depth,
                    exclude_tests,
                    callers_in,
                    changed,
                    visited,
                    seen,
                    lines,
                )?;
            }
        }
    }
    Ok(())
}


/// Returns a human-readable error hint when a callers/ensemble query resolves to a struct or enum
/// rather than a function, guiding the user toward `refs` or `usages` instead.
pub(crate) fn type_redirect_hint(project: &ProjectData, query: &str, threshold: f64) -> Option<String> {
    if let Some(hint) = type_redirect_for_path_line(project, query) {
        return Some(hint);
    }
    let (_, structs, enums) =
        crate::search_items(&[], &project.structs, &project.enums, query, threshold);
    if let Some(s) = structs.into_iter().next() {
        return Some(format!(
            "'{}' is a struct (found at {}:{}), not a function. Try `rustgraph refs {}` or `rustgraph usages {}` to find references.",
            query, s.file_path, s.start_line, s.name, s.name
        ));
    }
    if let Some(e) = enums.into_iter().next() {
        return Some(format!(
            "'{}' is an enum (found at {}:{}), not a function. Try `rustgraph refs {}` or `rustgraph usages {}` to find references.",
            query, e.file_path, e.start_line, e.name, e.name
        ));
    }
    None
}


fn type_redirect_for_path_line(project: &ProjectData, query: &str) -> Option<String> {
    let trimmed = query.trim();
    let (path_part, line_part) = trimmed.rsplit_once(':')?;
    let line: usize = line_part.parse().ok()?;


    let fn_encloses = project.functions.iter().any(|f| {
        f.file_path.ends_with(path_part) && f.start_line <= line && line <= f.end_line
    });
    if fn_encloses {
        return None;
    }
    if let Some(s) = project.structs.iter().find(|s| {
        s.file_path.ends_with(path_part) && s.start_line <= line && line <= s.end_line
    }) {
        return Some(format!(
            "'{}' is a struct (found at {}:{}), not a function. Try `rustgraph refs {}` or `rustgraph usages {}` to find references.",
            query, s.file_path, s.start_line, s.name, s.name
        ));
    }
    if let Some(e) = project.enums.iter().find(|e| {
        e.file_path.ends_with(path_part) && e.start_line <= line && line <= e.end_line
    }) {
        return Some(format!(
            "'{}' is an enum (found at {}:{}), not a function. Try `rustgraph refs {}` or `rustgraph usages {}` to find references.",
            query, e.file_path, e.start_line, e.name, e.name
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::project::ProjectData;
    use std::fs;
    use tempfile::tempdir;

    fn req(query: &str) -> CallersRequest {
        CallersRequest {
            query: query.to_string(),
            symbol_id: None,
            before_context: 0,
            after_context: 0,
            target_in: None,
            callers_in: None,
            depth: 1,
            all: false,
            ambiguity_cap: 7,
            max_results: None,
            flat: false,
        }
    }

    fn run_cmd(
        path: &std::path::Path,
        request: CallersRequest,
    ) -> Result<String, Box<dyn std::error::Error>> {
        use crate::cli::Args;
        use clap::Parser;
        let path_arg = path.to_string_lossy().to_string();
        let args = Args::try_parse_from(["rustgraph", "--path", &path_arg, "--json"])?;
        let project = ProjectData::load(&args.path, args.include_ignored);
        let captured_path = path.join("__callers_out.txt");
        let mut owned_args = args;
        owned_args.output = Some(captured_path.clone());
        run(&owned_args, &project, request, None)?;
        Ok(fs::read_to_string(&captured_path)?)
    }

    #[test]
    fn callers_in_path_filter_keeps_matching_files_only() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/keep")).unwrap();
        fs::create_dir_all(dir.path().join("src/drop")).unwrap();
        fs::write(
            dir.path().join("src/keep/a.rs"),
            "pub fn helper() {}\nfn caller_keep() { helper(); }\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/drop/b.rs"),
            "pub fn helper() {}\nfn caller_drop() { helper(); }\n",
        )
        .unwrap();
        let mut r = req("helper");
        r.target_in = Some("keep".to_string());
        let out = run_cmd(dir.path(), r).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let m = v["matches"].as_array().unwrap();
        assert_eq!(m.len(), 1, "got: {}", out);
        assert!(m[0]["info"]["file_path"].as_str().unwrap().contains("keep"));
    }

    #[test]
    fn callers_depth_2_walks_one_extra_hop() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "fn target() {}\nfn lvl1() { target(); }\nfn lvl2() { lvl1(); }\n",
        )
        .unwrap();
        let mut r = req("target");
        r.depth = 2;
        let out = run_cmd(dir.path(), r).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let matches = v["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1);
        let lvl1_callers = matches[0]["callers"].as_array().unwrap();
        assert_eq!(lvl1_callers.len(), 1);
        assert_eq!(lvl1_callers[0]["info"]["name"].as_str().unwrap(), "lvl1");
        assert_eq!(lvl1_callers[0]["depth"].as_u64().unwrap(), 1);
        let lvl2_callers = lvl1_callers[0]["callers"].as_array().unwrap();
        assert_eq!(lvl2_callers.len(), 1);
        assert_eq!(lvl2_callers[0]["info"]["name"].as_str().unwrap(), "lvl2");
        assert_eq!(lvl2_callers[0]["depth"].as_u64().unwrap(), 2);
    }

    #[test]
    fn callers_depth_handles_simple_cycle_without_loop() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "fn target() { other(); }\nfn other() { target(); }\n",
        )
        .unwrap();
        let mut r = req("target");
        r.depth = 5;
        let out = run_cmd(dir.path(), r).unwrap();

        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(!v["matches"].as_array().unwrap().is_empty());
    }
}
