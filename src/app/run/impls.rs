use std::fs;
use std::path::Path;

use syn::spanned::Spanned;
use syn::visit::Visit;

use super::super::changed::{ChangedRanges, file_was_changed};
use super::super::modes::ImplsRequest;
use super::super::project::ProjectData;
use crate::cli::Args;

/// Run `rustgraph impls <TRAIT> [--derived-only|--handwritten-only] [--in <PATH>]`.
///
/// Lists every type in the codebase that implements the given trait, distinguishing `#[derive]`
/// invocations from hand-written `impl Trait for Type` blocks.  Matching is done by the last
/// path segment so both `Serialize` and `serde::Serialize` resolve correctly.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: ImplsRequest,
    changed: Option<&ChangedRanges>,
) -> Result<(), Box<dyn std::error::Error>> {
    if request.derived_only && request.handwritten_only {
        return Err("--derived-only and --handwritten-only are mutually exclusive".into());
    }

    let target = request.trait_name.trim().to_string();
    let target_last = last_segment(&target);

    let mut hits: Vec<ImplHit> = Vec::new();

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

        if let Some(ranges) = changed
            && !file_was_changed(&file_str, ranges)
        {
            continue;
        }
        let Some(syntax) = project.parsed_file(file) else {
            continue;
        };
        let mut v = ImplVisitor {
            file_path: file_str.clone(),
            target: &target,
            target_last: &target_last,
            hits: &mut hits,
        };
        v.visit_file(syntax);
    }

    hits.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then_with(|| a.line.cmp(&b.line))
    });

    let derived_count = hits.iter().filter(|h| h.kind == ImplKind::Derived).count();
    let handwritten_count = hits
        .iter()
        .filter(|h| h.kind == ImplKind::Handwritten)
        .count();

    let filtered: Vec<&ImplHit> = hits
        .iter()
        .filter(|h| {
            if request.derived_only {
                h.kind == ImplKind::Derived
            } else if request.handwritten_only {
                h.kind == ImplKind::Handwritten
            } else {
                true
            }
        })
        .collect();


    let total_filtered = filtered.len();
    let cap = if request.max_results == 0 {
        usize::MAX
    } else {
        request.max_results
    };
    let truncated = total_filtered > cap;
    let display: Vec<&ImplHit> = filtered.iter().copied().take(cap).collect();

    if args.json {
        let arr: Vec<serde_json::Value> = display
            .iter()
            .map(|h| {
                let mut obj = serde_json::Map::new();
                obj.insert("type".into(), serde_json::Value::String(h.type_name.clone()));
                obj.insert(
                    "file_path".into(),
                    serde_json::Value::String(relative_path(&h.file_path, &args.path)),
                );
                obj.insert("line".into(), serde_json::Value::Number(h.line.into()));


                obj.insert(
                    "symbol_id".into(),
                    serde_json::Value::String(format!("struct:{}:{}:{}", h.file_path, h.line, h.type_name)),
                );
                obj.insert(
                    "kind".into(),
                    serde_json::Value::String(h.kind.as_str().to_string()),
                );
                obj.insert(
                    "module_path".into(),
                    serde_json::Value::String(module_path_from(&h.file_path, &args.path)),
                );
                obj.insert(
                    "trait_path".into(),
                    serde_json::Value::String(h.trait_path.clone()),
                );
                serde_json::Value::Object(obj)
            })
            .collect();
        let mut payload = serde_json::Map::new();
        payload.insert("trait".into(), serde_json::Value::String(target.clone()));
        payload.insert(
            "trait_last_segment".into(),
            serde_json::Value::String(target_last.clone()),
        );
        payload.insert("derived_count".into(), serde_json::Value::Number(derived_count.into()));
        payload.insert(
            "handwritten_count".into(),
            serde_json::Value::Number(handwritten_count.into()),
        );
        payload.insert(
            "total_count".into(),
            serde_json::Value::Number((derived_count + handwritten_count).into()),
        );
        payload.insert("truncated".into(), serde_json::Value::Bool(truncated));
        payload.insert("impls".into(), serde_json::Value::Array(arr));
        let serialized = serde_json::to_string_pretty(&serde_json::Value::Object(payload))?;
        super::switchboard::write_string_output(args.output.as_deref(), &serialized)?;
    } else {
        let mut out = String::new();
        out.push_str(&format!(
            "rustgraph impls '{}' (matched by last segment '{}'): {} derived, {} hand-written, {} total\n",
            target,
            target_last,
            derived_count,
            handwritten_count,
            derived_count + handwritten_count
        ));
        for h in &display {
            out.push_str(&format!(
                "{}:{} {}  [{}]  via {}  ({})\n",
                relative_path(&h.file_path, &args.path),
                h.line,
                h.type_name,
                h.kind.as_str(),
                h.trait_path,
                module_path_from(&h.file_path, &args.path),
            ));
        }
        if filtered.is_empty() {
            if hits.is_empty() {


                if let Some(needle) = &request.in_path {
                    out.push_str(&format!(
                        "(no implementors of {} found in files matching --in '{}'; widen the substring or drop --in)\n",
                        target, needle
                    ));
                } else if trait_referenced_anywhere(project, &target_last) {
                    out.push_str(&format!(
                        "(no types implement {} in this codebase — 0 implementors found via #[derive] or `impl {} for X`)\n",
                        target, target_last
                    ));
                } else {
                    out.push_str("(no matches — check the trait name spelling, or try the fully-qualified path)\n");
                }
            } else {
                let dropped_kind = if request.derived_only {
                    "hand-written"
                } else if request.handwritten_only {
                    "derive"
                } else {
                    "matched"
                };
                out.push_str(&format!(
                    "(filter --{}-only dropped all {} {} match(es); drop the filter to see them)\n",
                    if request.derived_only { "derived" } else { "handwritten" },
                    hits.len(),
                    dropped_kind
                ));
            }
        }
        if truncated {
            out.push_str(&format!(
                "(showing {} of {}; use --max-results to expand)\n",
                cap, total_filtered
            ));
        }
        super::switchboard::write_string_output(args.output.as_deref(), out.trim_end())?;
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImplKind {
    Derived,
    Handwritten,
}

impl ImplKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Derived => "derive",
            Self::Handwritten => "handwritten",
        }
    }
}

