use super::super::changed::{ChangedRanges, fn_was_changed};
use super::super::project::ProjectData;
use crate::cli::Args;
use crate::dead_code;

use super::switchboard::write_string_output;

/// Implements `rustgraph dead-code` — identifies `pub` functions that are never called within the
/// project's parsed call graph.
///
/// Optionally restricts output to a path substring (`in_path`), strips test functions
/// (`--exclude-tests`), intersects with changed hunks (`--changed`), and caps the result list
/// with `max_results`. Emits filter-specific notes to stderr when every candidate is eliminated
/// by a filter. Outputs JSON or plain text depending on `--json`.
pub fn run(
    args: &Args,
    project: &ProjectData,
    in_path: Option<String>,
    verbose: bool,
    max_results: usize,
    changed: Option<&ChangedRanges>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut report = dead_code::build_dead_code_report(
        &project.functions,
        &project.call_sites,
        &project.rust_files,
        &args.path,
        Some(project),
    )?;


    if let Some(needle) = &in_path {
        let before = report.dead_functions.len();
        report
            .dead_functions
            .retain(|f| f.info.file_path.contains(needle));
        let after = report.dead_functions.len();
        eprintln!(
            "--in '{}' filter: {} → {} dead fn(s)",
            needle, before, after
        );
        if before > 0 && after == 0 {
            eprintln!(
                "NOTE: --in '{}' filtered all {} dead fn(s) out. Globally there are {} dead fns; none match the path filter.",
                needle, before, before
            );
            eprintln!(
                "Hint: re-run without --in to see them, or widen the path substring."
            );
        }


        report.dead_functions_total = after;
    }

    if args.exclude_tests {
        let before = report.dead_functions.len();
        report.dead_functions.retain(|f| !f.info.is_test);
        let after = report.dead_functions.len();
        eprintln!(
            "--exclude-tests filter: {} → {} dead fn(s) (dropped test fns)",
            before, after
        );
        if before > 0 && after == 0 {
            eprintln!(
                "NOTE: --exclude-tests filtered all {} dead fn(s) out. All dead fns were #[test]/#[cfg(test)] items.",
                before
            );
            eprintln!(
                "Hint: re-run without --exclude-tests to see them."
            );
        }
        report.dead_functions_total = after;
    }


    if let Some(ranges) = changed {
        let before = report.dead_functions.len();
        report.dead_functions.retain(|f| {
            fn_was_changed(&f.info.file_path, f.info.start_line, f.info.end_line, ranges)
        });
        let after = report.dead_functions.len();
        eprintln!(
            "--changed filter: {} → {} dead fn(s) (kept only PR-touched)",
            before, after
        );
        if before > 0 && after == 0 {
            eprintln!(
                "NOTE: --changed filtered all {} dead fn(s) out. None of the dead fns overlap a changed hunk.",
                before
            );
            eprintln!(
                "Hint: re-run without --changed to see all dead fns project-wide."
            );
        }
        report.dead_functions_total = after;
    }

    if args.json {
        let payload = serde_json::to_string_pretty(&report)?;
        write_string_output(args.output.as_deref(), &payload)?;
    } else {


        dead_code::print_dead_code_text_with_verbosity_and_cap(&report, verbose, max_results);
    }

    Ok(())
}
