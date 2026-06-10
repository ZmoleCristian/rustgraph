//! Internal helper sub-modules for building ensemble views.
//!
//! All public-facing items in this file are `pub(super)` and are only available
//! to the `ensemble` and `ensemble_lifecycle` modules.

mod code;
mod lifecycle;
mod related;
mod sections;
mod structs;
mod types;

pub(super) use code::{extract_code_snippet, parse_file_line_query, parse_symbol_id_query};
pub(super) use lifecycle::lifecycle_hop_from_function;
pub(super) use related::{
    collect_related_functions, dedup_map_values, related_from_function, sort_related,
};
pub(super) use structs::collect_used_structs;
pub(super) use types::{
    function_consumes_type, function_mentions_type, function_produces_type, normalize_type_query,
};

#[allow(unused_imports)]
pub(super) use sections::{EnsembleSectionFlags, parse_ensemble_sections};
#[allow(unused_imports)]
pub(super) use types::contains_identifier_token;
