#[path = "dead_code/model.rs"]
mod model;
#[path = "dead_code/report/mod.rs"]
mod report;
#[path = "dead_code/rust_analyzer/mod.rs"]
mod rust_analyzer;
#[path = "dead_code/text_refs/mod.rs"]
mod text_refs;

/// A public function that has zero non-test call sites in the indexed crate.
pub use model::DeadFunction;

/// Per-name disambiguation metadata for a symbol name that matched multiple
/// candidate functions during dead-code analysis, preventing a definitive
/// verdict.
pub use model::AmbiguousNameDetail;

/// Aggregate counts of symbols skipped during dead-code analysis, broken down
/// by skip reason (re-exported, inside `#[cfg(test)]`, runtime entrypoints,
/// ambiguous names, framework-handler references).
pub use model::DeadCodeSkippedTotals;

/// Complete result of a dead-code analysis run: list of dead functions, total
/// candidate count, skip-reason totals, and ambiguous-name details.
pub use model::DeadCodeReport;

/// Run the full dead-code analysis pipeline and return a [`DeadCodeReport`].
///
/// Accepts the parsed symbol tables and call-site list from
/// [`crate::project::ProjectData`]. Returns an error only if an underlying
/// I/O operation (e.g. reading source lines for reference counting) fails.
pub use report::build_dead_code_report;

/// Print a [`DeadCodeReport`] to stdout in human-readable text format with
/// verbose skip-reason summary enabled.
pub use report::print_dead_code_text;

/// Print a [`DeadCodeReport`] to stdout with configurable verbosity.
///
/// When `verbose` is `false`, the skip-reason summary line is omitted.
pub use report::print_dead_code_text_with_verbosity;

/// Print a [`DeadCodeReport`] to stdout with configurable verbosity and a cap
/// on the number of dead functions rendered.
///
/// `max_results = 0` means unlimited. When the list is truncated a footer
/// notes the original count; the summary header is always un-capped.
pub use report::print_dead_code_text_with_verbosity_and_cap;
