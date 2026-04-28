//! Optional rust-analyzer LSP integration for precise reference counting.
//!
//! When `rust-analyzer` is available on `PATH`, this module starts it as an LSP
//! subprocess and queries `textDocument/references` for each candidate function,
//! producing per-location reference counts that the main report assembly merges
//! with the text-based counts.

mod client;
mod collector;
mod protocol;

/// Query rust-analyzer for reference counts across all `candidates`.
///
/// Re-exported from `collector` for use by the report-assembly pipeline.
pub(super) use collector::collect_rust_analyzer_reference_counts;
