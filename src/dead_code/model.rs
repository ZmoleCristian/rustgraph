//! Data model for the dead-code subsystem.
//!
//! Exposes the core types (`DeadCodeReport`, `DeadFunction`, etc.) and re-exports
//! internal helpers used across report assembly, rust-analyzer integration, and
//! text-based reference detection.

use std::collections::HashMap;

/// Name of the `rust-analyzer` binary looked up on `PATH`.
pub(super) const RUST_ANALYZER_BIN: &str = "rust-analyzer";

/// A single reference location: (file path, 1-based line, UTF-16 column).
pub(super) type ReferenceLocation = (String, usize, usize);

/// Key that uniquely identifies a function for reference-count look-up: (file path, start line).
pub(super) type ReferenceCountKey = (String, usize);

/// Map from a function key to its accumulated reference count.
pub(super) type ReferenceCounts = HashMap<ReferenceCountKey, usize>;

/// Pair of reference-count maps: `(non_test_counts, test_only_counts)`.
pub(super) type ReferenceCountMaps = (ReferenceCounts, ReferenceCounts);

#[path = "model/file.rs"]
pub(super) mod file;
#[path = "model/helpers.rs"]
pub(super) mod helpers;
#[path = "model/path.rs"]
pub(super) mod path;
#[path = "model/ranges.rs"]
pub(super) mod ranges;
#[path = "model/types.rs"]
pub(super) mod types;

pub use types::AmbiguousNameDetail;
pub use types::DeadCodeReport;
pub use types::DeadCodeSkippedTotals;
pub use types::DeadFunction;

pub(super) use file::{find_function_name_position, read_file_lines_cached};
pub(super) use helpers::{is_generated_path, is_test_helper_name, is_test_path};
pub(super) use path::{path_to_file_uri, uri_to_file_path};
pub(super) use ranges::{
    collect_cfg_test_ranges, collect_cfg_test_ranges_from_ast, collect_macro_rules_ranges,
    collect_macro_rules_ranges_from_ast, line_in_ranges,
};
