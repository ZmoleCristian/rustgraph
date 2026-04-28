//! Re-exports of source-parsing utilities from `crate::source_refs`.
//!
//! Centralises the import so all `text_refs` sub-modules use a single, stable path.

/// Re-export: clamp a byte offset to a valid UTF-8 character boundary.
pub(super) use crate::source_refs::clamp_to_char_boundary;
/// Re-export: extract the nth argument expression from a call site in a source file.
pub(super) use crate::source_refs::extract_nth_call_arg_segment_from_file as extract_call_arg_segment_from_file;
/// Re-export: normalise a handler expression string into `(full_callee, base_name)`.
pub(super) use crate::source_refs::normalize_handler_expr;
