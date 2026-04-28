//! I/O boundary classification and value-flow analysis for ensemble views.
//!
//! Provides heuristic detection of which external systems a function touches
//! (HTTP, database, file, network, queue, cache) and lightweight dataflow
//! hints that trace how parameter values propagate through a function's calls.

mod boundaries;
mod handlers;
mod parsing;
mod value_flow;

pub(in crate::index) use boundaries::classify_io_boundaries_for_function;
pub(crate) use handlers::collect_framework_handler_refs;
pub(in crate::index) use handlers::is_low_signal_unresolved_callee;
pub(in crate::index) use value_flow::build_value_flow_hints;
