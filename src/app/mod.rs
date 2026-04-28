//! Orchestration layer between the CLI parser and subcommand runners.
//!
//! Receives a parsed [`crate::cli::Args`], selects an [`modes::ExecutionMode`],
//! loads [`project::ProjectData`], applies optional search / changed-range filters,
//! and delegates to `run::execute`.

pub mod callers;
pub mod changed;
pub mod modes;
pub mod project;
pub mod render;
mod run;
pub mod symbol_id;

use clap::CommandFactory;

use crate::cli::Args;
use modes::ExecutionMode;
use project::ProjectData;

/// Strips the primary root or any `--also` root prefix from `path`, returning a display-friendly
/// relative string. Falls back to stripping `./` if no root matches.
pub(crate) fn relativize_for_display(
    path: &str,
    root: &std::path::Path,
    also_roots: &[std::path::PathBuf],
) -> String {
    let pb = std::path::Path::new(path);
    let canon = std::fs::canonicalize(pb).unwrap_or_else(|_| pb.to_path_buf());
    let abs_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if let Ok(stripped) = canon.strip_prefix(&abs_root) {
        return stripped.to_string_lossy().to_string();
    }
    if let Ok(stripped) = pb.strip_prefix(&abs_root) {
        return stripped.to_string_lossy().to_string();
    }
    if let Ok(stripped) = pb.strip_prefix(root) {
        return stripped.to_string_lossy().to_string();
    }
    for r in also_roots {
        let canon_also = std::fs::canonicalize(r).unwrap_or_else(|_| r.clone());
        let label = canon_also
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "also".to_string());
        if let Ok(stripped) = canon.strip_prefix(&canon_also) {
            return format!("{}/{}", label, stripped.to_string_lossy());
        }
        if let Ok(stripped) = pb.strip_prefix(&canon_also) {
            return format!("{}/{}", label, stripped.to_string_lossy());
        }
    }
    path.trim_start_matches("./").to_string()
}


fn relativize_project_paths(
    project: &mut crate::app::project::ProjectData,
    root: &std::path::Path,
    also_roots: &[std::path::PathBuf],
) {
    let strip = |p: &str| -> String { relativize_for_display(p, root, also_roots) };
    for f in project.functions.iter_mut() {
        f.file_path = strip(&f.file_path);
    }
    for s in project.structs.iter_mut() {
        s.file_path = strip(&s.file_path);
    }
    for e in project.enums.iter_mut() {
        e.file_path = strip(&e.file_path);
    }
    for cs in project.call_sites.iter_mut() {
        cs.file_path = strip(&cs.file_path);
    }

    let mut new_call_map = std::collections::HashMap::new();
    for (key, val) in std::mem::take(&mut project.call_map) {

        let new_key = if let Some((path_part, rest)) = key.split_once(':') {
            format!("{}:{}", strip(path_part), rest)
        } else {
            key
        };
        new_call_map.insert(new_key, val);
    }
    project.call_map = new_call_map;

    for cs in project.call_sites.iter_mut() {
        if let Some(id) = cs.caller_id.as_mut() {
            if let Some((path_part, rest)) = id.split_once(':') {
                *id = format!("{}:{}", strip(path_part), rest);
            }
        }
    }
}

fn find_crate_root_from_cwd() -> Option<std::path::PathBuf> {
    let mut cur = std::env::current_dir().ok()?;
    loop {
        if cur.join("Cargo.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn no_action_specified(args: &Args) -> bool {
    args.command.is_none()
        && args.search.is_none()
        && args.analyze.is_none()
        && !args.func
        && !args.r#struct
        && !args.r#enum
        && !args.call_graph
        && !args.dead_code
        && args.ensemble.is_none()
        && args.callers.is_none()
}

/// Top-level entry point: validates args, auto-detects crate root, loads and merges
/// `ProjectData`, applies filters, then dispatches to the appropriate subcommand runner.
pub fn run(mut args: Args) -> Result<(), Box<dyn std::error::Error>> {
    if no_action_specified(&args) {
        crate::mcp_install::print_nudge_if_needed();
        Args::command().print_help()?;
        println!();
        return Ok(());
    }

    if !args.no_auto_path && args.path == std::path::PathBuf::from(".") {
        if let Some(detected) = find_crate_root_from_cwd() {


            let cwd = std::env::current_dir().ok();
            let is_silent = cwd.as_ref().is_some_and(|c| {
                std::fs::canonicalize(c).ok() == std::fs::canonicalize(&detected).ok()
            });
            if !is_silent {
                eprintln!(
                    "auto-detected crate root: {} (override with -p or --no-auto-path)",
                    detected.display()
                );
            }
            args.path = detected;
        }
    }

    if !args.path.exists() {
        return Err(format!(
            "-p '{}' does not exist. Pass an existing crate root or omit -p to auto-detect Cargo.toml from cwd.",
            args.path.display()
        )
        .into());
    }
    if !args.path.is_dir() {
        return Err(format!(
            "-p '{}' is not a directory. Pass a directory containing Rust source files.",
            args.path.display()
        )
        .into());
    }

    let mode = ExecutionMode::from_args(&args);
    let mut project = ProjectData::load(&args.path, args.include_ignored);

    for extra in &args.also {
        let extra_canon =
            std::fs::canonicalize(extra).unwrap_or_else(|_| extra.clone());
        let primary_canon =
            std::fs::canonicalize(&args.path).unwrap_or_else(|_| args.path.clone());
        if extra_canon == primary_canon {
            eprintln!("--also '{}': same as -p root, skipped", extra.display());
            continue;
        }
        let extra_project = ProjectData::load(extra, args.include_ignored);
        eprintln!(
            "--also '{}': merged {} fn(s), {} struct(s), {} enum(s), {} file(s)",
            extra.display(),
            extra_project.functions.len(),
            extra_project.structs.len(),
            extra_project.enums.len(),
            extra_project.rust_files.len()
        );
        project.rust_files.extend(extra_project.rust_files);
        project.functions.extend(extra_project.functions);
        project.structs.extend(extra_project.structs);
        project.enums.extend(extra_project.enums);
        for (k, v) in extra_project.call_map {
            project.call_map.entry(k).or_default().extend(v);
        }
        project.call_sites.extend(extra_project.call_sites);
        project.parse_errors.extend(extra_project.parse_errors);
    }

    project.rust_files.sort();
    project.rust_files.dedup();

    for parse_error in &project.parse_errors {
        eprintln!("{}", parse_error);
    }

    if !args.absolute_paths {
        relativize_project_paths(&mut project, &args.path, &args.also);
    }

    crate::index::set_root_hint(args.path.clone());


    if !matches!(
        mode,
        ExecutionMode::Ensemble(_) | ExecutionMode::Callers(_) | ExecutionMode::CallGraph { .. }
    ) && let Some(search_terms) = &args.search
    {
        project.apply_search_filter_with_options(
            search_terms,
            args.search_threshold,
            args.match_signature,
        );
    }


    let changed_ranges = if args.changed {
        let git_ref = args
            .since
            .clone()
            .unwrap_or_else(|| changed::DEFAULT_SINCE.to_string());
        let map = changed::build_changed_ranges(&args.path, &git_ref)?;
        if map.is_empty() {
            eprintln!(
                "note: 0 files changed since {} — subcommand will return empty results",
                git_ref
            );
        }
        Some(map)
    } else {
        None
    };

    run::execute(&args, &project, mode, changed_ranges.as_ref())
}
