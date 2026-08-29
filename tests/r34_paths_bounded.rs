use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directories");
    }
    fs::write(path, content).expect("write fixture");
}

fn run_rustgraph(args: &[&str]) -> Output {
    let binary = env!("CARGO_BIN_EXE_rustgraph");
    Command::new(binary)
        .args(["--absolute-paths", "--no-auto-path"])
        .args(args)
        .output()
        .expect("run rustgraph")
}

fn json_output(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "rustgraph failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON output")
}

#[test]
fn qualified_same_name_call_does_not_create_phantom_path() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod status;\npub mod edge;\npub fn splice() {}\npub fn run_status() { status::serve(); }\npub fn run_edge() { edge::serve(); }\n",
    );
    write_file(
        &fixture.path().join("src/status.rs"),
        "pub fn serve() {}\n",
    );
    write_file(
        &fixture.path().join("src/edge.rs"),
        "pub fn serve() { crate::splice(); }\n",
    );
    let root = fixture.path().to_string_lossy().to_string();

    let phantom = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "run_status",
        "splice",
        "--json",
    ]));
    assert_eq!(phantom["paths"].as_array().unwrap().len(), 0, "{phantom}");

    let real = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "run_edge",
        "splice",
        "--show-call-sites",
        "--json",
    ]));
    let path = real["paths"][0].as_array().expect("real path");
    let names: Vec<&str> = path
        .iter()
        .map(|node| node["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["run_edge", "serve", "splice"]);
    assert!(path[0]["call_site_line"].as_u64().is_some());
    assert!(path[1]["call_site_line"].as_u64().is_some());
}

#[test]
fn external_qualified_call_does_not_bind_to_local_homonym() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\npub fn spawn() { target(); }\npub fn worker() {}\npub fn entry() { tokio::spawn(worker()); }\n",
    );
    let root = fixture.path().to_string_lossy().to_string();
    let value = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "entry",
        "target",
        "--json",
    ]));
    assert!(value["paths"].as_array().unwrap().is_empty(), "{value}");
}

#[test]
fn method_call_does_not_bind_to_same_name_free_function() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct Listener;\nimpl Listener { pub fn accept(&self) {} }\npub fn target() {}\npub fn accept() { target(); }\npub fn entry(listener: &Listener) { listener.accept(); }\n",
    );
    let root = fixture.path().to_string_lossy().to_string();
    let value = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "entry",
        "target",
        "--json",
    ]));
    assert!(value["paths"].as_array().unwrap().is_empty(), "{value}");
}

#[test]
fn renamed_import_remains_a_real_path_edge() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/inner.rs"),
        "pub fn target() {}\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod inner;\npub use inner::target as renamed_target;\npub fn entry() { renamed_target(); }\n",
    );
    let root = fixture.path().to_string_lossy().to_string();
    let value = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "entry",
        "target",
        "--json",
    ]));
    let names: Vec<&str> = value["paths"][0]
        .as_array()
        .expect("path through renamed import")
        .iter()
        .map(|node| node["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["entry", "target"]);
}

#[test]
fn depth_is_enforced_as_an_exact_node_limit() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() { gamma(); }\npub fn gamma() {}\n",
    );
    let root = fixture.path().to_string_lossy().to_string();
    let shallow = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "alpha",
        "gamma",
        "--depth",
        "2",
        "--json",
    ]));
    assert!(shallow["paths"].as_array().unwrap().is_empty());

    let exact = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "alpha",
        "gamma",
        "--depth",
        "3",
        "--json",
    ]));
    assert_eq!(exact["paths"][0].as_array().unwrap().len(), 3);
}

#[test]
fn expansion_budget_is_reported_in_json() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() { gamma(); }\npub fn gamma() {}\n",
    );
    let root = fixture.path().to_string_lossy().to_string();
    let value = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "alpha",
        "gamma",
        "--max-expansions",
        "1",
        "--json",
    ]));
    assert_eq!(value["search_truncated"].as_bool(), Some(true));
    assert_eq!(value["expansions"].as_u64(), Some(1));
    assert!(value["paths"].as_array().unwrap().is_empty());
}

#[test]
fn max_results_returns_the_shortest_path_first() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn entry() { middle(); target(); }\npub fn middle() { target(); }\npub fn target() {}\n",
    );
    let root = fixture.path().to_string_lossy().to_string();
    let value = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "entry",
        "target",
        "--max-results",
        "1",
        "--json",
    ]));
    let paths = value["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].as_array().unwrap().len(), 2);
}

#[test]
fn exclude_tests_removes_test_nodes_from_traversal() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn entry() { bridge(); }\npub fn target() {}\n#[cfg(test)]\nfn bridge() { target(); }\n",
    );
    let root = fixture.path().to_string_lossy().to_string();
    let default = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "paths-between",
        "entry",
        "target",
        "--json",
    ]));
    assert_eq!(default["paths"].as_array().unwrap().len(), 1);

    let filtered = json_output(&run_rustgraph(&[
        "--path",
        &root,
        "--exclude-tests",
        "paths-between",
        "entry",
        "target",
        "--json",
    ]));
    assert!(filtered["paths"].as_array().unwrap().is_empty());
}
