

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


#[test]
fn regression_callers_in_filter_strip_all_message() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\n\
         fn caller_keep() { target(); }\n\
         fn caller_drop() { target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path", &base, "callers", "target", "--callers-in", "nonexistent_substr",
    ]);
    assert!(
        out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        stderr
    );


    assert!(
        stderr.contains("filtered out all") || stderr.contains("dropped all"),
        "expected --callers-in filter-strip-all hint in stderr; got combined output:\n{}",
        combined
    );


    assert!(
        stderr.contains("nonexistent_substr"),
        "filter hint should echo the substring `nonexistent_substr`; got stderr:\n{}",
        stderr
    );
}


#[test]
fn regression_callers_in_filters_transitive_callers() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod keep;\npub mod other;\n",
    );
    write_file(
        &fixture.path().join("src/keep.rs"),
        "pub mod inner;\n",
    );
    write_file(
        &fixture.path().join("src/keep/inner.rs"),
        "pub fn target() {}\n\
         pub fn lvl1_fn() { target(); }\n",
    );
    write_file(
        &fixture.path().join("src/other.rs"),
        "pub mod outer;\n",
    );
    write_file(
        &fixture.path().join("src/other/outer.rs"),
        "pub fn lvl2_fn() { crate::keep::inner::lvl1_fn(); }\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path", &base, "callers", "target",
        "--depth", "2",
        "--callers-in", "keep",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: Value = serde_json::from_str(&stdout).expect("valid json");
    let matches = json["matches"].as_array().expect("matches array");


    fn collect_names(node: &Value, out: &mut Vec<String>) {
        if let Some(name) = node["info"]["name"].as_str() {
            out.push(name.to_string());
        }
        if let Some(callers) = node["callers"].as_array() {
            for c in callers {
                collect_names(c, out);
            }
        }
    }

    let mut all_names: Vec<String> = Vec::new();
    for m in matches {
        if let Some(callers) = m["callers"].as_array() {
            for c in callers {
                collect_names(c, &mut all_names);
            }
        }
    }

    assert!(
        all_names.iter().any(|n| n == "lvl1_fn"),
        "expected lvl1_fn (inside `keep`) to appear in transitive callers; got names: {:?}\nfull json:\n{}",
        all_names, stdout
    );
    assert!(
        !all_names.iter().any(|n| n == "lvl2_fn"),
        "expected lvl2_fn (inside `other/`, OUTSIDE --callers-in keep) to be filtered \
         at every depth; got names: {:?}\nfull json:\n{}",
        all_names, stdout
    );
}


#[test]
fn regression_paths_between_reverse_suggestion_in_json() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn a() { b(); }\npub fn b() {}\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path", &base, "paths-between", "b", "a", "-j",
    ]);
    assert!(
        out.status.success(),
        "paths-between exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: Value = serde_json::from_str(&stdout).expect("valid paths-between json");


    let forward_paths = json["paths"].as_array().expect("paths array");
    assert!(
        forward_paths.is_empty(),
        "expected zero forward paths from b to a; got: {:?}",
        forward_paths
    );


    let suggestion = json
        .get("reverse_suggestion")
        .and_then(|v| v.as_array())
        .expect(&format!(
            "expected `reverse_suggestion` array on JSON when forward is empty + reverse \
             has paths; full json:\n{}",
            stdout
        ));
    assert!(
        !suggestion.is_empty(),
        "reverse_suggestion present but empty; should contain at least the a → b path. \
         full json:\n{}",
        stdout
    );
    let first_path = suggestion[0].as_array().expect("path is an array of nodes");
    assert!(
        first_path.len() >= 2,
        "reverse path should have ≥ 2 nodes (a → b); got {} nodes. full json:\n{}",
        first_path.len(),
        stdout
    );
    let names: Vec<&str> = first_path
        .iter()
        .map(|n| n["name"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        names.first().copied(),
        Some("a"),
        "reverse path should start at `a` (the original TO); got names: {:?}",
        names
    );
    assert_eq!(
        names.last().copied(),
        Some("b"),
        "reverse path should end at `b` (the original FROM); got names: {:?}",
        names
    );
}


#[test]
fn regression_paths_between_to_module_filter() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod mod1;\npub mod mod2;\n\
         pub fn entry() { mod1::target1(); mod2::target2(); }\n",
    );
    write_file(
        &fixture.path().join("src/mod1.rs"),
        "pub fn target1() {}\n",
    );
    write_file(
        &fixture.path().join("src/mod2.rs"),
        "pub fn target2() {}\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path", &base, "paths-between",
        "entry", "XYZ_no_match",
        "--to-module", "mod1",
        "-j",
    ]);
    assert!(
        out.status.success(),
        "paths-between exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: Value = serde_json::from_str(&stdout).expect("valid paths-between json");
    let paths = json["paths"].as_array().expect("paths array");
    assert!(
        !paths.is_empty(),
        "expected at least one path entry → mod1::target1; got 0 paths. full json:\n{}",
        stdout
    );

    let mut reaches_target1 = false;
    let mut reaches_target2 = false;
    for path in paths {
        if let Some(nodes) = path.as_array() {
            if let Some(last) = nodes.last() {
                let name = last["name"].as_str().unwrap_or("");
                if name == "target1" {
                    reaches_target1 = true;
                }
                if name == "target2" {
                    reaches_target2 = true;
                }
            }
        }
    }
    assert!(
        reaches_target1,
        "expected at least one path to reach target1 (in mod1.rs); paths:\n{}",
        stdout
    );
    assert!(
        !reaches_target2,
        "expected NO path to reach target2 (in mod2.rs, outside --to-module mod1); \
         paths:\n{}",
        stdout
    );
}


#[test]
fn regression_paths_between_to_module_without_to_arg() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod mod1;\npub mod mod2;\n\
         pub fn entry() { mod1::target1(); mod2::target2(); }\n",
    );
    write_file(
        &fixture.path().join("src/mod1.rs"),
        "pub fn target1() {}\n",
    );
    write_file(
        &fixture.path().join("src/mod2.rs"),
        "pub fn target2() {}\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path", &base, "paths-between", "entry",
        "--to-module", "mod1",
        "-j",
    ]);
    assert!(
        out.status.success(),
        "paths-between with omitted TO + --to-module exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: Value = serde_json::from_str(&stdout).expect("valid paths-between json");
    let paths = json["paths"].as_array().expect("paths array");
    assert!(
        !paths.is_empty(),
        "expected at least one path entry → mod1::target1 (no TO positional); paths:\n{}",
        stdout
    );


    let reaches_target1 = paths.iter().any(|p| {
        p.as_array()
            .and_then(|nodes| nodes.last())
            .and_then(|n| n["name"].as_str())
            .map(|n| n == "target1")
            .unwrap_or(false)
    });
    assert!(
        reaches_target1,
        "expected at least one path to reach target1; paths:\n{}",
        stdout
    );
}
