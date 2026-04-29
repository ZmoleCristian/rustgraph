

use std::collections::HashMap;
use std::fs;

use serde::Serialize;
use syn::spanned::Spanned;
use syn::visit::Visit;

use super::super::modes::MembersRequest;
use super::super::project::ProjectData;
use crate::cli::Args;

/// Run `rustgraph members <TYPE> [--in <PATH>] [--max-results N]`.
///
/// Produces a per-field access rollup for a named struct: for each named field it finds every
/// `expr.field` access site across the codebase and reports location and count.
/// Returns an error if the named type is not a struct, does not exist, or has no named fields.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: MembersRequest,
) -> Result<(), Box<dyn std::error::Error>> {


    if let Some(struct_info) =
        project.structs.iter().find(|s| s.name == request.type_name)
    {
        let field_names = extract_field_names(&struct_info.fields);
        if field_names.is_empty() {
            return Err(format!(
                "'{}' is a unit struct (no fields). Try `refs {}` for type uses, or `slice {}` to view the definition.",
                request.type_name, request.type_name, request.type_name
            )
            .into());
        }


        let mut hits_by_field: HashMap<String, Vec<FieldHit>> = HashMap::new();
        for fname in &field_names {
            hits_by_field.insert(fname.clone(), Vec::new());
        }

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
            let content = match fs::read_to_string(file) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let Some(syntax) = project.parsed_file(file) else {
                continue;
            };
            let content_lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let mut v = MemberVisitor {
                file_path: file_str.clone(),
                fields: &field_names,
                hits_by_field: &mut hits_by_field,
                content_lines: &content_lines,
            };
            v.visit_file(syntax);
        }


        for hits in hits_by_field.values_mut() {
            hits.sort_by(|a, b| {
                a.file_path
                    .cmp(&b.file_path)
                    .then(a.line.cmp(&b.line))
                    .then(a.col.cmp(&b.col))
            });

            hits.dedup_by(|a, b| a.file_path == b.file_path && a.line == b.line && a.col == b.col);
        }


        let cap = if request.max_results == 0 {
            usize::MAX
        } else {
            request.max_results
        };


        let ordered_fields: Vec<String> = field_names.clone();
        let fields_with_accesses: Vec<&String> = ordered_fields
            .iter()
            .filter(|f| hits_by_field.get(*f).map(|v| !v.is_empty()).unwrap_or(false))
            .collect();

        if args.json {
            #[derive(Serialize)]
            struct FieldOut<'a> {
                name: &'a str,
                access_count: usize,
                truncated: bool,
                sites: Vec<&'a FieldHit>,
            }
            let fields_out: Vec<FieldOut> = ordered_fields
                .iter()
                .map(|fname| {
                    let hits = hits_by_field.get(fname).map(|v| v.as_slice()).unwrap_or(&[]);
                    let total = hits.len();
                    let display: Vec<&FieldHit> = hits.iter().take(cap).collect();
                    FieldOut {
                        name: fname.as_str(),
                        access_count: total,
                        truncated: total > cap,
                        sites: display,
                    }
                })
                .collect();
            #[derive(Serialize)]
            struct Wrap<'a> {
                #[serde(rename = "type")]
                type_name: &'a str,
                fields_with_accesses: usize,
                fields_total: usize,
                fields: Vec<FieldOut<'a>>,
            }
            let payload = serde_json::to_string_pretty(&Wrap {
                type_name: &request.type_name,
                fields_with_accesses: fields_with_accesses.len(),
                fields_total: ordered_fields.len(),
                fields: fields_out,
            })?;
            super::switchboard::write_string_output(args.output.as_deref(), &payload)?;
        } else {
            let mut out = format!(
                "rustgraph members '{}': {} field(s) with accesses (of {} total)\n",
                request.type_name,
                fields_with_accesses.len(),
                ordered_fields.len()
            );
            for fname in &ordered_fields {
                let hits = hits_by_field.get(fname).map(|v| v.as_slice()).unwrap_or(&[]);
                let total = hits.len();
                if total == 0 {


                    continue;
                }
                out.push_str(&format!("\n  {}: {} access(es)\n", fname, total));
                let display = hits.iter().take(cap);
                for h in display {
                    out.push_str(&format!(
                        "    {}:{}:{} - {}\n",
                        h.file_path,
                        h.line,
                        h.col,
                        h.line_text.trim()
                    ));
                }
                if total > cap {
                    out.push_str(&format!(
                        "    (showing {} of {}; use --max-results to expand)\n",
                        cap, total
                    ));
                }
            }
            super::switchboard::write_string_output(args.output.as_deref(), out.trim_end())?;
        }
        return Ok(());
    }


    if let Some(enum_info) = project.enums.iter().find(|e| e.name == request.type_name) {
        let _ = enum_info;
        return Err(format!(
            "'{}' is an enum, not a struct. `members` is for per-field rollups on structs only. Try `refs {} --kind variant` for variant uses, or `refs {}` for type uses.",
            request.type_name, request.type_name, request.type_name
        )
        .into());
    }


    let mut suggestions: Vec<(String, f64)> = project
        .structs
        .iter()
        .map(|s| {
            let sim = crate::fuzzy_similarity(&request.type_name, &s.name);
            (s.name.clone(), sim)
        })
        .filter(|(_, sim)| *sim >= 0.5)
        .collect();
    suggestions.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    suggestions.dedup_by(|a, b| a.0 == b.0);
    suggestions.truncate(3);

    let mut msg = format!(
        "no struct named '{}' found in the project (case-sensitive)",
        request.type_name
    );
    if !suggestions.is_empty() {
        msg.push_str("\nDid you mean: ");
        let names: Vec<String> = suggestions.into_iter().map(|(n, _)| n).collect();
        msg.push_str(&names.join(", "));
        msg.push('?');
    } else {
        msg.push_str(". Try `find ");
        msg.push_str(&request.type_name);
        msg.push_str("` for fuzzy lookup.");
    }
    Err(msg.into())
}


