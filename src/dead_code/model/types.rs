//! Output types for the dead-code report.

use crate::FunctionInfo;
use serde::Serialize;

/// A public function that was determined to have zero non-test call sites.
#[derive(Debug, Clone, Serialize)]
pub struct DeadFunction {
    /// Full metadata about the function as extracted by the AST visitor.
    pub info: FunctionInfo,
    /// Number of non-test call sites observed across the entire project.
    pub call_sites_non_test_total: usize,
    /// Number of call sites that appear exclusively inside `#[cfg(test)]` or test files.
    pub call_sites_test_only_total: usize,
}

/// Diagnostic detail for a function name that maps to multiple candidate definitions.
///
/// When a call-site token matches more than one `pub fn` with the same base name,
/// the analyser cannot resolve it unambiguously and records this entry instead of
/// falsely marking any of the candidates as dead.
#[derive(Debug, Clone, Serialize)]
pub struct AmbiguousNameDetail {
    /// The shared function name that is ambiguous.
    pub symbol: String,
    /// Number of non-test call sites that could not be attributed to a specific candidate.
    pub unresolved_non_test_call_sites_total: usize,
    /// Number of candidate definitions sharing this name.
    pub candidates_total: usize,
    /// Human-readable `"file:line"` strings for each candidate definition.
    pub candidate_locations: Vec<String>,
}

/// Counts of candidates that were excluded from the dead-code result set.
#[derive(Debug, Clone, Serialize)]
pub struct DeadCodeSkippedTotals {
    /// Functions skipped because they appear in a `pub use` re-export.
    pub reexported_total: usize,
    /// Functions skipped because they live inside a `#[cfg(test)]` block.
    pub cfg_test_total: usize,
    /// Functions skipped because they carry a runtime-entrypoint attribute (e.g. `#[tokio::main]`).
    pub runtime_entrypoints_total: usize,
    /// Functions skipped due to an ambiguous shared name that could not be resolved.
    pub ambiguous_name_total: usize,
    /// Functions skipped because they are passed as handlers to routing/functional-combinator calls.
    pub framework_handler_refs_total: usize,
}

/// Top-level result of a dead-code analysis run.
#[derive(Debug, Clone, Serialize)]
pub struct DeadCodeReport {
    /// Functions with zero observed non-test call sites.
    pub dead_functions: Vec<DeadFunction>,
    /// Total number of `pub fn` candidates evaluated before any skipping.
    pub candidates_total: usize,
    /// Number of entries in `dead_functions`.
    pub dead_functions_total: usize,
    /// Breakdown of candidates removed before the dead/live decision.
    pub skipped: DeadCodeSkippedTotals,
    /// Per-symbol detail for ambiguous names that could not be resolved.
    pub ambiguous_name_details: Vec<AmbiguousNameDetail>,
}
