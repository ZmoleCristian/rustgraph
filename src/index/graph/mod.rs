//! Call-graph types and edge construction for the `callgraph` subcommand.
//!
//! Exposes type-annotated edge building (`build_type_aware_edges`) and
//! DOT/text rendering (`generate_call_graph`) on top of a flat AST index.

mod dot;
mod edges;
mod types;

pub use dot::generate_call_graph;
pub use edges::{build_type_aware_edges, extract_type_info};
pub use types::{EdgeSemantics, TypeInfo, TypedEdge};
