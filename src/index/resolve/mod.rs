//! Call-target resolution: maps raw callee names to stable function IDs using
//! module-path inference, qualifier matching, and ambiguity-breaking heuristics.

mod ambiguous;
mod module_paths;
mod targets;

pub(crate) use ambiguous::{
    resolve_ambiguous_call_target, resolve_ambiguous_call_target_for_functions,
};
pub(crate) use module_paths::{
    build_function_module_lookup, infer_file_crate_key, infer_file_module_segments,
};
pub use targets::resolve_call_targets;
