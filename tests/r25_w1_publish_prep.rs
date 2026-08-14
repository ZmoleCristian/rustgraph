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
fn help_output_does_not_contain_internal_referee_codes() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustgraph"))
        .arg("--help")
        .output()
        .expect("run rustgraph --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !text.contains("R22-W3"),
        "R22-W3 referee code leaked into --help. text:\n{text}"
    );
    assert!(
        !text.contains("R23-W1"),
        "R23-W1 referee code leaked into --help. text:\n{text}"
    );
    assert!(
        !text.contains("R23-W4"),
        "R23-W4 referee code leaked into --help. text:\n{text}"
    );
    assert!(
        !text.contains("GOTCHA:"),
        "GOTCHA: leaked into --help. text:\n{text}"
    );
}

#[test]
fn refs_long_help_does_not_contain_internal_codes() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustgraph"))
        .args(["refs", "--help"])
        .output()
        .expect("run rustgraph refs --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !text.contains("R22-W3"),
        "R22-W3 referee code leaked into refs --help. text:\n{text}"
    );
    assert!(
        !text.contains("R23-W4 GOTCHA:"),
        "R23-W4 GOTCHA: leaked into refs --help. text:\n{text}"
    );
    assert!(
        !text.contains("GOTCHA:"),
        "GOTCHA: prefix leaked into refs --help. text:\n{text}"
    );

    assert!(
        text.contains("assoc_call"),
        "assoc_call explanation should still be in refs --help. text:\n{text}"
    );
}

#[test]
fn call_graph_help_does_not_contain_r23_w4_reference() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustgraph"))
        .args(["call-graph", "--help"])
        .output()
        .expect("run rustgraph call-graph --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !text.contains("R23-W4"),
        "R23-W4 referee code leaked into call-graph --help. text:\n{text}"
    );

    assert!(
        text.contains("--all"),
        "expected --all flag in call-graph --help. text:\n{text}"
    );
}

fn write_callers_fixture(dir: &tempfile::TempDir) -> String {
    let mut src = String::from("pub fn target() {}\n");
    for i in 0..10 {
        src.push_str(&format!("fn caller_{i}() {{ target(); }}\n"));
    }
    write_file(&dir.path().join("lib.rs"), &src);
    dir.path().to_string_lossy().to_string()
}

#[test]
fn callers_max_results_short_flag_n_truncates_output() {
    let fixture = tempdir().expect("tempdir");
    let base = write_callers_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "callers", "target", "-n", "3"]);
    assert!(
        out.status.success(),
        "callers -n 3 should succeed. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("showing 3 of"),
        "expected truncation footer '(showing 3 of ...)'. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("use --max-results to expand"),
        "expected '--max-results' hint in footer. stdout:\n{stdout}"
    );

    let caller_count = (0..10)
        .filter(|i| stdout.contains(&format!("caller_{i}")))
        .count();
    assert!(
        caller_count <= 3,
        "expected at most 3 callers with -n 3, found {caller_count}. stdout:\n{stdout}"
    );
}

#[test]
fn callers_max_results_long_flag_truncates_output() {
    let fixture = tempdir().expect("tempdir");
    let base = write_callers_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "callers", "target", "--max-results", "3"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("showing 3 of"),
        "expected truncation footer. stdout:\n{stdout}"
    );
}

#[test]
fn callers_max_results_zero_shows_all() {
    let fixture = tempdir().expect("tempdir");
    let base = write_callers_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "callers", "target", "--max-results", "0"]);
    assert!(
        out.status.success(),
        "callers --max-results 0 should succeed. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        !stdout.contains("use --max-results to expand"),
        "expected NO truncation footer with --max-results 0. stdout:\n{stdout}"
    );

    let caller_count = (0..10)
        .filter(|i| stdout.contains(&format!("caller_{i}")))
        .count();
    assert_eq!(
        caller_count, 10,
        "expected all 10 callers with --max-results 0. stdout:\n{stdout}"
    );
}

fn write_find_in_fixture(dir: &tempfile::TempDir) -> String {
    fs::create_dir_all(dir.path().join("src/daemon")).expect("mkdir daemon");
    fs::create_dir_all(dir.path().join("src/session")).expect("mkdir session");
    write_file(
        &dir.path().join("src/daemon/core.rs"),
        "pub fn helper() {}\npub fn daemon_only() {}\n",
    );
    write_file(
        &dir.path().join("src/session/state.rs"),
        "pub fn helper() {}\npub fn session_only() {}\n",
    );
    dir.path().to_string_lossy().to_string()
}

