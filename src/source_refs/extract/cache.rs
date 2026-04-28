//! Per-call file-line cache to avoid re-reading files during a single extraction pass.

use std::collections::HashMap;

/// In-memory map from file path to its lines, used to avoid redundant `read_to_string` calls.
pub(crate) type FileLinesCache = HashMap<String, Vec<String>>;
