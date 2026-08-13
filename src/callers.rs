//! AST-based callers report builder.
//!
//! Resolves call sites for a queried function via the ensemble pipeline and groups them
//! by calling function, attaching optional source context lines. The result is a
//! [`CallersReport`] that the render layer formats for display or JSON output.

mod context;
mod report;
mod types;

pub use context::ContextLine;
pub use report::{build_callers_report, build_callers_report_with_options};
pub use types::{CallerFunction, CallerSite, CallersMatch, CallersReport, UnresolvedCallSite};
