

use serde_json::Value;
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
    Command::new(bin).args(full).output().expect("run rustgraph")
}


fn write_mixed_visibility_fixture() -> tempfile::TempDir {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn pub_one() {}\n\
         fn priv_one() {}\n\
         pub struct PubStruct;\n\
         struct PrivStruct;\n",
    );
    fixture
}


#[test]
fn regression_public_only_filters_inventory_to_pub_items_only() {
    let fixture = write_mixed_visibility_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "--analyze",
        "functions",
        "--public-only",
        "-j",
    ]);
    assert!(
        out.status.success(),
        "inventory failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid JSON");

    let names: Vec<String> = json["functions"]
        .as_array()
        .expect("functions array")
        .iter()
        .filter_map(|f| f["name"].as_str().map(String::from))
        .collect();

    assert!(
        names.contains(&"pub_one".to_string()),
        "expected `pub_one` in --public-only inventory; got: {names:?}"
    );
    assert!(
        !names.contains(&"priv_one".to_string()),
        "private fn `priv_one` should be filtered out by --public-only; got: {names:?}"
    );
}


#[test]
fn regression_public_only_propagates_to_find() {
    let fixture = write_mixed_visibility_fixture();
    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&[
        "--path",
        &base,
        "find",
        "pub_one|priv_one",
        "--public-only",
        "-j",
    ]);
    assert!(
        out.status.success(),
        "find failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid JSON");

    let names: Vec<String> = json["functions"]
        .as_array()
        .expect("functions array")
        .iter()
        .filter_map(|f| f["name"].as_str().map(String::from))
        .collect();

    assert!(
        names.contains(&"pub_one".to_string()),
        "expected `pub_one` in find --public-only output; got: {names:?}"
    );
    assert!(
        !names.contains(&"priv_one".to_string()),
        "private fn `priv_one` should be filtered out by --public-only; got: {names:?}"
    );
}
