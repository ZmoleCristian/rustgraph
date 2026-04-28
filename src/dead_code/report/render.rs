//! Text rendering for dead-code reports.

use super::super::model::{DeadCodeReport, DeadFunction};
use std::collections::HashMap;

/// Print `report` to stdout with full verbosity.
pub fn print_dead_code_text(report: &DeadCodeReport) {
    print_dead_code_text_with_verbosity(report, true);
}


/// Print `report` to stdout, controlling summary detail via `verbose`.
///
/// When `verbose` is `false` the skip-reason breakdown is omitted from the summary line.
pub fn print_dead_code_text_with_verbosity(report: &DeadCodeReport, verbose: bool) {
    print_dead_code_text_with_verbosity_and_cap(report, verbose, 0)
}


/// Print `report` to stdout with verbosity control and an optional result cap.
///
/// Results are sorted by file path then start line.  When `max_results` is non-zero
/// only the first `max_results` entries are displayed and a truncation notice is printed.
pub fn print_dead_code_text_with_verbosity_and_cap(
    report: &DeadCodeReport,
    verbose: bool,
    max_results: usize,
) {
    println!("=== DEAD PUBLIC FUNCTIONS (0 call sites) ===\n");
    let summary_line = build_summary_line(report, verbose);
    if report.dead_functions.is_empty() {
        println!("(none)");
        println!("\n{}", summary_line);
        return;
    }

    let total = report.dead_functions.len();
    let cap = if max_results == 0 {
        usize::MAX
    } else {
        max_results
    };


    let mut sorted: Vec<&DeadFunction> = report.dead_functions.iter().collect();
    sorted.sort_by(|a, b| {
        a.info
            .file_path
            .cmp(&b.info.file_path)
            .then_with(|| a.info.start_line.cmp(&b.info.start_line))
    });
    let truncated = total > cap;
    let display: Vec<&DeadFunction> = sorted.into_iter().take(cap).collect();

    let mut by_file: HashMap<&str, Vec<&DeadFunction>> = HashMap::new();
    for item in &display {
        by_file.entry(&item.info.file_path).or_default().push(*item);
    }

    let mut files: Vec<&str> = by_file.keys().cloned().collect();
    files.sort();

    for file in files {
        println!("{file}:");
        let mut entries = by_file[file].clone();
        entries.sort_by_key(|e| e.info.start_line);
        for e in entries {
            let head = if e.info.is_async {
                format!("pub async fn {}", e.info.name)
            } else {
                format!("pub fn {}", e.info.name)
            };
            let tail = if e.call_sites_test_only_total > 0 {
                format!("— 0 calls ({} test-only)", e.call_sites_test_only_total)
            } else {
                "— 0 calls".to_string()
            };
            println!("  :{:<4} {:<42} {}", e.info.start_line, head, tail);
        }
        println!();
    }

    if truncated {
        println!(
            "(showing {} of {}; use --max-results to expand)",
            cap, total
        );
        println!();
    }

    println!("{}", summary_line);


    if verbose && !report.ambiguous_name_details.is_empty() {
        println!("Ambiguous unresolved call targets:");
        for item in report.ambiguous_name_details.iter().take(10) {
            println!(
                "  {}: {} unresolved calls across {} candidates",
                item.symbol, item.unresolved_non_test_call_sites_total, item.candidates_total
            );
        }
    }
}

fn build_summary_line(report: &DeadCodeReport, verbose: bool) -> String {
    if verbose {
        format!(
            "Summary: {} candidates, {} dead, {} skipped as re-exported, {} skipped inside cfg(test), {} skipped as runtime entrypoints, {} skipped due to ambiguous names, {} skipped as framework handler refs",
            report.candidates_total,
            report.dead_functions_total,
            report.skipped.reexported_total,
            report.skipped.cfg_test_total,
            report.skipped.runtime_entrypoints_total,
            report.skipped.ambiguous_name_total,
            report.skipped.framework_handler_refs_total
        )
    } else {
        format!(
            "Summary: {} candidates, {} dead (pass --verbose for skip-reason breakdown)",
            report.candidates_total, report.dead_functions_total
        )
    }
}
