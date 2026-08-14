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
    Command::new(bin)
        .args(full)
        .output()
        .expect("run rustgraph")
}

#[test]
fn regression_callers_sees_calls_inside_macro_bodies() {
    let fixture = tempdir().expect("tempdir");

    write_file(
        &fixture.path().join("src/lib.rs"),
        concat!(
            "pub fn helper() -> bool {\n",
            "    true\n",
            "}\n",
            "\n",
            "pub fn driver() {\n",
            "    assert!(helper()); // call exists ONLY inside macro tokens (R19-A)\n",
            "}\n",
        ),
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "helper", "-j"]);
    assert!(
        out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let matches = json["matches"].as_array().expect("matches array");
    assert!(
        !matches.is_empty(),
        "no matches for `helper`; json:\n{}",
        json
    );

    let caller_names: Vec<String> = matches
        .iter()
        .flat_map(|m| m["callers"].as_array().cloned().unwrap_or_default())
        .filter_map(|c| c["info"]["name"].as_str().map(String::from))
        .collect();
    assert!(
        caller_names.contains(&"driver".to_string()),
        "driver's macro-body call to helper() was not reported as a caller; got {:?}\njson:\n{}",
        caller_names,
        json
    );
}

#[test]
fn regression_callers_resolves_pub_use_rename_alias() {
    let fixture = tempdir().expect("tempdir");

    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod store;\npub mod consumer;\n",
    );
    write_file(
        &fixture.path().join("src/store/mod.rs"),
        "mod operator;\nmod org;\npub use operator::add as add_operator; // rename (R19-B)\n",
    );
    write_file(
        &fixture.path().join("src/store/operator.rs"),
        "pub fn add() {}\n",
    );
    write_file(
        &fixture.path().join("src/store/org.rs"),
        "pub fn add() {} // homonym in a sibling module, must NOT match\n",
    );
    write_file(
        &fixture.path().join("src/consumer.rs"),
        concat!(
            "pub fn operator_add() {\n",
            "    crate::store::add_operator();\n",
            "}\n",
        ),
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "add_operator", "-j"]);
    assert!(
        out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let matches = json["matches"].as_array().expect("matches array");
    assert!(
        !matches.is_empty(),
        "alias query `add_operator` resolved to nothing; json:\n{}",
        json
    );

    let target_files: Vec<String> = matches
        .iter()
        .filter_map(|m| m["info"]["file_path"].as_str().map(String::from))
        .collect();
    assert!(
        target_files.iter().all(|f| f.contains("operator")),
        "alias resolution leaked outside the re-exported module; files {:?}\njson:\n{}",
        target_files,
        json
    );
    assert!(
        target_files.iter().any(|f| f.ends_with("operator.rs")),
        "expected the operator::add target; files {:?}",
        target_files
    );
}
