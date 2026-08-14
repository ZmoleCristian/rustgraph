use std::collections::HashMap;
use std::fs;
use std::path::Path;

use ignore::WalkBuilder;
use regex::RegexBuilder;

use super::super::changed::{ChangedRanges, file_was_changed};
use super::super::modes::GrepRequest;
use super::super::project::ProjectData;
use crate::FunctionInfo;
use crate::cli::Args;

/// Run `rustgraph grep <PATTERN> [--fixed-string] [-i] [--by-function] [--by-file] [--in <PATH>]`.
///
/// Searches only `.rs` files for lines matching a Rust regex (or literal string with `-F`).
/// Supports optional context lines, enclosing-function annotation, per-function or per-file
/// rollup modes, `--changed` diff filtering, and a configurable result cap via `--max-results`.
pub fn run(
    args: &Args,
    project: &ProjectData,
    mut request: GrepRequest,
    changed: Option<&ChangedRanges>,
) -> Result<(), Box<dyn std::error::Error>> {
    if request.by_function {
        request.enclosing_function = true;
    }

    if args.exclude_tests {
        request.enclosing_function = true;
    }

    let paren_warning_pending = !request.fixed_string
        && (request.pattern.contains(r"\(") || request.pattern.contains(r"\)"));
    if !request.fixed_string && request.pattern.contains(r"\|") {
        let suggested = request.pattern.replace(r"\|", "|");
        let bar = "\u{2501}".repeat(78);
        eprintln!("{}", bar);

        eprintln!(
            "warning: pattern contains `\\|` — Rust regex uses `|` (no escape) for OR. `\\|` matches a literal pipe — did you mean `{}`?",
            suggested
        );
        eprintln!(
            "  Use `-F`/`--fixed-string` for literal-text match, or escape only meta-chars you intend."
        );
        eprintln!("{}", bar);
    }

    let pattern_str = if request.fixed_string {
        regex::escape(&request.pattern)
    } else {
        request.pattern.clone()
    };

    let regex = match RegexBuilder::new(&pattern_str)
        .case_insensitive(request.ignore_case)
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            if paren_warning_pending {
                emit_paren_warning(&request.pattern);
            }
            let msg = if request.fixed_string {
                format!("invalid regex: {}", e)
            } else {
                format!(
                    "invalid regex pattern '{}': {}\n  hint: shell-quote your pattern (single quotes) and escape regex metacharacters: ( ) [ ] . * + ? ^ $ | \\\n  or use -F/--fixed-string for literal-text search",
                    request.pattern, e
                )
            };
            return Err(msg.into());
        }
    };

    let mut files: Vec<(String, String)> = if !project.rust_files.is_empty() {
        project
            .rust_files
            .iter()
            .map(|p| {
                let abs = crate::normalize_path_separators(&p.to_string_lossy());
                let key = if args.absolute_paths {
                    abs.clone()
                } else {
                    super::super::relativize_for_display(&abs, &args.path, &args.also)
                };
                (abs, key)
            })
            .collect()
    } else {
        walk_rust_files(&args.path, args.include_ignored)
            .into_iter()
            .map(|s| (s.clone(), s))
            .collect()
    };

    if let Some(needle) = &request.in_path {
        let before = files.len();
        files.retain(|(_, key)| key.contains(needle));
        if files.is_empty() && before > 0 {
            eprintln!(
                "hint: --in '{}' filtered out all {} file(s); try a shorter substring",
                needle, before
            );
        }
    }

    if let Some(ranges) = changed {
        let before = files.len();
        files.retain(|(_, key)| file_was_changed(key, ranges));
        if files.is_empty() && before > 0 {
            eprintln!(
                "note: --changed filter dropped all {} file(s); 0 files in the diff matched",
                before
            );
        }
    }

    let by_file: HashMap<String, Vec<&FunctionInfo>> = if request.enclosing_function {
        let mut map: HashMap<String, Vec<&FunctionInfo>> = HashMap::new();
        for f in &project.functions {
            let key = if args.absolute_paths {
                f.file_path.clone()
            } else {
                super::super::relativize_for_display(&f.file_path, &args.path, &args.also)
            };
            map.entry(key).or_default().push(f);
        }
        map
    } else {
        HashMap::new()
    };

    let cap = if request.max_results == 0 {
        usize::MAX
    } else {
        request.max_results
    };

    let before_context = request.before_context;
    let after_context = request.after_context;
    let context_total = before_context + after_context;

    let mut buf = String::new();
    let mut json_matches: Vec<MatchRow> = Vec::new();
    let mut total_matches = 0usize;
    let mut truncated = false;

    let mut summary: HashMap<String, FnSummary> = HashMap::new();
    let mut orphan_count = 0usize;

    let mut by_file_counts: HashMap<String, usize> = HashMap::new();

    let mut raw_match_count = 0usize;
    let mut excluded_examples: Vec<String> = Vec::new();

    let mut total_post_filter = 0usize;

    for (abs, key) in &files {
        let content = match fs::read_to_string(abs) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let lines: Vec<&str> = content.lines().collect();
        let rel = key.clone();
        let fns = by_file.get(key);

        for (idx, line) in lines.iter().enumerate() {
            if !regex.is_match(line) {
                continue;
            }

            raw_match_count += 1;
            let line_no = idx + 1;
            let enclosing = if request.enclosing_function {
                fns.and_then(|fs| innermost_enclosing(fs, line_no))
            } else {
                None
            };

            if args.exclude_tests && enclosing.is_some_and(|f| f.is_test) {
                if excluded_examples.len() < 3 {
                    excluded_examples.push(format!("{}:{}", rel, line_no));
                }
                continue;
            }

            total_post_filter += 1;
            if total_matches >= cap {
                truncated = true;
                continue;
            }

            if request.enclosing_function {
                if let Some(f) = enclosing {
                    let key = format!("{}:{}:{}", f.file_path, f.start_line, f.name);
                    let entry = summary.entry(key).or_insert_with(|| FnSummary {
                        name: f.name.clone(),
                        file_path: relative_path(&f.file_path, &args.path),
                        start_line: f.start_line,
                        end_line: f.end_line,
                        is_test: f.is_test,
                        count: 0,
                    });
                    entry.count += 1;
                } else {
                    orphan_count += 1;
                }
            }

            *by_file_counts.entry(rel.clone()).or_insert(0) += 1;

            if request.by_function || request.by_file {
            } else if args.json {
                json_matches.push(MatchRow {
                    path: rel.clone(),
                    line: line_no,
                    text: line.to_string(),
                    enclosing_function: enclosing.map(|f| f.name.clone()),
                    enclosing_function_file: enclosing
                        .map(|f| relative_path(&f.file_path, &args.path)),
                    enclosing_function_start: enclosing.map(|f| f.start_line),
                    enclosing_function_end: enclosing.map(|f| f.end_line),
                    is_test: enclosing.map(|f| f.is_test),
                });
            } else if context_total == 0 {
                let suffix = enclosing
                    .map(|f| {
                        format!(
                            "    [fn: {} @ {}:{}-{}]",
                            f.name,
                            relative_path(&f.file_path, &args.path),
                            f.start_line,
                            f.end_line
                        )
                    })
                    .unwrap_or_default();
                buf.push_str(&format!("{}:{}:{}{}\n", rel, line_no, line, suffix));
            } else {
                let lo = idx.saturating_sub(before_context);
                let hi = (idx + after_context + 1).min(lines.len());
                let header = if let Some(f) = enclosing {
                    format!(
                        "--- {}:{}  [fn: {} @ {}:{}-{}] ---\n",
                        rel,
                        line_no,
                        f.name,
                        relative_path(&f.file_path, &args.path),
                        f.start_line,
                        f.end_line
                    )
                } else {
                    format!("--- {}:{} ---\n", rel, line_no)
                };
                buf.push_str(&header);
                for i in lo..hi {
                    let marker = if i == idx { ">" } else { " " };
                    buf.push_str(&format!("{} {}: {}\n", marker, i + 1, lines[i]));
                }
            }

            total_matches += 1;
        }
    }

    let header = format!(
        "rustgraph grep '{}' across {} file(s): {} match(es){}{}\n",
        request.pattern,
        files.len(),
        total_post_filter,
        if truncated {
            format!(
                " (showing {} of {}; use --max-results to expand)",
                total_matches, total_post_filter
            )
        } else {
            String::new()
        },
        if request.by_function {
            "  [--by-function: per-line dump suppressed, summary only]"
        } else if request.by_file {
            "  [--by-file: per-line dump suppressed, file rollup only]"
        } else {
            ""
        }
    );

    if args.json {
        let mut payload = serde_json::Map::new();
        payload.insert(
            "pattern".into(),
            serde_json::Value::String(request.pattern.clone()),
        );

        payload.insert(
            "total_matches".into(),
            serde_json::Value::Number(total_post_filter.into()),
        );
        payload.insert("truncated".into(), serde_json::Value::Bool(truncated));

        if request.by_function {
            payload.insert("by_function".into(), serde_json::Value::Bool(true));
        }
        if request.by_file {
            payload.insert("by_file_mode".into(), serde_json::Value::Bool(true));
        }
        if !request.by_function && !request.by_file {
            payload.insert(
                "matches".into(),
                serde_json::Value::Array(
                    json_matches
                        .into_iter()
                        .map(|m| {
                            let mut obj = serde_json::Map::new();

                            obj.insert("file_path".into(), serde_json::Value::String(m.path));
                            obj.insert(
                                "start_line".into(),
                                serde_json::Value::Number(m.line.into()),
                            );
                            obj.insert("text".into(), serde_json::Value::String(m.text));
                            if request.enclosing_function {
                                obj.insert(
                                    "enclosing_function".into(),
                                    m.enclosing_function
                                        .map(serde_json::Value::String)
                                        .unwrap_or(serde_json::Value::Null),
                                );
                                obj.insert(
                                    "enclosing_function_file".into(),
                                    m.enclosing_function_file
                                        .map(serde_json::Value::String)
                                        .unwrap_or(serde_json::Value::Null),
                                );
                                obj.insert(
                                    "enclosing_function_start".into(),
                                    m.enclosing_function_start
                                        .map(|n| serde_json::Value::Number(n.into()))
                                        .unwrap_or(serde_json::Value::Null),
                                );
                                obj.insert(
                                    "enclosing_function_end".into(),
                                    m.enclosing_function_end
                                        .map(|n| serde_json::Value::Number(n.into()))
                                        .unwrap_or(serde_json::Value::Null),
                                );
                                obj.insert(
                                    "is_test".into(),
                                    m.is_test
                                        .map(serde_json::Value::Bool)
                                        .unwrap_or(serde_json::Value::Null),
                                );
                            }
                            serde_json::Value::Object(obj)
                        })
                        .collect(),
                ),
            );
        }

        if request.by_file {
            let mut rows: Vec<(String, usize)> = by_file_counts
                .iter()
                .map(|(p, c)| (p.clone(), *c))
                .collect();
            rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            payload.insert(
                "by_file".into(),
                serde_json::Value::Array(
                    rows.into_iter()
                        .map(|(path, count)| {
                            let mut obj = serde_json::Map::new();
                            obj.insert("path".into(), serde_json::Value::String(path));
                            obj.insert("count".into(), serde_json::Value::Number(count.into()));
                            serde_json::Value::Object(obj)
                        })
                        .collect(),
                ),
            );
        }
        if request.enclosing_function {
            let mut sums: Vec<&FnSummary> = summary.values().collect();
            sums.sort_by(|a, b| {
                b.count
                    .cmp(&a.count)
                    .then_with(|| a.file_path.cmp(&b.file_path))
            });
            payload.insert(
                "summary_by_function".into(),
                serde_json::Value::Array(
                    sums.into_iter()
                        .map(|s| {
                            let mut obj = serde_json::Map::new();
                            obj.insert("name".into(), serde_json::Value::String(s.name.clone()));
                            obj.insert(
                                "file_path".into(),
                                serde_json::Value::String(s.file_path.clone()),
                            );
                            obj.insert(
                                "start_line".into(),
                                serde_json::Value::Number(s.start_line.into()),
                            );
                            obj.insert(
                                "end_line".into(),
                                serde_json::Value::Number(s.end_line.into()),
                            );
                            obj.insert("is_test".into(), serde_json::Value::Bool(s.is_test));
                            obj.insert("count".into(), serde_json::Value::Number(s.count.into()));
                            serde_json::Value::Object(obj)
                        })
                        .collect(),
                ),
            );
            payload.insert(
                "orphan_match_count".into(),
                serde_json::Value::Number(orphan_count.into()),
            );
        }
        let serialized = serde_json::to_string_pretty(&serde_json::Value::Object(payload))?;
        super::switchboard::write_string_output(args.output.as_deref(), &serialized)?;
        if truncated {
            return Err(format!(
                "grep -j truncated: showing {} of {} matches; use --max-results to expand. JSON body has `truncated: true` for inspection (re-run with --max-results 0 for unlimited).",
                total_matches, total_post_filter
            )
            .into());
        }
    } else {
        let mut out = header;
        out.push_str(&buf);

        if request.by_file {
            let mut rows: Vec<(String, usize)> = by_file_counts
                .iter()
                .map(|(p, c)| (p.clone(), *c))
                .collect();
            rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            for (path, count) in &rows {
                let plural = if *count == 1 { "match" } else { "matches" };
                out.push_str(&format!("{}: {} {}\n", path, count, plural));
            }
            let total: usize = rows.iter().map(|(_, c)| *c).sum();
            let plural_total = if total == 1 { "match" } else { "matches" };
            let plural_files = if rows.len() == 1 { "file" } else { "files" };
            out.push_str(&format!(
                "total: {} {} across {} {}\n",
                total,
                plural_total,
                rows.len(),
                plural_files
            ));
        }

        if request.enclosing_function && !request.by_file {
            let mut sums: Vec<&FnSummary> = summary.values().collect();
            sums.sort_by(|a, b| {
                b.count
                    .cmp(&a.count)
                    .then_with(|| a.file_path.cmp(&b.file_path))
            });
            out.push_str(&format!(
                "\n=== summary by enclosing function ({} functions, {} matches; {} orphan) ===\n",
                sums.len(),
                total_matches,
                orphan_count
            ));
            for s in sums {
                let test_tag = if s.is_test { " [test]" } else { "" };
                out.push_str(&format!(
                    "{}:{} {} — {} match(es){}\n",
                    s.file_path, s.start_line, s.name, s.count, test_tag
                ));
            }
            if orphan_count > 0 {
                out.push_str(&format!(
                    "<orphan, no enclosing fn> — {} match(es)\n",
                    orphan_count
                ));
            }
        }
        super::switchboard::write_string_output(args.output.as_deref(), out.trim_end())?;
        if truncated {
            eprintln!(
                "note: showing {} of {} matches; use --max-results to expand (or pass --max-results 0 for unlimited).",
                total_matches, total_post_filter
            );
        }
    }

    if args.exclude_tests && total_matches == 0 && raw_match_count > 0 {
        let examples = if excluded_examples.is_empty() {
            String::new()
        } else {
            format!(
                "\nHint: drop --exclude-tests to see them, or check {}",
                excluded_examples.join(", ")
            )
        };
        eprintln!(
            "Note: {} matches found but all dropped by --exclude-tests (in test fns).{}",
            raw_match_count, examples
        );
    }

    if paren_warning_pending && total_matches == 0 {
        emit_paren_warning(&request.pattern);
    }

    Ok(())
}

