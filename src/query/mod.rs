//! High-level query API: symbol search, source slicing, project loading, and ensemble/callers/dead-code reports.
//!
//! This module re-exports the main entry points from each sub-module. Consumers
//! import from here rather than from the individual sub-modules.

mod builders;
mod languages;
mod project;
mod source;
mod symbols;

pub use builders::{build_callers, build_dead_code, build_ensemble};
pub use languages::{
    ProjectScopeAssessment, assess_project_scope, display_name_for_path, language_profile,
    quick_language_profile,
};
pub use project::{
    ProjectSummaryInput, cache_path_for_slug, load_project, project_counts, project_summary,
    slug_for_path,
};
pub use source::{file_slice_for_lines, source_slice_for_lines, source_slice_for_symbol};
pub use symbols::{search_symbols, search_symbols_with_options};
