//! `syn` AST visitor that records functions, structs, enums, and call sites
//! from a parsed Rust source file.

mod calls;
pub(crate) mod metadata;
mod state;
mod traversal;

pub(crate) use calls::extract_call_target_from_call;
pub(crate) use metadata::{attrs_imply_cfg_test, make_function_id, render_cfg_attrs};
pub(crate) use state::CodeVisitor;
