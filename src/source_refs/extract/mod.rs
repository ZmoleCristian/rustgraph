//! Internal extraction pipeline: locate a call marker in source, extract its
//! argument body, and split out a specific argument.
//!
//! Public surface: [`extract_nth_call_arg_segment_from_file`] and
//! [`clamp_to_char_boundary`].

mod cache;
mod entry;
mod extract_body;
mod find_marker;
mod strip_comment;

pub(crate) use entry::extract_nth_call_arg_segment_from_file;
pub(crate) use find_marker::clamp_to_char_boundary;
