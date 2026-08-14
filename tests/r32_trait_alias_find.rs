//! R32: traits and type aliases are findable symbols.
//!
//! Closes the second half of the redstring-audit gap class: `find ChangedRanges`
//! (a `pub type` alias) used to hard-error, and `find MyTrait` returned nothing
//! even though `impls` knew the trait. Both are now first-class in the index.

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
    let mut full: Vec<&str> = vec!["--absolute-paths", "--no-auto-path"];
    full.extend_from_slice(args);
    Command::new(bin)
        .args(full)
        .output()
        .expect("run rustgraph")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn fixture(dir: &Path) {
    write_file(
        &dir.join("src/lib.rs"),
        "pub type ChangedRanges = HashMap<String, Vec<(usize, usize)>>;\n\
         \n\
         pub trait Renderer: Send {\n\
         \x20   fn draw(&self);\n\
         \x20   fn clear(&self) {}\n\
         }\n\
         \n\
         type Internal = u64;\n\
         \n\
         pub struct Painter;\n\
         impl Renderer for Painter {\n\
         \x20   fn draw(&self) {}\n\
         }\n",
    );
}

#[test]
fn find_type_alias_by_exact_name() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "ChangedRanges"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("pub type ChangedRanges = HashMap<String, Vec<(usize, usize)>>"),
        "alias must be findable with its RHS in the signature: {stdout}"
    );
    assert!(
        stdout.contains("[name]"),
        "exact hit gets the bare tag: {stdout}"
    );
}

#[test]
fn find_trait_by_exact_name_spans_body() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "Renderer"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("pub trait Renderer: Send"),
        "trait must render with supertraits: {stdout}"
    );
    // Trait body spans lines 3-6 so Read covers the method set.
    assert!(
        stdout.contains(":3-6"),
        "trait range must span its body: {stdout}"
    );
}

#[test]
fn find_kind_filters_narrow_to_trait_and_alias() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "find",
        "Renderer | ChangedRanges",
        "--trait",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("trait Renderer"), "{stdout}");
    assert!(
        !stdout.contains("ChangedRanges"),
        "--trait must exclude aliases: {stdout}"
    );

    let out = run_rustgraph(&[
        "--path",
        &base,
        "find",
        "Renderer | ChangedRanges",
        "--alias",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("ChangedRanges"), "{stdout}");
    assert!(
        !stdout.contains("trait Renderer"),
        "--alias must exclude traits: {stdout}"
    );
}

#[test]
fn find_json_carries_type_decl_partitions_and_ids() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "-j", "find", "ChangedRanges"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).expect("valid JSON");
    let exact = v["type_decls_exact"].as_array().unwrap();
    assert_eq!(exact.len(), 1, "{v}");
    assert_eq!(exact[0]["name"].as_str(), Some("ChangedRanges"));
    assert_eq!(exact[0]["kind"].as_str(), Some("alias"));
    assert!(
        exact[0]["symbol_id"]
            .as_str()
            .unwrap()
            .starts_with("alias:"),
        "{v}"
    );
}

#[test]
fn callers_on_trait_redirects_to_impls() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "Renderer"]);
    assert!(!out.status.success(), "traits have no callers; must steer");
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("is a trait") && stderr.contains("impls Renderer"),
        "callers on a trait must steer to impls: {stderr}"
    );
}

#[test]
fn callers_on_alias_redirects_to_refs() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "ChangedRanges"]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("is a type alias") && stderr.contains("refs ChangedRanges"),
        "callers on an alias must steer to refs: {stderr}"
    );
}

#[test]
fn inventory_counts_traits_and_aliases() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "inventory", "--trait", "--alias"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("Traits: 1"), "{stdout}");
    assert!(stdout.contains("Aliases: 2"), "{stdout}");
    assert!(
        !stdout.contains("fn draw"),
        "kind filter must exclude fns: {stdout}"
    );
}

#[test]
fn tree_annotates_trait_and_alias_counts() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "tree"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("1 trait"), "{stdout}");
    assert!(stdout.contains("2 alias"), "{stdout}");
}

#[test]
fn private_alias_excluded_by_public_only() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--public-only", "find", "Internal"]);
    assert!(
        !out.status.success(),
        "private alias must not match under --public-only; stdout: {}",
        stdout_of(&out)
    );
}
