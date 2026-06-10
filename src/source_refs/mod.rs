//! Call-argument extraction subsystem used by ensemble dataflow analysis and
//! dead-code text-reference parsing.
//!
//! Given a file path and line/column hint, locates the argument list of a
//! specific call expression and returns an individual argument by index.
//! Includes normalization utilities for handler expressions and an in-process
//! file-line cache to avoid redundant disk reads.

mod args;
mod extract;
mod normalize;

pub(crate) use extract::{clamp_to_char_boundary, extract_nth_call_arg_segment_from_file};
pub(crate) use normalize::normalize_handler_expr;