fn extract_field_names(fields: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for f in fields {
        if let Some((head, _)) = f.split_once(':') {
            let name = head.trim().to_string();

            if !name.is_empty() && !name.chars().all(|c| c.is_ascii_digit()) {
                names.push(name);
            }
        }
    }
    names
}

#[derive(Debug, Clone, Serialize)]
struct FieldHit {
    file_path: String,
    line: usize,
    col: usize,
    line_text: String,
}

struct MemberVisitor<'a> {
    file_path: String,
    fields: &'a [String],
    hits_by_field: &'a mut HashMap<String, Vec<FieldHit>>,
    content_lines: &'a [String],
}

impl<'ast> Visit<'ast> for MemberVisitor<'_> {
    fn visit_expr_field(&mut self, node: &'ast syn::ExprField) {


        if let syn::Member::Named(name) = &node.member {
            let name_str = name.to_string();
            if let Some(matched) = self.fields.iter().find(|f| **f == name_str) {
                let span = name.span();
                let loc = span.start();
                let line_text = self
                    .content_lines
                    .get(loc.line.saturating_sub(1))
                    .cloned()
                    .unwrap_or_default();
                self.hits_by_field
                    .entry(matched.clone())
                    .or_default()
                    .push(FieldHit {
                        file_path: self.file_path.clone(),
                        line: loc.line,
                        col: loc.column,
                        line_text,
                    });
            }
        }
        syn::visit::visit_expr_field(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_field_names_filters_tuple_positions() {
        let fields = vec![
            "x: u32".to_string(),
            "y: String".to_string(),
            "0: u32".to_string(),
        ];
        let names = extract_field_names(&fields);
        assert_eq!(names, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn extract_field_names_handles_complex_types() {
        let fields = vec![
            "data: HashMap<String, Vec<u8>>".to_string(),
            "callback: Box<dyn Fn() -> ()>".to_string(),
        ];
        let names = extract_field_names(&fields);
        assert_eq!(names, vec!["data".to_string(), "callback".to_string()]);
    }
}
