use serde::Serialize;

use super::super::modes::{CallersRequest, RefsRequest, UsagesRequest};
use super::super::project::ProjectData;
use crate::cli::Args;


/// Run `rustgraph usages <NAME> [--in <PATH>] [--max-results N]`.
///
/// Combines callers (call-site tracing via the AST call graph) and refs (identifier references
/// via AST visitor) into a single deduplicated report, grouped into labelled sections.
/// Caller lines are excluded from the refs section to avoid double-counting.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: UsagesRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let has_fn = project.functions.iter().any(|f| f.name == request.name);

    let mut sections: Vec<Section> = Vec::new();


    if !has_fn {
        sections.push(Section {
            label: "callers (skipped — not a function)".into(),
            count: 0,
            entries: Vec::new(),
        });
    }

    if has_fn {
        let callers_req = CallersRequest {
            query: request.name.clone(),
            symbol_id: None,
            before_context: 0,
            after_context: 0,
            target_in: request.in_path.clone(),
            callers_in: None,
            depth: 1,


            all: true,
            ambiguity_cap: usize::MAX,
            max_results: None,
            flat: false,
        };
        let report =
            crate::app::callers::build_callers_report_with_options(
                project,
                &callers_req.query,
                args.search_threshold,
                0,
                0,
                args.match_signature,
                Some(&args.path),
                &args.also,
            )?;
        let mut entries: Vec<UsageEntry> = Vec::new();
        for m in &report.matches {
            if let Some(needle) = &request.in_path {
                if !m.info.file_path.contains(needle) {
                    continue;
                }
            }
            for c in &m.callers {

                if args.exclude_tests && c.info.is_test {
                    continue;
                }
                for site in &c.call_sites {
                    entries.push(UsageEntry {
                        kind: "call".into(),
                        file_path: c.info.file_path.clone(),
                        line: site.line,
                        enclosing: Some(c.info.name.clone()),
                        target: Some(format!(
                            "{} ({}:{})",
                            m.info.name, m.info.file_path, m.info.start_line
                        )),
                    });
                }
            }
        }
        sections.push(Section {
            label: "callers".into(),
            count: entries.len(),
            entries,
        });
    }


    let refs_req = RefsRequest {
        ident: request.name.clone(),
        kind: Vec::new(),
        in_path: request.in_path.clone(),
        by_function: false,

        max_results: 0,
    };
    let ref_hits = collect_refs(args, project, &refs_req);


    use std::collections::HashSet;
    let callers_sites: HashSet<(String, usize)> = sections
        .iter()
        .filter(|s| s.label == "callers")
        .flat_map(|s| s.entries.iter().map(|e| (e.file_path.clone(), e.line)))
        .collect();
    let ref_entries: Vec<UsageEntry> = ref_hits
        .into_iter()
        .filter(|h| !callers_sites.contains(&(h.file_path.clone(), h.line)))
        .map(|h| UsageEntry {
            kind: h.kind,
            file_path: h.file_path,
            line: h.line,
            enclosing: h.enclosing,
            target: None,
        })
        .collect();
    let refs_label = if has_fn {
        "refs (excluding lines covered by callers section)".to_string()
    } else {
        "refs".to_string()
    };
    sections.push(Section {
        label: refs_label,
        count: ref_entries.len(),
        entries: ref_entries,
    });

    let total: usize = sections.iter().map(|s| s.count).sum();
    if total == 0 {
        eprintln!(
            "no usages found for '{}' (no callers, no refs). Check spelling or pass --in <PATH> to widen scope.",
            request.name
        );
    }


    let cap = if request.max_results == 0 {
        usize::MAX
    } else {
        request.max_results
    };
    let truncated = total > cap;
    if truncated {
        let mut remaining = cap;
        for s in sections.iter_mut() {
            let take = remaining.min(s.entries.len());
            s.entries.truncate(take);
            remaining = remaining.saturating_sub(take);
        }
    }

    let report = UsagesReport {
        name: request.name.clone(),
        total,
        truncated,
        sections,
    };

    if args.json {
        let payload = serde_json::to_string_pretty(&report)?;
        super::switchboard::write_string_output(args.output.as_deref(), &payload)?;
    } else {
        let mut text = render_text(&report);
        if truncated {
            text.push_str(&format!(
                "\n(showing {} of {}; use --max-results to expand)\n",
                cap, total
            ));
        }
        super::switchboard::write_string_output(args.output.as_deref(), &text)?;
    }
    Ok(())
}

#[derive(Serialize)]
struct UsageEntry {
    kind: String,
    file_path: String,
    line: usize,
    enclosing: Option<String>,
    target: Option<String>,
}

