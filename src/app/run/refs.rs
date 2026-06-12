use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::Serialize;
use syn::spanned::Spanned;
use syn::visit::Visit;

use super::super::changed::{ChangedRanges, fn_was_changed};
use super::super::modes::RefsRequest;
use super::super::project::ProjectData;
use crate::FunctionInfo;
use crate::cli::Args;

/// Priority order used when two hits land on the same `file:line:col` — the
/// most specific classification survives the location dedup.
fn kind_priority(k: &str) -> u8 {
    match k {
        "call" => 0,
        "method" => 1,
        "variant" => 2,
        "assoc_call" => 3,
        "struct" => 4,
        "pattern" => 5,
        "type" => 6,
        "field" => 7,
        "path" => 8,
        "macro" => 9,
        _ => 10,
    }
}

/// Shared collection pipeline for `refs` and `usages`: walks every parsed file and records
/// each AST-level reference to `request.ident`, descending into macro bodies (`vec![...]`,
/// `assert_eq!(...)`) by re-parsing their token streams. Annotates each hit with its
/// innermost enclosing function, applies `--exclude-tests`, then sorts and dedups by
/// location keeping the highest-priority kind tag.
pub(crate) fn collect_hits(
    args: &Args,
    project: &ProjectData,
    request: &RefsRequest,
) -> Vec<RefHit> {
    let allowed_kinds: Option<HashSet<&str>> = if request.kind.is_empty() {
        None
    } else {
        Some(request.kind.iter().map(|s| s.as_str()).collect())
    };

    let by_file: HashMap<String, Vec<&FunctionInfo>> = {
        let mut m: HashMap<String, Vec<&FunctionInfo>> = HashMap::new();
        for f in &project.functions {
            m.entry(f.file_path.clone()).or_default().push(f);
        }
        m
    };


    let project_enum_names: HashSet<String> =
        project.enums.iter().map(|e| e.name.clone()).collect();
    let project_struct_names: HashSet<String> =
        project.structs.iter().map(|s| s.name.clone()).collect();

    let mut hits: Vec<RefHit> = Vec::new();

    for file in &project.rust_files {
        let abs = crate::normalize_path_separators(&file.to_string_lossy());
        let file_str = if args.absolute_paths {
            abs.clone()
        } else {
            super::super::relativize_for_display(&abs, &args.path, &args.also)
        };
        if let Some(needle) = &request.in_path {
            if !file_str.contains(needle) {
                continue;
            }
        }
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let Some(syntax) = project.parsed_file(file) else {
            continue;
        };
        let mut v = RefsVisitor {
            file_path: file_str.clone(),
            ident: &request.ident,
            allowed: allowed_kinds.as_ref(),
            hits: &mut hits,
            content_lines: content.lines().map(|s| s.to_string()).collect(),
            project_enum_names: &project_enum_names,
            project_struct_names: &project_struct_names,
        };
        v.visit_file(syntax);
    }

    let mut hit_is_test: Vec<bool> = vec![false; hits.len()];
    for (idx, hit) in hits.iter_mut().enumerate() {
        if let Some(funcs) = by_file.get(&hit.file_path) {
            if let Some(f) = innermost_enclosing(funcs, hit.line) {
                hit.enclosing_function = Some(f.name.clone());
                hit.enclosing_function_start = Some(f.start_line);
                hit.enclosing_function_end = Some(f.end_line);
                hit.enclosing_function_is_test = f.is_test;
                hit_is_test[idx] = f.is_test;
            }
        }
    }
    if args.exclude_tests {
        let mut iter = hit_is_test.into_iter();
        hits.retain(|_| !iter.next().unwrap_or(false));
    }

    hits.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
            .then(kind_priority(a.kind).cmp(&kind_priority(b.kind)))
    });
    hits.dedup_by(|a, b| {
        a.file_path == b.file_path && a.line == b.line && a.col == b.col
    });
    hits
}

