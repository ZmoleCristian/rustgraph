/// Subcommand handler modules; each exposes a `run` entry point invoked by `switchboard::execute`.
pub(super) mod call_graph;
pub(super) mod callers;
pub(super) mod dead_code;
pub(super) mod def;
pub(super) mod ensemble;
pub(super) mod find;
pub(super) mod grep;
pub(super) mod impls;
pub(super) mod inventory;
pub(super) mod members;
pub(super) mod paths_between;
pub(super) mod refs;
pub(super) mod slice;
mod switchboard;
pub(super) mod tree;
pub(super) mod usages;

/// Routes an `ExecutionMode` to the appropriate subcommand handler.
///
/// This is the single public entry point for the `run` layer; callers resolve the mode first
/// and then hand off here.
pub use switchboard::execute;
