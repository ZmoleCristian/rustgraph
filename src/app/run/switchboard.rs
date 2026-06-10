use std::fs;

use super::super::changed::ChangedRanges;
use super::super::modes::ExecutionMode;
use super::super::project::ProjectData;
use super::call_graph;
use super::callers;
use super::dead_code;
use super::def;
use super::ensemble;
use super::find;
use super::grep;
use super::impls;
use super::inventory;
use super::members;
use super::paths_between;
use super::refs;
use super::tree;
use super::usages;
use crate::cli::Args;

/// Dispatch an `ExecutionMode` to the matching subcommand handler.
///
/// Every CLI subcommand ultimately passes through here after project loading and mode resolution.
/// `changed` carries the diff range map when `--changed` filtering is active; pass `None` to skip it.
pub fn execute(
    args: &Args,
    project: &ProjectData,
    mode: ExecutionMode,
    changed: Option<&ChangedRanges>,
) -> Result<(), Box<dyn std::error::Error>> {
    match mode {
        ExecutionMode::Ensemble(request) => ensemble::run(args, project, request),
        ExecutionMode::Callers(request) => callers::run(args, project, request, changed),
        ExecutionMode::CallGraph { detail, config } => call_graph::run(args, project, detail, config),
        ExecutionMode::DeadCode { in_path, verbose, max_results } => dead_code::run(args, project, in_path, verbose, max_results, changed),
        ExecutionMode::Inventory { selection } => inventory::run(args, project.clone(), selection),
        ExecutionMode::Tree(request) => tree::run(args, project, request, changed),
        ExecutionMode::Grep(request) => grep::run(args, project, request, changed),
        ExecutionMode::Impls(request) => impls::run(args, project, request, changed),
        ExecutionMode::Find(request) => find::run(args, project, request, changed),
        ExecutionMode::Refs(request) => refs::run(args, project, request, changed),
        ExecutionMode::PathsBetween(request) => paths_between::run(args, project, request),
        ExecutionMode::Usages(request) => usages::run(args, project, request),
        ExecutionMode::Def(request) => def::run(args, project, request),
        ExecutionMode::Members(request) => members::run(args, project, request),
    }
}

/// Steering footer appended to locator-tool text output (find, callers).
///
/// These tools answer "where is X" / "who calls X" with `file:line` locations,
/// not source. The hint nudges the model to open those locations with the Read
/// tool rather than reaching for a code-dumping substitute.
pub(super) const READ_TOOL_HINT: &str = "↳ Use the Read tool to read functions of interest.";

/// Write `payload` to `output_path` when provided, or print to stdout when `None`.
///
/// Used uniformly by all subcommand handlers to honour the `-o`/`--output` flag.
pub fn write_string_output(
    output_path: Option<&std::path::Path>,
    payload: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = output_path {
        fs::write(path, payload)?;
    } else {
        println!("{}", payload);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_string_output_writes_to_file_when_path_given() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("out.txt");
        write_string_output(Some(&target), "hello").expect("write");
        let read = fs::read_to_string(&target).expect("read");
        assert_eq!(read, "hello");
    }

    #[test]
    fn write_string_output_returns_err_when_dir_missing() {
        let result =
            write_string_output(Some(std::path::Path::new("/nonexistent/dir/out.txt")), "x");
        assert!(result.is_err());
    }

    #[test]
    fn write_string_output_succeeds_with_no_path() {

        write_string_output(None, "stdout-message").expect("ok");
    }
}
