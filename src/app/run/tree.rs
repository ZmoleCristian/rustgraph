use std::collections::BTreeMap;
use std::path::Path;

use super::super::changed::{ChangedRanges, file_was_changed};
use super::super::modes::TreeRequest;
use super::super::project::ProjectData;
use crate::cli::Args;

#[derive(Default, Debug)]
struct FileCounts {
    functions: usize,
    structs: usize,
    enums: usize,
}

#[derive(Default, Debug)]
struct TreeNode {
    children: BTreeMap<String, TreeNode>,
    file: Option<FileCounts>,
}

impl TreeNode {
    fn insert(&mut self, parts: &[&str], counts: FileCounts) {
        if let Some((head, tail)) = parts.split_first() {
            let child = self.children.entry((*head).to_string()).or_default();
            if tail.is_empty() {
                child.file = Some(counts);
            } else {
                child.insert(tail, counts);
            }
        }
    }

    fn render(&self, prefix: &str, files_only: bool, out: &mut String) {
        let total = self.children.len();
        for (idx, (name, child)) in self.children.iter().enumerate() {
            let is_last = idx + 1 == total;
            let connector = if is_last { "└── " } else { "├── " };
            let label = match &child.file {
                Some(c) if !files_only => format!(
                    "{} ({} fn, {} struct, {} enum)",
                    name, c.functions, c.structs, c.enums
                ),
                _ => name.clone(),
            };
            out.push_str(prefix);
            out.push_str(connector);
            out.push_str(&label);
            out.push('\n');
            let next_prefix = if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}│   ", prefix)
            };
            child.render(&next_prefix, files_only, out);
        }
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


fn collect_prefix_suggestions(all_paths: &[String], needle: &str) -> Vec<(String, usize)> {
    use std::collections::HashMap;
    if needle.is_empty() {
        return Vec::new();
    }
    let mut buckets: HashMap<String, usize> = HashMap::new();
    for raw in all_paths {
        let segments: Vec<&str> = raw.split('/').collect();


        let pos = segments.iter().position(|s| *s == needle);
        if let Some(idx) = pos {

            let bucket = segments[..=idx].join("/");
            *buckets.entry(bucket).or_insert(0) += 1;
        }
    }
    let mut sorted: Vec<(String, usize)> = buckets.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sorted.truncate(3);
    sorted
}

