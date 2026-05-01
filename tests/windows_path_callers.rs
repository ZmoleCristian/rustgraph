//! Regression tests for the Windows-path callers bug.
//!
//! On Windows, absolute crate roots begin with a drive-letter colon
//! (`C:\Users\...`). A previous implementation of the path-relativization
//! step in `app::mod` used `String::split_once(':')` to break a function id
//! `{path}:{line}:{name}` back into its parts. That call would split on the
//! drive-letter colon instead of the line-number colon, producing
//! `("C", "/Users/.../foo.rs:42:bar")` and silently breaking every caller
//! lookup against `function_by_id`.
//!
//! These tests build a small fixture crate, run `rustgraph callers <fn>`
//! against it, and assert that the resolver reports the calling functions
//! (with `Callers: 1 function(s), 1 total call site(s)` and zero
//! `unresolved`). The tests use the same end-to-end CLI path the MCP server
//! uses, so they exercise the relativization step that previously failed.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write fixture file");
}

fn run_rustgraph(args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let mut full: Vec<&str> = vec!["--no-auto-path"];
    full.extend_from_slice(args);
    Command::new(bin).args(full).output().expect("run rustgraph")
}

#[test]
fn callers_resolve_against_absolute_crate_root() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\n\
         pub fn caller_one() { target(); }\n\
         pub fn caller_two() { target(); target(); }\n",
    );

    // Pass the fixture path as an absolute path so the relativization
    // pipeline runs over a path with a drive-letter colon on Windows. On
    // POSIX the path has no drive letter and the test still verifies the
    // baseline behaviour: callers resolve and 0 are unresolved.
    let absolute = fixture.path().canonicalize().expect("canonicalize fixture");
    let absolute_str = absolute.to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &absolute_str, "callers", "target"]);
    assert!(
        out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("3 total call site(s)"),
        "expected 3 call sites, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("2 function(s)"),
        "expected 2 callers, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("unresolved"),
        "no call site should be unresolved, but stdout mentions unresolved:\n{}",
        stdout
    );
    assert!(
        stdout.contains("caller_one") && stdout.contains("caller_two"),
        "expected caller_one and caller_two in output, got:\n{}",
        stdout
    );
}

#[test]
fn callers_json_does_not_drop_resolved_sites_on_absolute_root() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\n\
         pub fn caller() { target(); }\n",
    );
    let absolute = fixture.path().canonicalize().expect("canonicalize fixture");
    let absolute_str = absolute.to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &absolute_str, "callers", "target", "--json"]);
    assert!(
        out.status.success(),
        "callers --json exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");

    let matches = value["matches"].as_array().expect("matches array");
    assert_eq!(matches.len(), 1, "one match expected, got: {}", stdout);
    let match_entry = &matches[0];

    assert_eq!(
        match_entry["call_sites_total"].as_u64(),
        Some(1),
        "call_sites_total should be 1, got: {}",
        stdout
    );
    assert_eq!(
        match_entry["unresolved_call_sites"].as_u64(),
        Some(0),
        "unresolved_call_sites should be 0 (regression: was 1 on Windows), got: {}",
        stdout
    );
    let callers = match_entry["callers"].as_array().expect("callers array");
    assert_eq!(callers.len(), 1, "one caller expected, got: {}", stdout);
    assert_eq!(
        callers[0]["info"]["name"].as_str(),
        Some("caller"),
        "caller name should be `caller`, got: {}",
        stdout
    );
}