fn emit_paren_warning(pattern: &str) {
    let suggested = pattern.replace(r"\(", "(").replace(r"\)", ")");
    let bar = "\u{2501}".repeat(78);
    eprintln!("{}", bar);

    eprintln!(
        "note: pattern contains `\\(` or `\\)` — Rust regex uses bare `(...)` for grouping (PCRE-style `\\(...\\)` matches literal parens). Try: '{}'",
        suggested
    );
    eprintln!(
        "  Use `-F`/`--fixed-string` for literal-text match, or escape only meta-chars you intend."
    );
    eprintln!("{}", bar);
}

#[derive(Debug, Clone)]
struct MatchRow {
    path: String,
    line: usize,
    text: String,
    enclosing_function: Option<String>,
    enclosing_function_file: Option<String>,
    enclosing_function_start: Option<usize>,
    enclosing_function_end: Option<usize>,
    is_test: Option<bool>,
}

#[derive(Debug, Clone)]
struct FnSummary {
    name: String,
    file_path: String,
    start_line: usize,
    end_line: usize,
    is_test: bool,
    count: usize,
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

fn relative_path(file_path: &str, project_root: &Path) -> String {
    let path = Path::new(file_path);
    if let Ok(stripped) = path.strip_prefix(project_root) {
        crate::normalize_path_separators(&stripped.to_string_lossy())
    } else {
        crate::normalize_path_separators(file_path.trim_start_matches("./"))
    }
}

fn walk_rust_files(root: &Path, include_ignored: bool) -> Vec<String> {
    let mut walk = WalkBuilder::new(root);
    walk.standard_filters(!include_ignored);
    walk.build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_some_and(|ft| ft.is_file()))
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "rs"))
        .map(|entry| entry.path().to_string_lossy().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn run_grep(path: &Path, request: GrepRequest) -> Result<String, Box<dyn std::error::Error>> {
        use crate::cli::Args;
        use clap::Parser;
        let path_arg = path.to_string_lossy().to_string();
        let args = Args::try_parse_from(["rustgraph", "--path", &path_arg])?;
        let project = ProjectData::load(&args.path, args.include_ignored);

        let captured_path = path.join("__grep_out.txt");
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

    fn req(pattern: &str) -> GrepRequest {
        GrepRequest {
            pattern: pattern.to_string(),
            ignore_case: false,
            fixed_string: true,
            context: None,
            before_context: 0,
            after_context: 0,
            max_results: 200,
            enclosing_function: false,
            by_function: false,
            by_file: false,
            in_path: None,
        }
    }

    #[test]
    fn grep_finds_literal_matches_in_comments() {
        let dir = tempdir().unwrap();
        write(dir.path(), "src/lib.rs", "// TODO: fix later\nfn ok() {}\n");
        let out = run_grep(dir.path(), req("TODO")).unwrap();
        assert!(out.contains("TODO: fix later"), "got: {}", out);
        assert!(out.contains("src/lib.rs:1:"));
    }

    #[test]
    fn grep_regex_pattern_matches() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/lib.rs",
            "let x: u32 = 42;\nlet y: i64 = 7;\n",
        );
        let mut r = req(r"u\d+");
        r.fixed_string = false;
        let out = run_grep(dir.path(), r).unwrap();
        assert!(out.contains("u32"), "got: {}", out);
        assert!(!out.contains("i64"));
    }

    #[test]
    fn grep_ignore_case_works() {
        let dir = tempdir().unwrap();
        write(dir.path(), "src/lib.rs", "FIXME me\n");
        let mut r = req("fixme");
        r.ignore_case = true;
        let out = run_grep(dir.path(), r).unwrap();
        assert!(out.contains("FIXME"), "got: {}", out);
    }

    #[test]
    fn grep_max_results_truncates() {
        let dir = tempdir().unwrap();
        let body: String = (0..50).map(|_| "needle\n").collect();
        write(dir.path(), "src/lib.rs", &body);
        let mut r = req("needle");
        r.max_results = 5;
        let out = run_grep(dir.path(), r).unwrap();
        assert!(out.contains("showing 5 of 50"), "got: {}", out);
        assert!(out.contains("--max-results"), "got: {}", out);
    }

    #[test]
    fn grep_invalid_regex_returns_error() {
        let dir = tempdir().unwrap();
        write(dir.path(), "src/lib.rs", "fn x(){}\n");
        let mut r = req("(unclosed");
        r.fixed_string = false;
        let err = run_grep(dir.path(), r);
        assert!(err.is_err());
    }

    #[test]
    fn grep_enclosing_function_tags_match_with_owner_fn() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/lib.rs",
            "fn alpha() {\n    let _ = some.unwrap();\n}\n\nfn beta() {\n    let _ = other.unwrap();\n    let _ = third.unwrap();\n}\n",
        );
        let mut r = req("unwrap");
        r.enclosing_function = true;
        let out = run_grep(dir.path(), r).unwrap();
        assert!(out.contains("[fn: alpha"), "got: {}", out);
        assert!(out.contains("[fn: beta"), "got: {}", out);

        assert!(
            out.contains("summary by enclosing function (2 functions"),
            "got: {}",
            out
        );

        assert!(out.contains("beta — 2 match"), "got: {}", out);
        assert!(out.contains("alpha — 1 match"), "got: {}", out);
    }

    #[test]
    fn grep_by_function_suppresses_per_line_dump_and_keeps_summary() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/lib.rs",
            "fn alpha() {\n    let _ = some.unwrap();\n    let _ = other.unwrap();\n}\nfn beta() {\n    let _ = third.unwrap();\n}\n",
        );
        let mut r = req("unwrap");
        r.by_function = true;
        let out = run_grep(dir.path(), r).unwrap();

        assert!(
            !out.contains("[fn: alpha @"),
            "per-line dump should be suppressed; got: {}",
            out
        );

        assert!(
            out.contains("summary by enclosing function (2 functions"),
            "got: {}",
            out
        );
        assert!(out.contains("alpha — 2 match"), "got: {}", out);
        assert!(out.contains("beta — 1 match"), "got: {}", out);

        assert!(
            out.contains("--by-function"),
            "header should mention mode; got: {}",
            out
        );
    }

    #[test]
    fn grep_by_function_implies_enclosing_function() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/lib.rs",
            "fn alpha() {\n    let _ = some.unwrap();\n}\n",
        );

        let mut r = req("unwrap");
        r.by_function = true;
        r.enclosing_function = false;
        let out = run_grep(dir.path(), r).unwrap();
        assert!(out.contains("alpha — 1 match"), "got: {}", out);
    }

    #[test]
    fn grep_enclosing_function_picks_innermost_for_nested() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "src/lib.rs",
            "fn outer() {\n    fn inner() {\n        let _ = x.unwrap();\n    }\n}\n",
        );
        let mut r = req("unwrap");
        r.enclosing_function = true;
        let out = run_grep(dir.path(), r).unwrap();
        assert!(
            out.contains("[fn: inner"),
            "expected innermost; got: {}",
            out
        );
    }
}
