//! AST parser and symbol indexer for Rust source trees.
//!
//! This subsystem walks a file tree, parses each `.rs` file with `syn`,
//! and produces a queryable index of functions, structs, enums, and call
//! sites.  Higher-level features (call-graph, ensemble views, lifecycle
//! analysis) are built on top of that flat index.

mod ensemble;
mod ensemble_flow;
mod ensemble_helpers;
mod ensemble_lifecycle;
mod graph;
mod parse;
pub mod qualifier;
mod render;
mod resolve;
mod scan;
mod search;
mod types;
mod visitor;

pub use ensemble::*;
pub use graph::{EdgeSemantics, TypeInfo, TypedEdge, generate_call_graph};
pub use parse::*;
pub use qualifier::{candidate_matches_qualifier, qualifier_for_callee};
pub use render::*;
pub use resolve::resolve_call_targets;
pub use search::*;
pub use types::*;

pub(crate) use resolve::{
    build_function_module_lookup, infer_file_crate_key, infer_file_module_segments,
    resolve_ambiguous_call_target_for_functions,
};
pub(crate) use ensemble_flow::collect_framework_handler_refs;

use std::path::PathBuf;
use std::sync::OnceLock;
static ROOT_HINT: OnceLock<std::sync::RwLock<Option<PathBuf>>> = OnceLock::new();


/// Override the project root used when resolving relative file paths.
///
/// Must be called before any path-sensitive analysis.  Subsequent calls
/// replace the previously stored root.
pub fn set_root_hint(root: PathBuf) {
    let lock = ROOT_HINT.get_or_init(|| std::sync::RwLock::new(None));
    if let Ok(mut guard) = lock.write() {
        *guard = Some(root);
    }
}

/// Return the currently stored root hint, or `None` if none has been set.
pub fn resolve_root_hint() -> Option<PathBuf> {
    ROOT_HINT
        .get()
        .and_then(|lock| lock.read().ok().and_then(|g| g.clone()))
}


/// Resolve a potentially relative file path to an absolute `PathBuf`.
///
/// Tries the path as-is, then resolves it against the root hint, and as a
/// last resort against the parent of the root hint (useful for multi-crate
/// repos where the tool root and the file root differ by one directory level).
pub fn resolve_read_path(p: &str) -> PathBuf {
    let pb = std::path::Path::new(p);
    if pb.is_absolute() && pb.exists() {
        return pb.to_path_buf();
    }
    if let Some(root) = resolve_root_hint() {
        let direct = root.join(pb);
        if direct.exists() {
            return direct;
        }
        if let Some((label, rest)) = p.split_once('/')
            && let Some(par) = root.parent()
        {
            let alt = par.join(label).join(rest);
            if alt.exists() {
                return alt;
            }
        }
    }
    if pb.exists() {
        return pb.to_path_buf();
    }
    pb.to_path_buf()
}
