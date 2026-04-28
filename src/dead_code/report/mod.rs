//! Dead-code report assembly and rendering.
//!
//! - `assemble` — orchestrates candidate collection, reference counting, and filtering.
//! - `entrypoint` — detects runtime-entrypoint attributes and external call patterns.
//! - `render` — formats a `DeadCodeReport` as human-readable text.

pub mod assemble;
pub mod entrypoint;
pub mod render;

pub use assemble::build_dead_code_report;
pub use render::{
    print_dead_code_text, print_dead_code_text_with_verbosity,
    print_dead_code_text_with_verbosity_and_cap,
};
