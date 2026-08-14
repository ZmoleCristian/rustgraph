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
fn regression_callers_finds_pub_use_reexport_call_sites() {
    let fixture = tempdir().expect("tempdir");

    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod shared;\npub mod alpha;\npub mod consumer;\n",
    );
    write_file(
        &fixture.path().join("src/shared.rs"),
        "pub fn target() {}\n",
    );
    write_file(
        &fixture.path().join("src/alpha.rs"),
        "pub mod session;\npub fn target() {} // homonym, forces name_collision\n",
    );
    write_file(
        &fixture.path().join("src/alpha/session.rs"),
        "pub use crate::shared::target;  // PLAIN pub use — UseTree::Path (R16-A4)\n",
    );
    write_file(
        &fixture.path().join("src/consumer.rs"),
        "pub fn caller() {\n    crate::alpha::session::target();\n}\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "target", "-j"]);
    assert!(
        out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let matches = json["matches"].as_array().expect("matches array");
    assert!(
        !matches.is_empty(),
        "no matches for `target`; json:\n{}",
        json
    );

    let mut real_target_callers: Vec<String> = Vec::new();
    let mut found_real_target_match = false;
    for m in matches {
        let file = m["info"]["file_path"].as_str().unwrap_or("");

        if file.ends_with("/shared.rs") {
            found_real_target_match = true;
            if let Some(callers) = m["callers"].as_array() {
                for c in callers {
                    if let Some(name) = c["info"]["name"].as_str() {
                        real_target_callers.push(name.to_string());
                    }
                }
            }
        }
    }

    assert!(
        found_real_target_match,
        "expected a match for the real `target` in src/shared.rs; full json:\n{}",
        json
    );
    assert!(
        real_target_callers.iter().any(|n| n == "caller"),
        "expected `caller` in callers list of the REAL `target` (src/shared.rs, \
         re-exported via `pub use crate::shared::target` from alpha::session); \
         got callers: {:?}\nfull json:\n{}",
        real_target_callers,
        json
    );
}

#[test]
fn regression_callers_and_usages_agree_on_caller_count() {
    let fixture = tempdir().expect("tempdir");

    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod shared;\npub mod alpha;\npub mod consumer;\n",
    );
    write_file(
        &fixture.path().join("src/shared.rs"),
        "pub fn target() {}\n",
    );
    write_file(
        &fixture.path().join("src/alpha.rs"),
        "pub mod session;\npub fn target() {}\n",
    );
    write_file(
        &fixture.path().join("src/alpha/session.rs"),
        "pub use crate::shared::target;\n",
    );
    write_file(
        &fixture.path().join("src/consumer.rs"),
        "pub fn caller() {\n    crate::alpha::session::target();\n}\n",
    );

    let base = fixture.path().to_string_lossy().to_string();

    let callers_out = run_rustgraph(&[
        "--path",
        &base,
        "callers",
        "target",
        "--target-in",
        "shared",
        "-j",
    ]);
    assert!(
        callers_out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&callers_out.stderr)
    );
    let callers_json: Value =
        serde_json::from_slice(&callers_out.stdout).expect("valid callers json");
    let callers_count: usize = callers_json["matches"]
        .as_array()
        .map(|matches| {
            matches
                .iter()
                .map(|m| m["callers"].as_array().map(|a| a.len()).unwrap_or(0))
                .sum()
        })
        .unwrap_or(0);

    let usages_out = run_rustgraph(&["--path", &base, "usages", "target", "--in", "shared", "-j"]);
    assert!(
        usages_out.status.success(),
        "usages exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&usages_out.stderr)
    );
    let usages_json: Value = serde_json::from_slice(&usages_out.stdout).expect("valid usages json");
    let usages_callers_section_count: usize = usages_json["sections"]
        .as_array()
        .map(|sections| {
            sections
                .iter()
                .filter(|s| s["label"].as_str().map(|l| l == "callers").unwrap_or(false))
                .map(|s| s["count"].as_u64().unwrap_or(0) as usize)
                .sum()
        })
        .unwrap_or(0);

    let expected = 1;
    assert_eq!(
        callers_count, expected,
        "expected {} caller(s) for shared::target (re-exported via alpha::session, \
         called from consumer.rs:2), got {}.\ncallers json:\n{}",
        expected, callers_count, callers_json
    );
    assert_eq!(
        callers_count, usages_callers_section_count,
        "callers count ({}) and usages 'callers' section count ({}) disagree on \
         the same target (shared::target) — silent trust-killer (sonnet_2 R15b \
         reported 0 vs 13 in real codebases).\ncallers json:\n{}\nusages json:\n{}",
        callers_count, usages_callers_section_count, callers_json, usages_json
    );
}

#[test]
fn regression_call_graph_dot_with_root_emits_subgraph_with_edges() {
    let fixture = tempdir().expect("tempdir");

    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn a() { b(); }\npub fn b() { c(); }\npub fn c() { d(); }\npub fn d() {}\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--search", "b", "call-graph", "--dot"]);
    assert!(
        out.status.success(),
        "call-graph exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("digraph call_graph"),
        "expected DOT output, got:\n{}",
        stdout
    );

    let edge_count = stdout.lines().filter(|l| l.contains(" -> ")).count();
    assert!(
        edge_count >= 2,
        "expected at least 2 edges in DOT subgraph (b -> c, c -> d), \
         got {} edges. Full DOT:\n{}",
        edge_count,
        stdout
    );
}

#[test]
fn regression_call_graph_dot_root_subgraph_includes_root_node() {
    let fixture = tempdir().expect("tempdir");

    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn a() { b(); }\npub fn b() { c(); }\npub fn c() { d(); }\npub fn d() {}\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--search", "b", "call-graph", "--dot"]);
    assert!(
        out.status.success(),
        "call-graph exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("digraph call_graph"),
        "expected DOT output, got:\n{}",
        stdout
    );

    let has_root_node = stdout.lines().any(|l| {
        let trimmed = l.trim_start();
        trimmed.starts_with('"')
            && (trimmed.contains(":b\"")
                || trimmed.contains("label=\"b\\n")
                || trimmed.contains("label=\"b\""))
    });
    assert!(
        has_root_node,
        "expected root node `b` in DOT subgraph, full DOT:\n{}",
        stdout
    );

    let edge_count = stdout.lines().filter(|l| l.contains(" -> ")).count();
    assert!(
        edge_count >= 1,
        "expected at least 1 edge from root `b` (b -> c), \
         got {} edges. Full DOT:\n{}",
        edge_count,
        stdout
    );
}
