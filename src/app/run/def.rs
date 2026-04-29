use std::collections::HashSet;

use syn::visit::Visit;

use super::super::modes::DefRequest;
use super::super::project::ProjectData;
use crate::cli::Args;
use crate::format_cfg_annotation;

use super::switchboard::write_string_output;


/// Run `rustgraph def <NAME> [--func|--struct|--enum] [--in <PATH>]`.
///
/// Performs an exact, case-sensitive go-to-definition lookup for a top-level symbol.
/// Returns an error if zero matches are found, or if more than one match exists and the
/// duplicates are not distinguishable by cfg attributes (cross-platform alternates).
/// When exactly one match is found it emits `file:line:name [kind]` or JSON.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: DefRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let want_fns = !request.struct_only && !request.enum_only;
    let want_structs = !request.func_only && !request.enum_only;
    let want_enums = !request.func_only && !request.struct_only;


    let mut hits: Vec<(String, String, usize, Vec<String>)> = Vec::new();
    if want_fns {
        for f in &project.functions {
            if f.name == request.name {
                hits.push((
                    "fn".to_string(),
                    f.file_path.clone(),
                    f.start_line,
                    f.cfg_attrs.clone(),
                ));
            }
        }
    }
    if want_structs {
        for s in &project.structs {
            if s.name == request.name {
                hits.push((
                    "struct".to_string(),
                    s.file_path.clone(),
                    s.start_line,
                    s.cfg_attrs.clone(),
                ));
            }
        }
    }
    if want_enums {
        for e in &project.enums {
            if e.name == request.name {
                hits.push((
                    "enum".to_string(),
                    e.file_path.clone(),
                    e.start_line,
                    e.cfg_attrs.clone(),
                ));
            }
        }
    }


    if let Some(needle) = &request.in_path {
        let pre_filter = hits.len();
        hits.retain(|(_, file, _, _)| file.contains(needle.as_str()));
        if hits.is_empty() && pre_filter > 0 {
            return Err(format!(
                "no match for '{}' in files containing '{}' (had {} candidate(s) before --in filter)",
                request.name, needle, pre_filter
            )
            .into());
        }
    }

    if hits.is_empty() {


        let usage = scan_name_usage(project, &request.name);
        if usage.total > 0 {
            let mut msg = format!(
                "no top-level symbol named '{}' (case-sensitive)\n",
                request.name
            );


            let mut classifications: Vec<&'static str> = Vec::new();
            if usage.is_field {
                classifications.push("struct field");
            }
            if usage.is_variant {
                classifications.push("enum variant");
            }
            if !classifications.is_empty() {
                msg.push_str(&format!(
                    "hint: '{}' appears as {} ({} ref(s) found). ",
                    request.name,
                    classifications.join(" / "),
                    usage.total
                ));
            } else {
                msg.push_str(&format!(
                    "hint: '{}' appears as a method/path identifier ({} ref(s) found). ",
                    request.name, usage.total
                ));
            }
            msg.push_str(&format!(
                "Try `refs {}` to see all uses, or `find {}` for fuzzy lookup.",
                request.name, request.name
            ));
            return Err(msg.into());
        }
        return Err(format!(
            "no exact match for '{}' (case-sensitive). Try `rustgraph find {}` for fuzzy lookup.",
            request.name, request.name
        )
        .into());
    }
    if hits.len() > 1 {


        if cfg_distinguishes_all_hits(&hits) {
            let n = hits.len();
            eprintln!(
                "note: {} matches differ only by cfg gate (likely cross-platform alternates); listed all",
                n
            );
            if args.json {
                let mut entries = String::new();
                for (i, (kind, file, line, cfg_attrs)) in hits.iter().enumerate() {
                    if i > 0 {
                        entries.push_str(",\n");
                    }
                    let cfg_array = if cfg_attrs.is_empty() {
                        "[]".to_string()
                    } else {
                        let parts: Vec<String> = cfg_attrs
                            .iter()
                            .map(|p| format!("\"{}\"", p.replace('\\', "\\\\").replace('"', "\\\"")))
                            .collect();
                        format!("[{}]", parts.join(", "))
                    };
                    entries.push_str(&format!(
                        "    {{\n      \"name\": \"{}\",\n      \"kind\": \"{}\",\n      \"file_path\": \"{}\",\n      \"start_line\": {},\n      \"cfg_attrs\": {}\n    }}",
                        request.name, kind, file, line, cfg_array
                    ));
                }
                let payload = format!(
                    "{{\n  \"name\": \"{}\",\n  \"cfg_alternates\": true,\n  \"matches\": [\n{}\n  ]\n}}",
                    request.name, entries
                );
                write_string_output(args.output.as_deref(), &payload)?;
            } else {
                let mut payload = String::new();
                for (kind, file, line, cfg_attrs) in &hits {
                    let cfg_suffix = format_cfg_annotation(cfg_attrs)
                        .map(|s| format!(" {}", s))
                        .unwrap_or_default();
                    payload.push_str(&format!(
                        "{}:{}:{} [{}]{}\n",
                        file, line, request.name, kind, cfg_suffix
                    ));
                }
                write_string_output(args.output.as_deref(), payload.trim_end_matches('\n'))?;
            }
            return Ok(());
        }


        let kinds: HashSet<&str> = hits.iter().map(|(k, _, _, _)| k.as_str()).collect();
        let single_kind = kinds.len() == 1;
        let mut msg = if single_kind {


            let kind_label = hits[0].0.as_str();
            format!(
                "'{}' is ambiguous ({} {} candidates)\n",
                request.name,
                hits.len(),
                kind_label
            )
        } else {
            format!(
                "ambiguous: '{}' matches {} symbols. Use --func/--struct/--enum to narrow, or `slice` with one of:\n",
                request.name,
                hits.len()
            )
        };
        for (kind, file, line, cfg_attrs) in &hits {
            let cfg_suffix = format_cfg_annotation(cfg_attrs)
                .map(|s| format!(" {}", s))
                .unwrap_or_default();
            if single_kind {
                msg.push_str(&format!("  {}:{}{}\n", file, line, cfg_suffix));
            } else {
                msg.push_str(&format!(
                    "  - {}:{}:{} [{}]{}\n",
                    file, line, request.name, kind, cfg_suffix
                ));
            }
        }
        if single_kind {


            msg.push_str(
                "hint: use --in <PATH_SUBSTR> to disambiguate by file path, or --symbol-id <PATH:LINE:NAME>",
            );


            let same_kind_label = hits[0].0.as_str();
            if matches!(same_kind_label, "struct" | "enum") {
                msg.push_str(&format!(
                    "\nrelated: `impls {}` for trait implementations, `refs {}` for all uses",
                    request.name, request.name
                ));
            }
        } else {


            let mentions_struct_or_enum =
                kinds.contains("struct") || kinds.contains("enum");
            if mentions_struct_or_enum {
                msg.push_str(&format!(
                    "related: `impls {}` for trait implementations, `refs {}` for all uses\n",
                    request.name, request.name
                ));
            }
        }
        return Err(msg.into());
    }

    let (kind, file, line, cfg_attrs) = &hits[0];


    if !args.json && (kind == "struct" || kind == "enum") {
        let kind_article = if kind == "struct" { "a" } else { "an" };
        let msg = format!(
            "'{}' is {} {} ({}:{}). Use `slice {}:{}` to see definition, `impls {}` for trait implementations, `refs {}` for all uses.",
            request.name, kind_article, kind, file, line, file, line, request.name, request.name
        );
        return Err(msg.into());
    }

    if args.json {
        let cfg_array = if cfg_attrs.is_empty() {
            "[]".to_string()
        } else {
            let parts: Vec<String> = cfg_attrs
                .iter()
                .map(|p| format!("\"{}\"", p.replace('\\', "\\\\").replace('"', "\\\"")))
                .collect();
            format!("[{}]", parts.join(", "))
        };
        let payload = format!(
            "{{\n  \"name\": \"{}\",\n  \"kind\": \"{}\",\n  \"file_path\": \"{}\",\n  \"start_line\": {},\n  \"cfg_attrs\": {}\n}}",
            request.name, kind, file, line, cfg_array
        );
        write_string_output(args.output.as_deref(), &payload)?;
    } else {
        let cfg_suffix = format_cfg_annotation(cfg_attrs)
            .map(|s| format!(" {}", s))
            .unwrap_or_default();
        let payload = format!("{}:{}:{} [{}]{}", file, line, request.name, kind, cfg_suffix);
        write_string_output(args.output.as_deref(), &payload)?;
    }
    Ok(())
}


