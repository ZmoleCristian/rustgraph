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
///
/// The returned path always uses forward slashes regardless of host OS, so
/// downstream consumers (terminal renderers, IDE links, symbol-id strings)
/// see the same shape on Windows and POSIX.
pub(crate) fn relativize_for_display(
    path: &str,
    root: &std::path::Path,
    also_roots: &[std::path::PathBuf],
) -> String {
    let pb = std::path::Path::new(path);
    let canon = std::fs::canonicalize(pb).unwrap_or_else(|_| pb.to_path_buf());
    let abs_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if let Ok(stripped) = canon.strip_prefix(&abs_root) {
        return crate::normalize_path_separators(&stripped.to_string_lossy());
    }
    if let Ok(stripped) = pb.strip_prefix(&abs_root) {
        return crate::normalize_path_separators(&stripped.to_string_lossy());
    }
    if let Ok(stripped) = pb.strip_prefix(root) {
        return crate::normalize_path_separators(&stripped.to_string_lossy());
    }
    for r in also_roots {
        let canon_also = std::fs::canonicalize(r).unwrap_or_else(|_| r.clone());
        let label = canon_also
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "also".to_string());
        if let Ok(stripped) = canon.strip_prefix(&canon_also) {
            return format!(
                "{}/{}",
                label,
                crate::normalize_path_separators(&stripped.to_string_lossy())
            );
        }
        if let Ok(stripped) = pb.strip_prefix(&canon_also) {
            return format!(
                "{}/{}",
                label,
                crate::normalize_path_separators(&stripped.to_string_lossy())
            );
        }
    }
    crate::normalize_path_separators(path.trim_start_matches("./"))
}

/// Split a function id (`{path}:{line}:{name}`) back into its three parts.
///
/// `split_once(':')` is the wrong tool for this on Windows because absolute
/// paths begin with a drive-letter colon (`C:/Users/...`) and the first `:`
/// would land *inside* the path. Walking from the right is unambiguous because
/// the suffix is always `:{line}:{name}` regardless of platform — the line
/// number is a small integer and the name has no colons.
fn split_function_id(id: &str) -> Option<(&str, &str, &str)> {
    let (rest, name) = id.rsplit_once(':')?;
    let (path, line) = rest.rsplit_once(':')?;
    Some((path, line, name))
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
        let new_key = match split_function_id(&key) {
            Some((path_part, line, name)) => {
                format!("{}:{}:{}", strip(path_part), line, name)
            }
            None => key,
        };
        new_call_map.insert(new_key, val);
    }
    project.call_map = new_call_map;

    for cs in project.call_sites.iter_mut() {
        if let Some(id) = cs.caller_id.as_mut() {
            if let Some((path_part, line, name)) = split_function_id(id) {
                *id = format!("{}:{}:{}", strip(path_part), line, name);
            }
        }
    }

    // The parsed-file cache must stay in sync with the file_path strings on
    // FunctionInfo / CallSite (which are now relativized) AND with the raw
    // PathBufs in rust_files (which are not).  Insert relativized aliases
    // pointing at the same AST so both lookup styles hit a cache entry.
    let original_keys: Vec<String> = project.parsed_files_keys();
    for key in original_keys {
        let stripped = strip(&key);
        if stripped != key {
            project.alias_parsed_file(&key, &stripped);
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
        let mut extra_project = ProjectData::load(extra, args.include_ignored);
        eprintln!(
            "--also '{}': merged {} fn(s), {} struct(s), {} enum(s), {} file(s)",
            extra.display(),
            extra_project.functions.len(),
            extra_project.structs.len(),
            extra_project.enums.len(),
            extra_project.rust_files.len()
        );
        let extra_parsed = extra_project.take_parsed_files();
        project.rust_files.extend(extra_project.rust_files);
        project.functions.extend(extra_project.functions);
        project.structs.extend(extra_project.structs);
        project.enums.extend(extra_project.enums);
        // Concatenate callee vecs on collision so cross-crate `--also`
        // merges don't drop edges when two crates happen to land on the
        // same `file:line:name` caller key after relativization.
        for (k, mut v) in extra_project.call_map {
            project.call_map.entry(k).or_default().append(&mut v);
        }
        project.call_sites.extend(extra_project.call_sites);
        project.parse_errors.extend(extra_project.parse_errors);
        project.absorb_parsed_files(extra_parsed);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_function_id_unix_path() {
        let parts = split_function_id("src/foo.rs:42:bar").expect("split should succeed");
        assert_eq!(parts, ("src/foo.rs", "42", "bar"));
    }

    #[test]
    fn split_function_id_windows_relative_with_backslashes() {
        let parts = split_function_id(r"src\foo.rs:42:bar").expect("split should succeed");
        assert_eq!(parts, (r"src\foo.rs", "42", "bar"));
    }

    #[test]
    fn split_function_id_windows_absolute_with_drive_letter() {
        // The bug fix: a previous implementation used `split_once(':')` and
        // would mis-split on the drive-letter colon, producing
        // ("C", "/Users/.../foo.rs:42:bar") and silently breaking every call
        // graph lookup on Windows. `rsplit_once` is OS-agnostic.
        let parts = split_function_id(r"C:\Users\me\proj\src\foo.rs:42:bar")
            .expect("split should succeed");
        assert_eq!(parts, (r"C:\Users\me\proj\src\foo.rs", "42", "bar"));
    }

    #[test]
    fn split_function_id_windows_absolute_forward_slashes() {
        let parts = split_function_id("C:/Users/me/proj/src/foo.rs:42:bar")
            .expect("split should succeed");
        assert_eq!(parts, ("C:/Users/me/proj/src/foo.rs", "42", "bar"));
    }

    #[test]
    fn split_function_id_returns_none_on_short_input() {
        assert!(split_function_id("just_a_name").is_none());
        assert!(split_function_id("name:42").is_none());
    }

    #[test]
    fn split_function_id_handles_unicode_path() {
        let parts = split_function_id("src/Æ_module.rs:7:über").expect("split should succeed");
        assert_eq!(parts, ("src/Æ_module.rs", "7", "über"));
    }
}
