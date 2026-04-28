

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

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}


#[test]
fn regression_short_query_suggestions_drop_similarity_one() {
    let fixture = tempdir().expect("tempdir");

    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn alpha_helper() {}
pub fn beta_loader() {}
pub fn gamma_runner() {}
pub fn delta_probe() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&["--path", &base, "find", "zyq"]);
    assert!(
        !out.status.success(),
        "find should exit non-zero on 0 matches (no substring rescue); status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("no match"),
        "expected 'no match' header; got: {stderr}"
    );

    assert!(
        !stderr.contains("falling back to substring"),
        "rescue note must NOT fire when fallback also returns 0 (genuine no-match); got:\n{stderr}"
    );


    assert!(
        !stderr.contains("similarity 1.00"),
        "short-query did-you-mean must drop similarity 1.00 entries; got:\n{stderr}"
    );
}


#[test]
fn regression_short_query_run_now_rescues_via_fallback() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn run_cli() {}
pub fn run_session() {}
pub fn run_daemon() {}
pub fn unrelated_helper() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "run"]);
    assert!(
        out.status.success(),
        "R26-W3 fallback should rescue `find run` against run_*; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    for name in ["run_cli", "run_session", "run_daemon"] {
        assert!(
            stdout.contains(name),
            "expected substring rescue to surface `{name}`; got stdout:\n{stdout}"
        );
    }

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("falling back to substring"),
        "expected R26-W3 rescue note on stderr; got:\n{stderr}"
    );
}


#[test]
fn regression_multi_term_or_query_unaffected_by_suggestion_filter() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn cut() {}
pub fn pick() {}
pub fn inject() {}
pub fn unrelated() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "cut|pick|inject", "-j"]);
    assert!(
        out.status.success(),
        "multi-term OR query failed: stderr={}",
        stderr_of(&out)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let names: Vec<String> = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names.len(),
        3,
        "multi-term OR should match all 3 alts (R18-W2 disables short-query strict); got: {names:?}"
    );
}


#[test]
fn regression_long_query_drops_suggestions_above_threshold() {
    let fixture = tempdir().expect("tempdir");


    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn aLongName_fooBarDoesNotExist_yes() {}
pub fn fooBarMostlyExistx_helper() {}
pub fn complete_unrelated() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&[
        "--path",
        &base,
        "find",
        "fooBarDoesNotExist",
        "--search-threshold",
        "0.99",
    ]);
    assert!(
        !out.status.success(),
        "expected non-zero exit at strict threshold; status={:?}, stderr={}",
        out.status,
        stderr_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(stderr.contains("no match"), "expected 'no match' header; got: {stderr}");


    assert!(
        !stderr.contains("similarity 1.00"),
        "long-query did-you-mean must drop entries ≥ effective threshold; got:\n{stderr}"
    );
}


#[test]
fn regression_suggestions_still_surface_genuine_fuzz_below_threshold() {
    let fixture = tempdir().expect("tempdir");


    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn widget_processing() {}\npub fn unrelated() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "find",
        "widget_proceessing",
        "--search-threshold",
        "0.99",
    ]);
    assert!(
        !out.status.success(),
        "expected non-zero exit at strict threshold; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("did you mean"),
        "expected 'did you mean' block to fire for genuine fuzz; got:\n{stderr}"
    );
    assert!(
        stderr.contains("widget_processing"),
        "expected genuine-fuzz suggestion `widget_processing` to surface; got:\n{stderr}"
    );
}


#[test]
fn regression_match_signature_matches_param_and_return_text() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct InventoryState;

pub fn handle_event(s: &InventoryState) {}
pub fn build_view() -> InventoryState { InventoryState }
pub fn unrelated_helper() -> u32 { 0 }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();


    let out_default =
        run_rustgraph(&["--path", &base, "find", "InventoryState", "-j", "--func"]);
    assert!(
        !out_default.status.success(),
        "expected non-zero exit when filtering to fns only with no fn name match; \
         status={:?}, stderr={}",
        out_default.status,
        stderr_of(&out_default)
    );
    let stderr_default = stderr_of(&out_default);
    assert!(
        stderr_default.contains("no match"),
        "expected 'no match' for fn-only without --match-signature; got: {stderr_default}"
    );


    let out_wide = run_rustgraph(&[
        "--path",
        &base,
        "--match-signature",
        "find",
        "InventoryState",
        "-j",
        "--func",
    ]);
    assert!(
        out_wide.status.success(),
        "--match-signature failed: stderr={}",
        stderr_of(&out_wide)
    );
    let json: Value = serde_json::from_slice(&out_wide.stdout).expect("valid json");
    let names: Vec<String> = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_string())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        vec!["build_view".to_string(), "handle_event".to_string()],
        "expected both signature-text matches; got: {names:?}"
    );
}