fn cfg_distinguishes_all_hits(hits: &[(String, String, usize, Vec<String>)]) -> bool {
    if hits.len() < 2 {
        return false;
    }

    let first_kind = hits[0].0.as_str();
    if !hits.iter().all(|(k, _, _, _)| k == first_kind) {
        return false;
    }

    if !hits.iter().all(|(_, _, _, c)| !c.is_empty()) {
        return false;
    }

    let mut seen: HashSet<Vec<String>> = HashSet::new();
    for (_, _, _, c) in hits {
        if !seen.insert(c.clone()) {
            return false;
        }
    }
    true
}


struct NameUsage {
    total: usize,
    is_field: bool,
    is_variant: bool,
}

fn scan_name_usage(project: &ProjectData, name: &str) -> NameUsage {
    let is_field = project.structs.iter().any(|s| {
        s.fields
            .iter()
            .any(|f| field_name_matches(f, name))
    });
    let is_variant = project.enums.iter().any(|e| {
        e.variants.iter().any(|v| v.name == name)
    });

    let mut total: usize = 0;
    for file in &project.rust_files {
        let Some(syntax) = project.parsed_file(file) else {
            continue;
        };
        let mut counter = NameCounter { ident: name, count: 0 };
        counter.visit_file(syntax);
        total = total.saturating_add(counter.count);
    }

    NameUsage {
        total,
        is_field,
        is_variant,
    }
}


