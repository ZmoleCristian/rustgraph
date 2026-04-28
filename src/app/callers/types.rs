//! Data types representing the structured callers report tree.

use crate::FunctionInfo;
use serde::Serialize;

use super::context::ContextLine;

/// A single location in a caller function where the target is invoked.
#[derive(Debug, Clone, Serialize)]
pub struct CallerSite {
    /// 1-based line number of the call expression.
    pub line: usize,
    /// 0-based column offset of the call expression.
    pub column: usize,
    /// Raw text of the source line containing the call.
    pub line_text: String,
    /// Optional surrounding source lines for display context.
    pub context_lines: Vec<ContextLine>,
}

/// A function that calls the target, together with the specific call sites it contributes.
#[derive(Debug, Clone, Serialize)]
pub struct CallerFunction {
    /// Metadata for the calling function.
    pub info: FunctionInfo,
    /// Total number of call sites this function contributes (may exceed `call_sites.len()` if
    /// results were capped).
    pub call_sites_total: usize,
    /// Detailed call-site records, sorted by line then column.
    pub call_sites: Vec<CallerSite>,
}

/// All callers for one resolved target function.
#[derive(Debug, Clone, Serialize)]
pub struct CallersMatch {
    /// Metadata for the target (callee) function.
    pub info: FunctionInfo,
    /// Total call-site count across all resolved callers.
    pub call_sites_total: usize,
    /// Number of call sites that could not be attributed to a known function.
    pub unresolved_call_sites: usize,
    /// Per-caller breakdown, sorted by file path then start line.
    pub callers: Vec<CallerFunction>,
}

/// Top-level result returned by `build_callers_report*`.
#[derive(Debug, Clone, Serialize)]
pub struct CallersReport {
    /// The original query string that was resolved.
    pub query: String,
    /// One entry per matching target function found for `query`.
    pub matches: Vec<CallersMatch>,
}