/// Implements `rustgraph refs <ident>` — walks every Rust source file via `syn` and records each
/// AST-level reference to the given identifier.
///
/// Classifies each hit as one of: `call`, `method`, `field`, `type`, `struct`, `pattern`,
/// `path`, `variant`, `assoc_call`, or `macro` (best-effort ident hit inside a macro body
/// that doesn't re-parse as exprs/stmts). Supports `--kind` filtering, `--in` scoping,
/// `--by-function` grouping, `--exclude-tests`, `--changed`, and `--max-results` capping.
/// Emits actionable hints when zero references are found.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: RefsRequest,
    changed: Option<&ChangedRanges>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut hits = collect_hits(args, project, &request);

    if hits.is_empty() {
        let project_enum_names: HashSet<String> =
            project.enums.iter().map(|e| e.name.clone()).collect();
        let project_struct_names: HashSet<String> =
            project.structs.iter().map(|s| s.name.clone()).collect();


        let mut notes: Vec<String> = Vec::new();
        notes.push(format!(
            "no references found for ident '{}' (kinds={:?}); check spelling or kind filter",
            request.ident, request.kind
        ));


        if request.kind.is_empty()
            && request
                .ident
                .chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false)
            && !project_struct_names.contains(&request.ident)
            && !project_enum_names.contains(&request.ident)
        {
            let needle = format!("derive(");
            let mut derive_count: usize = 0;
            for file in &project.rust_files {
                let content = match fs::read_to_string(file) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let mut start = 0usize;
                while let Some(pos) = content[start..].find(&needle) {
                    let abs = start + pos + needle.len();
                    let tail = &content[abs..];
                    let close = tail.find(')').unwrap_or(tail.len().min(200));
                    let inner = &tail[..close];
                    if inner.split(',').any(|t| t.trim() == request.ident) {
                        derive_count += 1;
                    }
                    start = abs;
                }
            }
            if derive_count > 0 {
                notes.push(format!(
                    "note: '{}' appears in #[derive(...)] {} time(s) — looks like a trait. For trait implementations (counted): impls {}",
                    request.ident, derive_count, request.ident
                ));
            }
        }


        let kinds_set: HashSet<&str> = request.kind.iter().map(|s| s.as_str()).collect();
        let want_assoc = kinds_set.contains("assoc_call");
        let want_variant = kinds_set.contains("variant");
        let want_field = kinds_set.contains("field");


        if want_field && project_struct_names.contains(&request.ident) {
            notes.push(format!(
                "note: 0 refs for '{}' with --kind field. Note: --kind field requires the IDENT to be the field name (not the owner type). For per-field access on a Type, try: members {}",
                request.ident, request.ident
            ));
        }

        if want_assoc || want_variant {
            let mut suggestions: Vec<(String, &'static str)> = Vec::new();

            if want_variant {

                for enum_info in &project.enums {
                    if enum_info.variants.iter().any(|v| v.name == request.ident) {
                        suggestions.push((enum_info.name.clone(), "variant"));
                    }
                }
            }

            if want_assoc {


                let needle = format!("::{}", request.ident);
                let mut found_types: HashSet<String> = HashSet::new();
                let known: HashSet<&str> = project
                    .structs
                    .iter()
                    .map(|s| s.name.as_str())
                    .chain(project.enums.iter().map(|e| e.name.as_str()))
                    .collect();
                'outer: for file in &project.rust_files {
                    let content = match fs::read_to_string(file) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    let mut start = 0usize;
                    while let Some(pos) = content[start..].find(&needle) {
                        let abs = start + pos;

                        let prefix = &content[..abs];
                        let qual: String = prefix
                            .chars()
                            .rev()
                            .take_while(|c| c.is_alphanumeric() || *c == '_')
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();
                        if !qual.is_empty() && known.contains(qual.as_str()) {


                            let end = abs + needle.len();
                            let next_ok = content[end..]
                                .chars()
                                .next()
                                .map(|c| !c.is_alphanumeric() && c != '_')
                                .unwrap_or(true);
                            if next_ok {
                                found_types.insert(qual);
                                if found_types.len() >= 3 {
                                    break 'outer;
                                }
                            }
                        }
                        start = abs + needle.len();
                    }
                }
                for ty in found_types {
                    suggestions.push((ty, "assoc_call"));
                }
            }

            if !suggestions.is_empty() {

                let mut seen: HashSet<(String, &'static str)> = HashSet::new();
                let suggestions: Vec<_> = suggestions
                    .into_iter()
                    .filter(|s| seen.insert((s.0.clone(), s.1)))
                    .collect();
                let kind_label = if want_assoc && want_variant {
                    "assoc_call/variant"
                } else if want_assoc {
                    "assoc_call"
                } else {
                    "variant"
                };
                if suggestions.len() == 1 {
                    notes.push(format!(
                        "note: 0 refs for '{}' with --kind {}. Did you mean: refs {} --kind {} (since {}::{} exists)?",
                        request.ident, kind_label, suggestions[0].0, suggestions[0].1,
                        suggestions[0].0, request.ident
                    ));
                } else {
                    notes.push(format!(
                        "note: 0 refs for '{}' with --kind {}. Try one of:",
                        request.ident, kind_label
                    ));
                    for (ty, kind) in suggestions.iter().take(3) {
                        notes.push(format!("  refs {} --kind {}", ty, kind));
                    }
                }
            }
        }


        for n in &notes {
            eprintln!("{}", n);
        }
        if !args.json {
            for n in &notes {
                println!("{}", n);
            }
        }
    }


    if let Some(ranges) = changed {
        hits.retain(|h| match (h.enclosing_function_start, h.enclosing_function_end) {
            (Some(start), Some(end)) => fn_was_changed(&h.file_path, start, end, ranges),
            _ => false,
        });
    }


    let total_hits = hits.len();
    let cap = if request.max_results == 0 {
        usize::MAX
    } else {
        request.max_results
    };
    let truncated = total_hits > cap;
    let display_hits: Vec<RefHit> = if truncated {
        hits.iter().take(cap).cloned().collect()
    } else {
        hits.clone()
    };

    if args.json {
        if request.by_function {
            #[derive(Serialize)]
            struct ByFnGroup<'a> {
                enclosing_function: Option<&'a str>,
                enclosing_function_start: Option<usize>,


                is_test: bool,
                count: usize,
                hits: Vec<&'a RefHit>,
            }
            let mut groups: HashMap<Option<String>, Vec<&RefHit>> = HashMap::new();
            for h in &display_hits {
                groups
                    .entry(h.enclosing_function.clone())
                    .or_default()
                    .push(h);
            }
            let mut out: Vec<ByFnGroup> = groups
                .iter()
                .map(|(name, hs)| ByFnGroup {
                    enclosing_function: name.as_deref(),
                    enclosing_function_start: hs.first().and_then(|h| h.enclosing_function_start),
                    is_test: hs.iter().any(|h| h.enclosing_function_is_test),
                    count: hs.len(),
                    hits: hs.clone(),
                })
                .collect();
            out.sort_by(|a, b| b.count.cmp(&a.count));
            #[derive(Serialize)]
            struct Wrap<'a> {
                ident: &'a str,
                total_refs: usize,
                truncated: bool,
                groups: Vec<ByFnGroup<'a>>,
            }
            let payload = serde_json::to_string_pretty(&Wrap {
                ident: &request.ident,
                total_refs: total_hits,
                truncated,
                groups: out,
            })?;
            super::switchboard::write_string_output(args.output.as_deref(), &payload)?;
        } else {
            #[derive(Serialize)]
            struct Wrap<'a> {
                ident: &'a str,
                total_refs: usize,
                truncated: bool,
                refs: &'a [RefHit],
            }
            let payload = serde_json::to_string_pretty(&Wrap {
                ident: &request.ident,
                total_refs: total_hits,
                truncated,
                refs: &display_hits,
            })?;
            super::switchboard::write_string_output(args.output.as_deref(), &payload)?;
        }
    } else {


        let scoped_files: usize = match &request.in_path {
            Some(needle) => project
                .rust_files
                .iter()
                .filter(|p| p.to_string_lossy().contains(needle.as_str()))
                .count(),
            None => project.rust_files.len(),
        };
        let mut out = format!(
            "rustgraph refs '{}' across {} file(s): {} reference(s)\n",
            request.ident,
            scoped_files,
            total_hits
        );
        if request.by_function {
            let mut groups: HashMap<Option<String>, Vec<&RefHit>> = HashMap::new();
            for h in &display_hits {
                groups.entry(h.enclosing_function.clone()).or_default().push(h);
            }
            let mut entries: Vec<(Option<String>, Vec<&RefHit>)> = groups.into_iter().collect();
            entries.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            for (name, hs) in entries {
                let label = name.as_deref().unwrap_or("<top-level>");


                let test_tag = if hs.iter().any(|h| h.enclosing_function_is_test) {
                    " [test]"
                } else {
                    ""
                };
                out.push_str(&format!("\n{}{} — {} ref(s)\n", label, test_tag, hs.len()));
                for h in hs {
                    out.push_str(&format!(
                        "  {}:{}:{} [{}]\n",
                        relative(&h.file_path, &args.path),
                        h.line,
                        h.col,
                        h.kind
                    ));
                }
            }
        } else {
            for h in &display_hits {
                let enc = h
                    .enclosing_function
                    .as_deref()
                    .map(|n| format!("    [fn: {}]", n))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "{}:{}:{} [{}] {}{}\n",
                    relative(&h.file_path, &args.path),
                    h.line,
                    h.col,
                    h.kind,
                    h.line_text.trim(),
                    enc
                ));
            }
        }
        if truncated {
            out.push_str(&format!(
                "(showing {} of {}; use --max-results to expand)\n",
                cap, total_hits
            ));
        }
        super::switchboard::write_string_output(args.output.as_deref(), out.trim_end())?;
    }
    Ok(())
}

