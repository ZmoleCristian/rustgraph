//! Text rendering layer for all analysis results.
//!
//! Each sub-module converts a typed result struct into human-readable output and
//! prints it to stdout. `render_*` variants return a `String`; `print_*` variants
//! write directly via `print!` / `println!`.

mod callers;
mod ensemble;
mod inventory;

pub use callers::{print_callers_text, render_callers_text};
pub use ensemble::{print_ensemble_text, print_ensemble_text_summary_compact};
pub use inventory::print_inventory_text;