#[test]
fn find_in_filter_restricts_to_matching_paths() {
    let fixture = tempdir().expect("tempdir");
    let base = write_find_in_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "find", "helper", "--in", "src/daemon"]);
    assert!(
        out.status.success(),
        "find --in should succeed. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("daemon"),
        "expected daemon path in results. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("session"),
        "session result should be filtered out. stdout:\n{stdout}"
    );
}

#[test]
fn find_without_in_returns_global_hits() {
    let fixture = tempdir().expect("tempdir");
    let base = write_find_in_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "find", "helper"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("daemon"),
        "expected daemon hit without --in. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("session"),
        "expected session hit without --in. stdout:\n{stdout}"
    );
}

fn write_paths_fixture(dir: &tempfile::TempDir) -> String {
    write_file(
        &dir.path().join("lib.rs"),
        "fn a() { b(); c(); }\nfn b() { z(); }\nfn c() { z(); }\nfn z() {}\n",
    );
    dir.path().to_string_lossy().to_string()
}

#[test]
fn paths_between_max_results_long_flag_works() {
    let fixture = tempdir().expect("tempdir");
    let base = write_paths_fixture(&fixture);

    let out = run_rustgraph(&[
        "--path",
        &base,
        "paths-between",
        "a",
        "z",
        "--max-results",
        "1",
    ]);
    assert!(
        out.status.success(),
        "paths-between --max-results 1 should succeed. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("search clamped") || stdout.contains("[1 path(s)]"),
        "expected exactly 1 path shown with --max-results 1. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("Path #2"),
        "expected only 1 path rendered, not 2. stdout:\n{stdout}"
    );
}

#[test]
fn paths_between_max_results_short_flag_n_works() {
    let fixture = tempdir().expect("tempdir");
    let base = write_paths_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "paths-between", "a", "z", "-n", "1"]);
    assert!(
        out.status.success(),
        "paths-between -n 1 should succeed. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("search clamped") || stdout.contains("[1 path(s)]"),
        "expected exactly 1 path shown with -n 1. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("Path #2"),
        "expected only 1 path rendered, not 2. stdout:\n{stdout}"
    );
}

#[test]
fn paths_between_max_paths_old_flag_errors() {
    let fixture = tempdir().expect("tempdir");
    let base = write_paths_fixture(&fixture);

    let out = run_rustgraph(&[
        "--path",
        &base,
        "paths-between",
        "a",
        "z",
        "--max-paths",
        "5",
    ]);
    assert!(
        !out.status.success(),
        "paths-between --max-paths should fail (hard rename). stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("max-paths")
            || stderr.contains("unexpected")
            || stderr.contains("unrecognized"),
        "expected error mentioning unknown flag. stderr:\n{stderr}"
    );
}

fn write_loose_resolution_fixture(dir: &tempfile::TempDir) -> String {
    // A bare-name call site plus a Type-qualified query: strict matching
    // zeroes, loose fallback rescues, and the stderr note is emitted.
    write_file(
        &dir.path().join("lib.rs"),
        "pub fn foo() -> u32 { 1 }\nfn caller_a() { let _ = foo(); }\n",
    );
    dir.path().to_string_lossy().to_string()
}

#[test]
fn loose_resolution_note_includes_ratio_format() {
    let fixture = tempdir().expect("tempdir");
    let base = write_loose_resolution_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "callers", "Bar::foo"]);
    assert!(
        out.status.success(),
        "callers should succeed via loose fallback. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        stderr.contains("note:") && stderr.contains("call sites"),
        "expected loose-fallback note on stderr. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains(" / ") && stderr.contains("%"),
        "expected ratio format 'X / Y call sites (Z%)' in stderr note. stderr:\n{stderr}"
    );

    assert!(
        !stderr.contains("R23-W1 strict filter zeroed"),
        "old alarming wording should be gone. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("type-strict matching returned 0") || stderr.contains("resolved by name"),
        "expected neutral explanation in stderr note. stderr:\n{stderr}"
    );
}

#[test]
fn paths_between_long_help_includes_strict_resolution() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustgraph"))
        .args(["paths-between", "--help"])
        .output()
        .expect("run paths-between --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("--strict-resolution") || text.contains("strict-resolution"),
        "expected --strict-resolution to appear in paths-between --help. text:\n{text}"
    );
}

#[test]
fn callers_long_help_includes_strict_resolution() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustgraph"))
        .args(["callers", "--help"])
        .output()
        .expect("run callers --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("strict-resolution"),
        "expected strict-resolution to appear in callers --help. text:\n{text}"
    );
}