/// One resolved reference to the queried identifier: its location, kind tag, source line text,
/// and the enclosing function (if any).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RefHit {
    pub(crate) file_path: String,
    pub(crate) line: usize,
    pub(crate) col: usize,
    pub(crate) kind: &'static str,
    pub(crate) line_text: String,
    pub(crate) enclosing_function: Option<String>,
    pub(crate) enclosing_function_start: Option<usize>,
    pub(crate) enclosing_function_end: Option<usize>,


    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) enclosing_function_is_test: bool,
}

/// `syn` AST visitor that walks a single parsed file and appends `RefHit` entries for every node
/// whose identifier matches the query ident, filtered by the optional kind allowlist.
struct RefsVisitor<'a> {
    file_path: String,
    ident: &'a str,
    allowed: Option<&'a HashSet<&'a str>>,
    hits: &'a mut Vec<RefHit>,
    content_lines: Vec<String>,


    project_enum_names: &'a HashSet<String>,


    project_struct_names: &'a HashSet<String>,
}

impl<'a> RefsVisitor<'a> {
    fn record(&mut self, span: proc_macro2::Span, kind: &'static str) {
        if let Some(allowed) = self.allowed
            && !allowed.contains(kind)
        {
            return;
        }
        let loc = span.start();
        let line_text = self
            .content_lines
            .get(loc.line.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        self.hits.push(RefHit {
            file_path: self.file_path.clone(),
            line: loc.line,
            col: loc.column,
            kind,
            line_text,
            enclosing_function: None,
            enclosing_function_start: None,
            enclosing_function_end: None,
            enclosing_function_is_test: false,
        });
    }

    fn matches_path(&self, path: &syn::Path) -> bool {
        path.segments
            .last()
            .map(|s| s.ident == self.ident)
            .unwrap_or(false)
    }


    fn matches_as_qualifier(&self, path: &syn::Path) -> bool {
        let segs = &path.segments;
        if segs.len() < 2 {
            return false;
        }
        segs[segs.len() - 2].ident == self.ident
    }


    fn classify_qualifier(&self, path: &syn::Path) -> &'static str {
        let parent_ident = match path.segments.iter().rev().nth(1) {
            Some(seg) => seg.ident.to_string(),
            None => return "path",
        };
        if self.project_enum_names.contains(&parent_ident) {
            "variant"
        } else if self.project_struct_names.contains(&parent_ident) {
            "assoc_call"
        } else {
            "path"
        }
    }


