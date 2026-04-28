//! Ensemble builder: resolves a query to matching functions and assembles
//! the full context bundle consumed by the `ensemble` subcommand.

mod builder;
mod candidates;
mod options;
mod resolution;

pub use builder::build_function_ensemble;
pub use options::FunctionEnsembleOptions;
