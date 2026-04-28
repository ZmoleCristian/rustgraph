

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
fn regression_find_no_match_exits_nonzero() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() {}\npub fn beta() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "totally_made_up_xyz"]);
    assert!(
        !out.status.success(),
        "find with no match should exit non-zero; status={:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("no match"),
        "expected 'no match' in stderr; got: {stderr}"
    );
}


fn write_widget_fixture(fixture: &tempfile::TempDir) -> String {
    let mut src = String::new();
    for i in 0..60 {
        src.push_str(&format!("pub fn widget_{i:02}() {{}}\n"));
    }
    write_file(&fixture.path().join("src/lib.rs"), &src);
    fixture.path().to_string_lossy().to_string()
}

#[test]
fn regression_find_max_results_default_caps_output() {
    let fixture = tempdir().expect("tempdir");
    let base = write_widget_fixture(&fixture);
    let out = run_rustgraph(&["--path", &base, "find", "widget", "--search-threshold", "0.6"]);
    assert!(out.status.success(), "find failed: stderr={}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let body_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains(":") && l.contains(" - "))
        .collect();
    assert!(
        body_lines.len() <= 50,
        "default --max-results should cap to 50; got {} lines",
        body_lines.len()
    );
    assert!(
        stdout.contains("showing") && stdout.contains("of"),
        "expected truncation footer (showing N of M) in stdout; got:\n{stdout}"
    );
}

#[test]
fn regression_find_max_results_unlimited_with_zero() {
    let fixture = tempdir().expect("tempdir");
    let base = write_widget_fixture(&fixture);
    let out = run_rustgraph(&[
        "--path", &base, "find", "widget",
        "--search-threshold", "0.6", "--max-results", "0",
    ]);
    assert!(out.status.success(), "find failed: stderr={}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let body_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains(":") && l.contains(" - "))
        .collect();
    assert_eq!(
        body_lines.len(),
        60,
        "--max-results 0 should emit ALL matches; got {} lines, stdout=\n{}",
        body_lines.len(),
        stdout
    );
    assert!(
        !stdout.contains("showing") || !stdout.contains("of "),
        "no truncation footer expected; got:\n{stdout}"
    );
}


#[test]
fn regression_find_drops_id_suffix_by_default() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn unique_target() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "unique_target"]);
    assert!(out.status.success(), "find failed: stderr={}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        !stdout.contains("[id:"),
        "default find text should NOT contain `[id:`; got:\n{stdout}"
    );
    assert!(
        stdout.contains("unique_target"),
        "expected the function name to render; got:\n{stdout}"
    );
}

#[test]
fn regression_find_show_ids_flag_restores_id_suffix() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn unique_target() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "unique_target", "--show-ids"]);
    assert!(out.status.success(), "find failed: stderr={}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        stdout.contains("[id:"),
        "find --show-ids should restore `[id:`; got:\n{stdout}"
    );
}


#[test]
fn regression_find_exact_returns_all_exact_matches() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/a.rs"), "pub fn build() {}\n");
    write_file(&fixture.path().join("src/b.rs"), "pub fn build() {}\n");
    write_file(&fixture.path().join("src/c.rs"), "pub fn build() {}\n");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "mod a; mod b; mod c;\npub fn builder() {}\npub fn build_thing() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "build", "--exact", "-j"]);
    assert!(out.status.success(), "find --exact failed: stderr={}", String::from_utf8_lossy(&out.stderr));
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let names: Vec<String> = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names.len(), 3, "expected exactly 3 exact-name matches; got: {names:?}");
    assert!(
        names.iter().all(|n| n == "build"),
        "every result should be exactly `build`; got: {names:?}"
    );
}

#[test]
fn regression_find_exact_zero_match_still_exits_nonzero() {

    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn alpha() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "xyz_no_such_thing", "--exact"]);
    assert!(
        !out.status.success(),
        "find --exact with no match should exit non-zero; status={:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("no match"),
        "expected 'no match' in stderr; got: {stderr}"
    );
}


#[test]
fn regression_did_you_mean_uses_higher_floor() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn run_remove_context() {}
pub fn xyz_helper() {}
pub fn unrelated_thing() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "foo_bar_does_not_exist"]);
    assert!(!out.status.success(), "expected non-zero exit on no match");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();


    assert!(
        !stderr.contains("run_remove_context"),
        "low-similarity suggestion `run_remove_context` should be filtered out; stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("xyz_helper"),
        "low-similarity suggestion `xyz_helper` should be filtered out; stderr:\n{stderr}"
    );
}


#[test]
fn regression_callers_default_ambiguity_cap_is_seven() {
    let fixture = tempdir().expect("tempdir");

    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn handle_a() {}
pub fn handle_b() {}
pub fn handle_c() {}
pub fn handle_d() {}
pub fn handle_e() {}
pub fn caller() { handle_a(); handle_b(); handle_c(); handle_d(); handle_e(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "handle"]);
    assert!(
        out.status.success(),
        "5 fuzzy matches should be within new cap=7 default; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );


    let fixture2 = tempdir().expect("tempdir");
    write_file(
        &fixture2.path().join("src/lib.rs"),
        r#"
pub fn handle_a() {}
pub fn handle_b() {}
pub fn handle_c() {}
pub fn handle_d() {}
pub fn handle_e() {}
pub fn handle_f() {}
pub fn handle_g() {}
pub fn handle_h() {}
pub fn caller() { handle_a(); handle_b(); handle_c(); handle_d(); handle_e(); handle_f(); handle_g(); handle_h(); }
"#,
    );
    let base2 = fixture2.path().to_string_lossy().to_string();
    let out2 = run_rustgraph(&["--path", &base2, "callers", "handle"]);
    assert!(
        !out2.status.success(),
        "8 fuzzy matches should exceed cap=7 and bail; stderr={}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let stderr2 = String::from_utf8_lossy(&out2.stderr).to_string();
    assert!(
        stderr2.contains("ambiguous"),
        "expected 'ambiguous' message; got: {stderr2}"
    );
}


#[test]
fn regression_find_or_with_short_alts_returns_results() {
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
        "find OR query failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
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
        vec!["cut".to_string(), "inject".to_string(), "pick".to_string()],
        "expected all 3 alts to match; got: {names:?}"
    );
}


#[test]
fn regression_find_dynamic_threshold_hint_on_many_matches() {
    let fixture = tempdir().expect("tempdir");
    let mut src = String::new();
    for i in 0..15 {
        src.push_str(&format!("pub fn doohickey_{i}() {{}}\n"));
    }
    write_file(&fixture.path().join("src/lib.rs"), &src);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "doohickey"]);
    assert!(out.status.success(), "find failed: stderr={}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("threshold"),
        "expected dynamic threshold hint in stderr; got: {stderr}"
    );

    let mentions_higher = stderr.contains("0.95") || stderr.contains("0.99");
    assert!(
        mentions_higher,
        "expected hint to propose a stricter threshold (0.95 or 0.99); got: {stderr}"
    );
}
