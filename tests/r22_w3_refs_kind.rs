

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::{TempDir, tempdir};

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, content).expect("write fixture file");
}

fn run_rustgraph(args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let mut full: Vec<&str> = vec!["--absolute-paths", "--no-auto-path"];
    full.extend_from_slice(args);
    Command::new(bin).args(full).output().expect("run rustgraph")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}


fn build_fixture() -> TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"// Synthetic R22-W3 fixture exercising all 4 priority --kind tags.

pub struct Container { pub foo: u32 }

pub fn read_field(c: &Container) -> u32 { c.foo }
pub fn read_field_again(c: &Container) -> u32 { c.foo + 1 }

pub mod helpers { pub fn foo() {} }

pub fn use_path() { let _ = helpers::foo; }
pub fn use_path_again() { let _ = helpers::foo; }

pub struct StructHost;

impl StructHost {
    pub fn foo() {}
}

pub fn use_assoc() { StructHost::foo(); }
pub fn use_assoc_again() { StructHost::foo(); }

pub enum Color { Foo, Bar }

pub fn use_variant() -> Color { Color::Foo }
pub fn use_variant_again() -> Color { Color::Bar }
"#,
    );
    write_file(
        &dir.path().join("Cargo.toml"),
        r#"[package]
name = "r22_w3_fixture"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
"#,
    );
    dir
}

fn refs_kinds(out: &Output) -> Vec<String> {
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    json["refs"]
        .as_array()
        .expect("refs array")
        .iter()
        .map(|r| r["kind"].as_str().unwrap_or("").to_string())
        .collect()
}


#[test]
fn refs_no_kind_filter_returns_multiple_kinds() {
    let dir = build_fixture();
    let path = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &path, "refs", "foo", "-j"]);
    assert!(
        out.status.success(),
        "refs foo failed; stderr={}",
        stderr_of(&out)
    );
    let kinds = refs_kinds(&out);
    assert!(!kinds.is_empty(), "expected refs hits; got none");

    let unique: std::collections::HashSet<&str> = kinds.iter().map(String::as_str).collect();
    assert!(unique.contains("field"), "expected `field` kind in: {kinds:?}");
    assert!(unique.contains("path"), "expected `path` kind in: {kinds:?}");
    assert!(
        unique.len() >= 2,
        "expected multiple kinds (no filter); got: {unique:?}"
    );
}


#[test]
fn refs_kind_field_filters_to_only_fields() {
    let dir = build_fixture();
    let path = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &path, "refs", "foo", "--kind", "field", "-j"]);
    assert!(
        out.status.success(),
        "refs --kind field failed; stderr={}",
        stderr_of(&out)
    );
    let kinds = refs_kinds(&out);
    assert!(!kinds.is_empty(), "expected field refs; got none");
    assert!(
        kinds.iter().all(|k| k == "field"),
        "expected only `field` kind; got: {kinds:?}"
    );
}


#[test]
fn refs_kind_multi_value_unions_allowed_kinds() {
    let dir = build_fixture();
    let path = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path", &path, "refs", "foo", "--kind", "field", "--kind", "path", "-j",
    ]);
    assert!(
        out.status.success(),
        "refs --kind field --kind path failed; stderr={}",
        stderr_of(&out)
    );
    let kinds = refs_kinds(&out);
    assert!(!kinds.is_empty(), "expected refs hits; got none");

    let unique: std::collections::HashSet<&str> = kinds.iter().map(String::as_str).collect();
    assert!(unique.contains("field"), "expected field; got: {unique:?}");
    assert!(unique.contains("path"), "expected path; got: {unique:?}");
    assert!(
        kinds.iter().all(|k| k == "field" || k == "path"),
        "expected only field/path; got: {kinds:?}"
    );
}


#[test]
fn refs_kind_invalid_value_errors_with_possible_values() {
    let dir = build_fixture();
    let path = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &path, "refs", "foo", "--kind", "bogus_kind"]);
    assert!(
        !out.status.success(),
        "expected non-zero exit for invalid --kind value; stdout={}, stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("invalid value 'bogus_kind'"),
        "expected `invalid value 'bogus_kind'` in stderr; got:\n{stderr}"
    );

    for expected in ["field", "path", "assoc_call", "variant"] {
        assert!(
            stderr.contains(expected),
            "expected `{expected}` in possible-values list; got:\n{stderr}"
        );
    }
}


#[test]
fn refs_kind_filter_applies_to_json_output() {
    let dir = build_fixture();
    let path = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &path, "refs", "foo", "--kind", "path", "-j"]);
    assert!(
        out.status.success(),
        "refs --kind path -j failed; stderr={}",
        stderr_of(&out)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    let refs = json["refs"].as_array().expect("refs array");
    assert!(!refs.is_empty(), "expected at least one path ref; got none");

    for entry in refs {
        let kind = entry["kind"].as_str().unwrap_or("");
        assert_eq!(
            kind, "path",
            "every entry's `kind` must be `path` under --kind path; got entry: {entry:#?}"
        );
    }
}


#[test]
fn refs_audit_all_four_priority_kinds_fire_on_fixture() {
    let dir = build_fixture();
    let path = dir.path().to_string_lossy().to_string();

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for query in &["foo", "Color", "StructHost"] {
        let out = run_rustgraph(&["--path", &path, "refs", query, "-j"]);
        assert!(
            out.status.success(),
            "refs {query} failed; stderr={}",
            stderr_of(&out)
        );
        for k in refs_kinds(&out) {
            seen.insert(k);
        }
    }

    for required in ["field", "path", "assoc_call", "variant"] {
        assert!(
            seen.contains(required),
            "audit: expected priority kind `{required}` to fire on fixture; saw: {seen:?}"
        );
    }
}


#[test]
fn refs_kind_variant_filters_enum_variants_only() {
    let dir = build_fixture();
    let path = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &path, "refs", "Color", "--kind", "variant", "-j"]);
    assert!(
        out.status.success(),
        "refs Color --kind variant failed; stderr={}",
        stderr_of(&out)
    );
    let kinds = refs_kinds(&out);
    assert!(!kinds.is_empty(), "expected variant refs; got none");
    assert!(
        kinds.iter().all(|k| k == "variant"),
        "expected only `variant` kind; got: {kinds:?}"
    );
}


#[test]
fn refs_kind_assoc_call_filters_struct_assoc_fns_only() {
    let dir = build_fixture();
    let path = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path", &path, "refs", "StructHost", "--kind", "assoc_call", "-j",
    ]);
    assert!(
        out.status.success(),
        "refs StructHost --kind assoc_call failed; stderr={}",
        stderr_of(&out)
    );
    let kinds = refs_kinds(&out);
    assert!(!kinds.is_empty(), "expected assoc_call refs; got none");
    assert!(
        kinds.iter().all(|k| k == "assoc_call"),
        "expected only `assoc_call` kind; got: {kinds:?}"
    );
}


#[test]
fn refs_help_lists_all_priority_kinds() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = Command::new(bin)
        .args(["refs", "--help"])
        .output()
        .expect("run refs --help");
    assert!(out.status.success(), "refs --help failed");
    let stdout = stdout_of(&out);
    for kind in ["field", "path", "assoc_call", "variant"] {
        assert!(
            stdout.contains(kind),
            "refs --help should list `{kind}`; got:\n{stdout}"
        );
    }
}
