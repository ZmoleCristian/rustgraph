//! Call-graph types and edge construction for the `callgraph` subcommand.
//!
//! Internally builds type-annotated edges (`build_type_aware_edges`) and
//! exposes DOT/text rendering (`generate_call_graph`) on top of a flat AST
//! index.  Edge construction is `pub(crate)`; only the rendered output and
//! shared edge types are part of the public API.

mod dot;
mod edges;
mod types;

pub use dot::generate_call_graph;
pub use types::{EdgeSemantics, TypeInfo, TypedEdge};