    /// Last-resort macro coverage: when a macro body parses as neither an expr list nor a
    /// statement list, walk its raw token stream and record bare ident matches as kind
    /// `macro` — exact line, approximate classification. Never silently misses.
    fn scan_macro_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        for tt in tokens {
            match tt {
                proc_macro2::TokenTree::Ident(ident) => {
                    if ident == self.ident {
                        self.record(ident.span(), "macro");
                    }
                }
                proc_macro2::TokenTree::Group(group) => self.scan_macro_tokens(group.stream()),
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for RefsVisitor<'_> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*node.func
            && self.matches_path(&path.path)
        {
            self.record(path.span(), "call");
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == self.ident {
            self.record(node.method.span(), "method");
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
        if let syn::Member::Named(name) = &node.member
            && name == self.ident
        {
            self.record(name.span(), "field");
        }
        syn::visit::visit_expr_field(self, node);
    }

    fn visit_pat_struct(&mut self, node: &'ast syn::PatStruct) {
        if self.matches_path(&node.path) {
            self.record(node.path.span(), "pattern");
        } else if self.matches_as_qualifier(&node.path) {
            self.record(node.path.span(), self.classify_qualifier(&node.path));
        }
        syn::visit::visit_pat_struct(self, node);
    }

    fn visit_pat_tuple_struct(&mut self, node: &'ast syn::PatTupleStruct) {
        if self.matches_path(&node.path) {
            self.record(node.path.span(), "pattern");
        } else if self.matches_as_qualifier(&node.path) {
            self.record(node.path.span(), self.classify_qualifier(&node.path));
        }
        syn::visit::visit_pat_tuple_struct(self, node);
    }

    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        if self.matches_path(&node.path) {
            self.record(node.path.span(), "type");
        }
        syn::visit::visit_type_path(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if self.matches_path(&node.path) {
            self.record(node.path.span(), "path");
        } else if self.matches_as_qualifier(&node.path) {
            self.record(node.path.span(), self.classify_qualifier(&node.path));
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        if self.matches_path(&node.path) {
            self.record(node.path.span(), "struct");
        } else if self.matches_as_qualifier(&node.path) {


            self.record(node.path.span(), self.classify_qualifier(&node.path));
        }
        syn::visit::visit_expr_struct(self, node);
    }

    // syn does not descend into macro token streams, so `vec![SellOrder { .. }]` and
    // `assert_eq!(o.field, 1)` were invisible. Re-parse the body (spans survive, so
    // line numbers stay exact): expr list first, then statement list, then a raw
    // token scan tagged `macro` so a reference inside a macro is NEVER silently lost.
    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        use syn::punctuated::Punctuated;
        if let Ok(exprs) = node
            .parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        {
            for e in &exprs {
                Visit::visit_expr(self, e);
            }
        } else if let Ok(stmts) = node.parse_body_with(syn::Block::parse_within) {
            for s in &stmts {
                Visit::visit_stmt(self, s);
            }
        } else {
            self.scan_macro_tokens(node.tokens.clone());
        }
        syn::visit::visit_macro(self, node);
    }
}

fn innermost_enclosing<'a>(fns: &'a [&FunctionInfo], line: usize) -> Option<&'a FunctionInfo> {
    let mut best: Option<&FunctionInfo> = None;
    for f in fns {
        if f.start_line <= line && line <= f.end_line {
            match best {
                Some(b) if b.start_line >= f.start_line => {}
                _ => best = Some(f),
            }
        }
    }
    best
}

fn relative(file_path: &str, project_root: &Path) -> String {
    let path = Path::new(file_path);
    if let Ok(stripped) = path.strip_prefix(project_root) {
        crate::normalize_path_separators(&stripped.to_string_lossy())
    } else {
        crate::normalize_path_separators(file_path.trim_start_matches("./"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn run_cmd(
        path: &Path,
        request: RefsRequest,
    ) -> Result<String, Box<dyn std::error::Error>> {
        use crate::cli::Args;
        use clap::Parser;
        let path_arg = path.to_string_lossy().to_string();
        let args = Args::try_parse_from(["rustgraph", "--path", &path_arg, "--json"])?;
        let project = ProjectData::load(&args.path, args.include_ignored);
        let captured = path.join("__refs_out.txt");
        let mut owned = args;
        owned.output = Some(captured.clone());
        run(&owned, &project, request, None)?;
        Ok(fs::read_to_string(&captured)?)
    }

    fn req(ident: &str) -> RefsRequest {
        RefsRequest {
            ident: ident.to_string(),
            kind: vec![],
            in_path: None,
            by_function: false,
            max_results: 0,
        }
    }

    #[test]
    fn refs_finds_type_field_struct_pattern_call() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            r#"
struct Foo { x: u32 }
fn make_foo() -> Foo { Foo { x: 1 } }
fn use_foo(f: Foo) -> u32 {
    let Foo { x } = f;
    f.x
}
fn caller() { let _ = make_foo(); }
"#,
        )
        .unwrap();
        let out = run_cmd(dir.path(), req("Foo")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let kinds: Vec<&str> = v["refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["kind"].as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"type"), "got: {:?}", kinds);
        assert!(kinds.contains(&"struct"), "got: {:?}", kinds);
        assert!(kinds.contains(&"pattern"), "got: {:?}", kinds);
    }

    #[test]
    fn refs_kind_filter_narrows() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "struct Bar; fn t(x: Bar) -> Bar { x } fn make() -> Bar { Bar }\n",
        )
        .unwrap();
        let mut r = req("Bar");
        r.kind = vec!["type".into()];
        let out = run_cmd(dir.path(), r).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let kinds: Vec<&str> = v["refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["kind"].as_str().unwrap())
            .collect();
        assert!(kinds.iter().all(|k| *k == "type"), "got: {:?}", kinds);
    }

    #[test]
    fn refs_method_kind_finds_method_call() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "fn t(s: String) -> String { s.clone() }\n",
        )
        .unwrap();
        let out = run_cmd(dir.path(), req("clone")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let kinds: Vec<&str> = v["refs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["kind"].as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"method"), "got: {:?}", kinds);
    }
}