#[derive(Serialize)]
struct Section {
    label: String,
    count: usize,
    entries: Vec<UsageEntry>,
}

#[derive(Serialize)]
struct UsagesReport {
    name: String,
    total: usize,


    #[serde(skip_serializing_if = "std::ops::Not::not")]
    truncated: bool,
    sections: Vec<Section>,
}

fn render_text(report: &UsagesReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "rustgraph usages '{}'  [{} total across {} section(s)]\n",
        report.name,
        report.total,
        report.sections.len()
    ));
    for s in &report.sections {
        out.push_str(&format!("\n=== via {} ({} hit(s)) ===\n", s.label, s.count));
        for e in &s.entries {
            let enc = e
                .enclosing
                .as_deref()
                .map(|n| format!("  [in {}]", n))
                .unwrap_or_default();
            let tgt = e
                .target
                .as_deref()
                .map(|t| format!("  → {}", t))
                .unwrap_or_default();
            out.push_str(&format!(
                "  {} {}:{}{}{}\n",
                e.kind, e.file_path, e.line, enc, tgt
            ));
        }
    }
    out
}

struct CollectedRef {
    kind: String,
    file_path: String,
    line: usize,
    enclosing: Option<String>,
}


fn collect_refs(args: &Args, project: &ProjectData, request: &RefsRequest) -> Vec<CollectedRef> {
    use std::collections::HashMap;
    use syn::visit::Visit;

    let by_file: HashMap<String, Vec<&crate::FunctionInfo>> = {
        let mut m: HashMap<String, Vec<&crate::FunctionInfo>> = HashMap::new();
        for f in &project.functions {
            m.entry(f.file_path.clone()).or_default().push(f);
        }
        m
    };

    let mut hits: Vec<CollectedRef> = Vec::new();
    for file in &project.rust_files {
        let abs = file.to_string_lossy().to_string();
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
        let Some(syntax) = project.parsed_file(file) else {
            continue;
        };
        let mut v = LiteRefsVisitor {
            file_path: file_str.clone(),
            ident: &request.ident,
            hits: &mut hits,
        };
        v.visit_file(syntax);
    }

    let mut hit_is_test = vec![false; hits.len()];
    for (idx, hit) in hits.iter_mut().enumerate() {
        if let Some(funcs) = by_file.get(&hit.file_path) {
            if let Some(f) = innermost_enclosing(funcs, hit.line) {
                hit.enclosing = Some(f.name.clone());
                hit_is_test[idx] = f.is_test;
            }
        }
    }
    if args.exclude_tests {


        let mut iter = hit_is_test.into_iter();
        hits.retain(|_| !iter.next().unwrap_or(false));
    }
    hits
}

struct LiteRefsVisitor<'a> {
    file_path: String,
    ident: &'a str,
    hits: &'a mut Vec<CollectedRef>,
}

impl<'a> LiteRefsVisitor<'a> {
    fn push(&mut self, kind: &str, line: usize) {
        self.hits.push(CollectedRef {
            kind: kind.to_string(),
            file_path: self.file_path.clone(),
            line,
            enclosing: None,
        });
    }
}

impl<'ast> syn::visit::Visit<'ast> for LiteRefsVisitor<'_> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        use syn::spanned::Spanned;
        if let syn::Expr::Path(p) = &*node.func {
            if let Some(seg) = p.path.segments.last() {
                if seg.ident == self.ident {
                    self.push("call", node.span().start().line);
                }
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        use syn::spanned::Spanned;
        if node.method == self.ident {
            self.push("method", node.span().start().line);
        }
        syn::visit::visit_expr_method_call(self, node);
    }
    fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
        use syn::spanned::Spanned;
        if let syn::Member::Named(name) = &node.member {
            if name == self.ident {
                self.push("field", node.span().start().line);
            }
        }
        syn::visit::visit_expr_field(self, node);
    }
    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        use syn::spanned::Spanned;
        if let Some(seg) = node.path.segments.last() {
            if seg.ident == self.ident {
                self.push("type", node.span().start().line);
            }
        }
        syn::visit::visit_type_path(self, node);
    }
    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        use syn::spanned::Spanned;
        if let Some(seg) = node.path.segments.last() {
            if seg.ident == self.ident {
                self.push("struct", node.span().start().line);
            }
        }
        syn::visit::visit_expr_struct(self, node);
    }
}

fn innermost_enclosing<'a>(
    funcs: &'a [&crate::FunctionInfo],
    line: usize,
) -> Option<&'a crate::FunctionInfo> {
    funcs
        .iter()
        .filter(|f| f.start_line <= line && line <= f.end_line)
        .min_by_key(|f| f.end_line - f.start_line)
        .copied()
}