#[derive(Debug, Clone)]
struct ImplHit {
    type_name: String,
    file_path: String,
    line: usize,
    kind: ImplKind,
    trait_path: String,
}

struct ImplVisitor<'a> {
    file_path: String,
    target: &'a str,
    target_last: &'a str,
    hits: &'a mut Vec<ImplHit>,
}

impl<'a> ImplVisitor<'a> {
    fn check_derives(&mut self, attrs: &[syn::Attribute], type_name: &str, line: usize) {
        for attr in attrs {
            if !attr.path().is_ident("derive") {
                continue;
            }
            let mut matched_traits: Vec<String> = Vec::new();
            let _ = attr.parse_nested_meta(|meta| {
                let last = meta
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident.to_string())
                    .unwrap_or_default();
                let full = path_to_string(&meta.path);
                if last == self.target_last || full == self.target {
                    matched_traits.push(full);
                }
                Ok(())
            });
            for full in matched_traits {
                self.hits.push(ImplHit {
                    type_name: type_name.to_string(),
                    file_path: self.file_path.clone(),
                    line,
                    kind: ImplKind::Derived,
                    trait_path: full,
                });
            }
        }
    }
}

impl<'ast> Visit<'ast> for ImplVisitor<'_> {
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        let line = item.span().start().line;
        self.check_derives(&item.attrs, &item.ident.to_string(), line);
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        let line = item.span().start().line;
        self.check_derives(&item.attrs, &item.ident.to_string(), line);
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_item_union(&mut self, item: &'ast syn::ItemUnion) {
        let line = item.span().start().line;
        self.check_derives(&item.attrs, &item.ident.to_string(), line);
        syn::visit::visit_item_union(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if let Some((_, trait_path, _)) = &item.trait_ {
            let last = trait_path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            let full = path_to_string(trait_path);
            if last == self.target_last || full == self.target {
                let self_ty_name = self_type_head(&item.self_ty);
                let line = item.span().start().line;
                self.hits.push(ImplHit {
                    type_name: self_ty_name,
                    file_path: self.file_path.clone(),
                    line,
                    kind: ImplKind::Handwritten,
                    trait_path: full,
                });
            }
        }
        syn::visit::visit_item_impl(self, item);
    }
}

fn last_segment(name: &str) -> String {
    name.rsplit("::").next().unwrap_or(name).trim().to_string()
}

