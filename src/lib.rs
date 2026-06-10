//! AST-aware Rust codebase navigation library and CLI.
//!
//! `rustgraph` parses Rust source files with `syn` and exposes structured
//! symbol information — functions, structs, enums, and call-graphs — through
//! both a CLI and an MCP server for AI assistants.
//!
//! # Main entry points for library consumers
//!
//! - [`api`] — stable serializable types shared across the crate and with
//!   downstream consumers.
//! - [`index`] — re-exported parse and search primitives (`FunctionInfo`,
//!   `StructInfo`, `ProjectAnalysis`, `search_items_with_options`, …).
//! - [`project::ProjectData`] — high-level loader that walks a crate root,
//!   parses every `.rs` file, and resolves alias/re-export edges.
//! - [`dead_code`] — dead-public-function detection and reporting.
//! - [`mcp`] — MCP server handler (stdio JSON-RPC) for Claude / Codex /
//!   Gemini integration.
//! - [`mcp_install`] — self-registration helpers that write the MCP entry
//!   into each detected AI client config.

#![allow(dead_code, unused_imports, unused_mut, unused_variables)]

/// Stable serializable types consumed by library users and downstream crates.
pub mod api;
/// Application dispatch layer: wires parsed [`cli::Args`] to analysis backends.
pub mod app;
/// Clap-derived CLI structs and enums for the `rustgraph` binary.
pub mod cli;
/// Project-level loader that aggregates per-file parse results into a single
/// queryable [`project::ProjectData`].
pub mod project;
/// Symbol search helpers (fuzzy matching, filter pipelines).
pub mod query;

/// MCP (Model Context Protocol) stdio server exposing rustgraph tools to AI
/// clients.
pub mod mcp;
/// Self-registration helpers for injecting the rustgraph MCP entry into
/// Claude, Codex, and Gemini config files.
pub mod mcp_install;

mod source_refs;

/// Dead-code detection: identifies public functions with no non-test callers.
#[path = "dead_code.rs"]
pub mod dead_code;

/// Core parse and index layer: symbol types, file walkers, and search
/// primitives.
#[path = "index/mod.rs"]
pub mod index;

pub use index::*;

/// Returns the crate version string from `Cargo.toml` at compile time.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_cargo_pkg_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn version_is_non_empty_semver_like_string() {
        let v = version();
        assert!(!v.is_empty(), "version must not be empty");

        assert!(v.contains('.'), "expected dotted version, got {v}");
    }
}