/// Run `rustgraph tree [--prefix <PATH>] [--files-only] [--changed]`.
///
/// Renders a Unicode tree of the module/file hierarchy with per-file symbol counts
/// (functions, structs, enums).  `--prefix` scopes the tree to paths starting with
/// the given prefix; `--files-only` suppresses the counts; `--changed` limits output
/// to diff-touched files.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: TreeRequest,
    changed: Option<&ChangedRanges>,
) -> Result<(), Box<dyn std::error::Error>> {

    let mut per_file: BTreeMap<String, FileCounts> = BTreeMap::new();
    for f in &project.functions {
        per_file
            .entry(super::super::relativize_for_display(&f.file_path, &args.path, &args.also))
            .or_default()
            .functions += 1;
    }
    for s in &project.structs {
        per_file
            .entry(super::super::relativize_for_display(&s.file_path, &args.path, &args.also))
            .or_default()
            .structs += 1;
    }
    for e in &project.enums {
        per_file
            .entry(super::super::relativize_for_display(&e.file_path, &args.path, &args.also))
            .or_default()
            .enums += 1;
    }


    for raw in &project.rust_files {
        let rel = super::super::relativize_for_display(&raw.to_string_lossy(), &args.path, &args.also);
        per_file.entry(rel).or_default();
    }


    if let Some(ranges) = changed {
        per_file.retain(|path, _| file_was_changed(path, ranges));
    }

    let prefix_filter = request.prefix.as_deref().map(|p| p.trim_matches('/').to_string());


    let strip_prefix = prefix_filter.as_ref().map(|p| {
        if p.ends_with('/') { p.clone() } else { format!("{}/", p) }
    });

    let mut root = TreeNode::default();
    let mut included_files = 0usize;


    let all_paths: Vec<String> = per_file.keys().cloned().collect();
    for (rel, counts) in per_file {
        if let Some(pf) = &prefix_filter
            && !rel.starts_with(pf.as_str())
        {
            continue;
        }
        let trimmed: &str = match &strip_prefix {
            Some(sp) if rel.starts_with(sp.as_str()) => &rel[sp.len()..],
            _ => rel.as_str(),
        };
        let parts: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {

            continue;
        }
        root.insert(&parts, counts);
        included_files += 1;
    }


    if let Some(pf) = &prefix_filter
        && included_files == 0
        && !pf.is_empty()
    {
        let suggestions = collect_prefix_suggestions(&all_paths, pf);


        let bare_word_dir_hit = if !pf.contains('/') && !pf.contains(':') {
            let candidate_rel = format!("src/{}", pf);
            let candidate_abs = args.path.join(&candidate_rel);
            if candidate_abs.is_dir() {
                eprintln!(
                    "note: 0 files match '{}'. Did you mean: tree {} ?",
                    pf, candidate_rel
                );
                true
            } else {
                false
            }
        } else {
            false
        };
        if !bare_word_dir_hit && !suggestions.is_empty() {
            let body = suggestions
                .iter()
                .map(|(p, n)| format!("'{}' ({} files)", p, n))
                .collect::<Vec<_>>()
                .join(" or ");
            eprintln!(
                "note: '{}' matched 0 files; try {}",
                pf, body
            );
        }
    }


    let also_suffix = if args.also.is_empty() {
        String::new()
    } else {
        let labels: Vec<String> = args
            .also
            .iter()
            .map(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.display().to_string())
            })
            .collect();
        format!(" + {} sibling crate(s) [{}]", labels.len(), labels.join(", "))
    };
    let header = match &prefix_filter {
        Some(p) => format!(
            "tree of {}{} (under '{}'): {} file(s)\n",
            args.path.display(),
            also_suffix,
            p,
            included_files
        ),
        None => format!(
            "tree of {}{}: {} file(s)\n",
            args.path.display(),
            also_suffix,
            included_files
        ),
    };

    let mut out = header;
    root.render("", request.files_only, &mut out);

    if args.json {

        let mut counts_list: BTreeMap<String, FileCounts> = BTreeMap::new();
        for f in &project.functions {
            counts_list
                .entry(super::super::relativize_for_display(&f.file_path, &args.path, &args.also))
                .or_default()
                .functions += 1;
        }
        for s in &project.structs {
            counts_list
                .entry(super::super::relativize_for_display(&s.file_path, &args.path, &args.also))
                .or_default()
                .structs += 1;
        }
        for e in &project.enums {
            counts_list
                .entry(super::super::relativize_for_display(&e.file_path, &args.path, &args.also))
                .or_default()
                .enums += 1;
        }
        for raw in &project.rust_files {
            let rel = super::super::relativize_for_display(&raw.to_string_lossy(), &args.path, &args.also);
            counts_list.entry(rel).or_default();
        }


        let entries: Vec<serde_json::Value> = counts_list
            .into_iter()
            .filter(|(rel, _)| {
                if let Some(pf) = &prefix_filter
                    && !rel.starts_with(pf.as_str())
                {
                    return false;
                }
                if let Some(ranges) = changed
                    && !file_was_changed(rel, ranges)
                {
                    return false;
                }
                true
            })
            .map(|(rel, c)| {
                serde_json::json!({
                    "path": rel,
                    "functions": c.functions,
                    "structs": c.structs,
                    "enums": c.enums,
                })
            })
            .collect();

        let payload = format!("{}\n", serde_json::to_string_pretty(&entries)?);
        super::switchboard::write_string_output(args.output.as_deref(), &payload)?;
    } else {
        super::switchboard::write_string_output(args.output.as_deref(), out.trim_end())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn relative_path_strips_project_prefix() {
        let dir = tempdir().expect("tempdir");
        let abs = dir.path().join("src/lib.rs");
        fs::create_dir_all(abs.parent().unwrap()).unwrap();
        fs::write(&abs, "").unwrap();
        let rel = relative_path(abs.to_str().unwrap(), dir.path());
        assert_eq!(rel, "src/lib.rs");
    }

    #[test]
    fn tree_node_renders_nested_files() {
        let mut root = TreeNode::default();
        root.insert(
            &["src", "lib.rs"],
            FileCounts { functions: 2, structs: 1, enums: 0 },
        );
        root.insert(
            &["src", "main.rs"],
            FileCounts { functions: 1, structs: 0, enums: 0 },
        );
        let mut out = String::new();
        root.render("", false, &mut out);
        assert!(out.contains("src"));
        assert!(out.contains("lib.rs (2 fn, 1 struct, 0 enum)"));
        assert!(out.contains("main.rs (1 fn"));
    }

    #[test]
    fn tree_node_files_only_omits_counts() {
        let mut root = TreeNode::default();
        root.insert(
            &["lib.rs"],
            FileCounts { functions: 5, structs: 0, enums: 0 },
        );
        let mut out = String::new();
        root.render("", true, &mut out);
        assert!(out.contains("lib.rs"));
        assert!(!out.contains("fn"));
    }

    #[test]
    fn tree_json_round_trips_paths_with_quote_and_backslash() {

        let weird = r#"src/odd "file"\with-backslash.rs"#;
        let entry = serde_json::json!({
            "path": weird,
            "functions": 3usize,
            "structs": 1usize,
            "enums": 0usize,
        });
        let payload = serde_json::to_string_pretty(&vec![entry])
            .expect("serialize entry array");


        let parsed: serde_json::Value =
            serde_json::from_str(&payload).expect("payload must be valid JSON");
        let arr = parsed.as_array().expect("top level array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["path"].as_str(), Some(weird));
        assert_eq!(arr[0]["functions"].as_u64(), Some(3));
        assert_eq!(arr[0]["structs"].as_u64(), Some(1));
        assert_eq!(arr[0]["enums"].as_u64(), Some(0));
    }
}