fn path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn self_type_head(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(tp) => {
            tp.path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_else(|| "<unknown>".into())
        }
        syn::Type::Reference(r) => self_type_head(&r.elem),
        _ => format!("{}", quote::quote! { #ty }),
    }
}

fn relative_path(file_path: &str, project_root: &Path) -> String {
    let path = Path::new(file_path);
    if let Ok(stripped) = path.strip_prefix(project_root) {
        crate::normalize_path_separators(&stripped.to_string_lossy())
    } else {
        crate::normalize_path_separators(file_path.trim_start_matches("./"))
    }
}

fn module_path_from(file_path: &str, project_root: &Path) -> String {

    let rel = relative_path(file_path, project_root);
    let trimmed = rel.trim_start_matches("./");


    let after_src = if let Some(idx) = trimmed.rfind("/src/") {
        &trimmed[idx + 5..]
    } else if let Some(stripped) = trimmed.strip_prefix("src/") {
        stripped
    } else {
        trimmed
    };
    let no_ext = after_src.strip_suffix(".rs").unwrap_or(after_src);
    let parts: Vec<&str> = no_ext
        .split('/')
        .filter(|p| !matches!(*p, "lib" | "main" | "mod"))
        .collect();
    if parts.is_empty() {
        "crate".to_string()
    } else {
        format!("crate::{}", parts.join("::"))
    }
}


fn trait_referenced_anywhere(project: &super::super::project::ProjectData, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    for file in &project.rust_files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let bytes = content.as_bytes();
        let needle = name.as_bytes();
        let mut idx = 0usize;
        while idx + needle.len() <= bytes.len() {
            if &bytes[idx..idx + needle.len()] == needle {
                let prev_ok = idx == 0
                    || !matches!(bytes[idx - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_');
                let next_ok = idx + needle.len() == bytes.len()
                    || !matches!(bytes[idx + needle.len()], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_');
                if prev_ok && next_ok {
                    return true;
                }
            }
            idx += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn run_impls(
        path: &Path,
        request: ImplsRequest,
    ) -> Result<String, Box<dyn std::error::Error>> {
        use crate::cli::Args;
        use clap::Parser;
        let path_arg = path.to_string_lossy().to_string();
        let args = Args::try_parse_from(["rustgraph", "--path", &path_arg])?;
        let project = ProjectData::load(&args.path, args.include_ignored);

        let captured_path = path.join("__impls_out.txt");
        let mut owned_args = args;
        owned_args.output = Some(captured_path.clone());
        run(&owned_args, &project, request, None)?;
        Ok(fs::read_to_string(&captured_path)?)
    }

    fn write(path: &Path, name: &str, content: &str) {
        let dst = path.join(name);
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        let mut f = fs::File::create(&dst).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    fn req(name: &str) -> ImplsRequest {
        ImplsRequest {
            trait_name: name.to_string(),
            derived_only: false,
            handwritten_only: false,
            in_path: None,
            max_results: 0,
        }
    }

    #[test]
    fn impls_finds_derive_serialize() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/lib.rs",
            "use serde::Serialize;\n#[derive(Serialize)]\npub struct Foo { x: u32 }\n",
        );
        let out = run_impls(dir.path(), req("Serialize")).unwrap();
        assert!(out.contains("Foo"), "got: {}", out);
        assert!(out.contains("derive"), "got: {}", out);
        assert!(out.contains("1 derived"), "got: {}", out);
    }

    #[test]
    fn impls_finds_handwritten_impl() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/lib.rs",
            "use serde::ser::Serializer;\npub struct Foo;\nimpl serde::Serialize for Foo {\n    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error> where S: Serializer { todo!() }\n}\n",
        );
        let out = run_impls(dir.path(), req("Serialize")).unwrap();
        assert!(out.contains("Foo"), "got: {}", out);
        assert!(out.contains("handwritten"), "got: {}", out);
        assert!(out.contains("1 hand-written"), "got: {}", out);
    }

    #[test]
    fn impls_handles_derive_with_path() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/lib.rs",
            "#[derive(serde::Serialize)]\npub struct Foo { x: u32 }\n",
        );
        let out = run_impls(dir.path(), req("Serialize")).unwrap();
        assert!(out.contains("Foo"), "got: {}", out);
        assert!(out.contains("derive"), "got: {}", out);
    }

    #[test]
    fn impls_module_path_heuristic() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/api.rs",
            "#[derive(serde::Serialize)]\npub struct SymbolRef;\n",
        );
        let out = run_impls(dir.path(), req("Serialize")).unwrap();
        assert!(out.contains("crate::api"), "got: {}", out);
    }

    #[test]
    fn impls_derived_only_filter_works() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/lib.rs",
            "#[derive(serde::Serialize)]\npub struct A;\npub struct B;\nimpl serde::Serialize for B { fn serialize<S>(&self, _: S) -> Result<S::Ok, S::Error> where S: serde::Serializer { todo!() } }\n",
        );
        let mut r = req("Serialize");
        r.derived_only = true;
        let out = run_impls(dir.path(), r).unwrap();
        assert!(out.contains("A"), "got: {}", out);
        assert!(!out.contains(" B "), "should hide handwritten; got: {}", out);
    }

    #[test]
    fn impls_no_matches_emits_hint() {
        let dir = tempdir().unwrap();
        write(dir.path(), "src/lib.rs", "pub struct Foo;\n");
        let out = run_impls(dir.path(), req("NonExistent")).unwrap();
        assert!(out.contains("no matches"), "got: {}", out);
    }

    #[test]
    fn impls_rejects_conflicting_filters() {
        let dir = tempdir().unwrap();
        write(dir.path(), "src/lib.rs", "pub struct Foo;\n");
        let mut r = req("Serialize");
        r.derived_only = true;
        r.handwritten_only = true;
        let err = run_impls(dir.path(), r);
        assert!(err.is_err());
    }
}
