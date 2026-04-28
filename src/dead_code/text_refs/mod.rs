//! Text-based reference detectors that complement AST call-site analysis.
//!
//! Catches patterns that the AST visitor cannot resolve statically:
//! - `handlers` — framework route registrations and functional combinator arguments
//! - `macros` — identifiers referenced inside macro invocations
//! - `reexports` — names re-exported via `pub use` in `lib.rs` / `mod.rs`
//! - `textual` — token-scan fallback for direct and argument-position call references

mod handlers;
mod macros;
mod parsing;
mod reexports;
mod textual;

pub(super) use handlers::{extract_framework_handler_refs, extract_functional_arg_refs};
pub(super) use macros::{extract_macro_function_refs, extract_macro_identifier_refs};
pub(super) use reexports::collect_public_reexport_names;
pub(super) use textual::{collect_textual_function_argument_refs, collect_textual_function_refs};