fn field_name_matches(field: &str, name: &str) -> bool {
    if let Some((head, _)) = field.split_once(':') {
        head.trim() == name
    } else {
        field.trim() == name
    }
}


struct NameCounter<'a> {
    ident: &'a str,
    count: usize,
}

impl<'a> NameCounter<'a> {
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
}

impl<'ast> Visit<'ast> for NameCounter<'_> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*node.func
            && self.matches_path(&path.path)
        {
            self.count += 1;
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == self.ident {
            self.count += 1;
        }
        syn::visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {
        if let syn::Member::Named(name) = &node.member
            && name == self.ident
        {
            self.count += 1;
        }
        syn::visit::visit_expr_field(self, node);
    }

    fn visit_pat_struct(&mut self, node: &'ast syn::PatStruct) {
        if self.matches_path(&node.path) || self.matches_as_qualifier(&node.path) {
            self.count += 1;
        }
        syn::visit::visit_pat_struct(self, node);
    }

    fn visit_pat_tuple_struct(&mut self, node: &'ast syn::PatTupleStruct) {
        if self.matches_path(&node.path) || self.matches_as_qualifier(&node.path) {
            self.count += 1;
        }
        syn::visit::visit_pat_tuple_struct(self, node);
    }

    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        if self.matches_path(&node.path) {
            self.count += 1;
        }
        syn::visit::visit_type_path(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if self.matches_path(&node.path) || self.matches_as_qualifier(&node.path) {
            self.count += 1;
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        if self.matches_path(&node.path) || self.matches_as_qualifier(&node.path) {
            self.count += 1;
        }
        syn::visit::visit_expr_struct(self, node);
    }
}
