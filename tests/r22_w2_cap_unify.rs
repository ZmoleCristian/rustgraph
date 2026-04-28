

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

fn write_needle_fixture(fixture: &tempfile::TempDir, count: usize) -> String {
    let body: String = (0..count)
        .map(|i| format!("// needle line {}\n", i))
        .collect();
    write_file(&fixture.path().join("src/lib.rs"), &body);
    fixture.path().to_string_lossy().to_string()
}


#[test]
fn r22_grep_max_results_caps_output() {
    let fixture = tempdir().expect("tempdir");
    let base = write_needle_fixture(&fixture, 10);

    let out = run_rustgraph(&["--path", &base, "grep", "needle", "--max-results", "5"]);
    assert!(
        out.status.success(),
        "grep --max-results 5 failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();


    assert!(
        stdout.contains("10 match(es)"),
        "expected '10 match(es)' (true total) in header; got:\n{stdout}"
    );

    assert!(
        stdout.contains("showing 5 of 10") && stdout.contains("--max-results"),
        "expected '(showing 5 of 10; use --max-results to expand)' footer; got:\n{stdout}"
    );
}


#[test]
fn r22_grep_max_matches_is_gone() {
    let fixture = tempdir().expect("tempdir");
    let base = write_needle_fixture(&fixture, 10);

    let out = run_rustgraph(&["--path", &base, "grep", "needle", "--max-matches", "5"]);
    assert!(
        !out.status.success(),
        "grep --max-matches must FAIL post-R22-W2 hard rename; status={:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("--max-matches") || stderr.contains("unexpected"),
        "expected clap unknown-flag error mentioning --max-matches; got:\n{stderr}"
    );
}


fn write_usages_firehose_fixture(fixture: &tempfile::TempDir) -> String {

    let mut src = String::from("pub fn target_fn() {}\n");
    src.push_str("pub fn caller() {\n");
    for _ in 0..130 {
        src.push_str("    let _ = target_fn();\n");
    }
    src.push_str("}\n");
    write_file(&fixture.path().join("src/lib.rs"), &src);
    fixture.path().to_string_lossy().to_string()
}

#[test]
fn r22_usages_accepts_max_results_flag() {
    let fixture = tempdir().expect("tempdir");
    let base = write_usages_firehose_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "usages", "target_fn", "--max-results", "10"]);
    assert!(
        out.status.success(),
        "usages --max-results 10 failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    assert!(
        stdout.contains("showing 10 of") && stdout.contains("--max-results"),
        "expected '(showing 10 of N; use --max-results to expand)' footer; got:\n{stdout}"
    );
}

#[test]
fn r22_usages_default_cap_doesnt_dump_unbounded() {
    let fixture = tempdir().expect("tempdir");
    let base = write_usages_firehose_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "usages", "target_fn"]);
    assert!(
        out.status.success(),
        "usages (no flag) failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    assert!(
        stdout.contains("showing") && stdout.contains("--max-results"),
        "default usages cap (100) should kick in on a 130-hit firehose; got:\n{stdout}"
    );


    let entry_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.trim_start().starts_with("call ") || l.trim_start().starts_with("method "))
        .collect();
    assert!(
        entry_lines.len() <= 100,
        "default usages cap should bound rendered entries to ≤ 100; got {} lines:\n{}",
        entry_lines.len(),
        stdout
    );
}


fn write_chain_fixture(fixture: &tempfile::TempDir) -> String {
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn alpha() { beta(); }
pub fn beta() { gamma(); }
pub fn gamma() { delta(); }
pub fn delta() { let _ = 1; }
"#,
    );
    fixture.path().to_string_lossy().to_string()
}

#[test]
fn r22_paths_between_depth_works() {
    let fixture = tempdir().expect("tempdir");
    let base = write_chain_fixture(&fixture);

    let out = run_rustgraph(&[
        "--path", &base, "paths-between", "alpha", "delta", "--depth", "5", "--json",
    ]);
    assert!(
        out.status.success(),
        "paths-between --depth 5 failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        stdout.contains("\"alpha\"") && stdout.contains("\"delta\""),
        "expected alpha→delta path in JSON; got:\n{stdout}"
    );
}


#[test]
fn r22_paths_between_max_depth_is_gone() {
    let fixture = tempdir().expect("tempdir");
    let base = write_chain_fixture(&fixture);

    let out = run_rustgraph(&[
        "--path", &base, "paths-between", "alpha", "delta", "--max-depth", "5",
    ]);
    assert!(
        !out.status.success(),
        "paths-between --max-depth must FAIL post-R22-W2 hard rename; status={:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("--max-depth") || stderr.contains("unexpected"),
        "expected clap unknown-flag error mentioning --max-depth; got:\n{stderr}"
    );
}


#[test]
fn r22_find_n_short_alias_works() {
    let fixture = tempdir().expect("tempdir");
    let mut src = String::new();
    for i in 0..15 {
        src.push_str(&format!("pub fn widget_{i:02}() {{}}\n"));
    }
    write_file(&fixture.path().join("src/lib.rs"), &src);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path", &base, "find", "widget", "--search-threshold", "0.6", "-n", "5",
    ]);
    assert!(
        out.status.success(),
        "find -n 5 (short alias) failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let body_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains(":") && l.contains(" - "))
        .collect();
    assert!(
        body_lines.len() <= 5,
        "find -n 5 should cap output at 5; got {} lines:\n{}",
        body_lines.len(),
        stdout
    );
    assert!(
        stdout.contains("showing 5 of") && stdout.contains("--max-results"),
        "expected '(showing 5 of N; use --max-results to expand)' footer; got:\n{stdout}"
    );
}


#[test]
fn r22_call_graph_depth_still_works() {
    let fixture = tempdir().expect("tempdir");
    let base = write_chain_fixture(&fixture);

    let out = run_rustgraph(&[
        "--path", &base, "call-graph", "--root", "alpha", "--depth", "2",
    ]);
    assert!(
        out.status.success(),
        "call-graph --depth 2 failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}


#[test]
fn r22_truncation_format_consistent_find_and_refs() {

    let fixture = tempdir().expect("tempdir");
    let mut src = String::new();
    for i in 0..60 {
        src.push_str(&format!("pub fn widget_{i:02}() {{}}\n"));
    }
    write_file(&fixture.path().join("src/lib.rs"), &src);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path", &base, "find", "widget", "--search-threshold", "0.6", "--max-results", "5",
    ]);
    assert!(out.status.success(), "find failed: stderr={}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    let footer_line = stdout
        .lines()
        .find(|l| l.contains("showing") && l.contains("--max-results"))
        .unwrap_or_else(|| panic!("find: no truncation footer found; stdout:\n{stdout}"));
    assert!(
        footer_line.starts_with("(showing ")
            && footer_line.contains(" of ")
            && footer_line.contains("use --max-results to expand")
            && footer_line.ends_with(')'),
        "find truncation footer must match `(showing N of M; use --max-results to expand)` shape; \
         got: `{footer_line}`"
    );


    let fixture2 = tempdir().expect("tempdir");
    let mut src2 = String::from("pub fn widget_target() {}\npub fn driver() {\n");
    for _ in 0..60 {
        src2.push_str("    let _ = widget_target();\n");
    }
    src2.push_str("}\n");
    write_file(&fixture2.path().join("src/lib.rs"), &src2);
    let base2 = fixture2.path().to_string_lossy().to_string();

    let out2 = run_rustgraph(&[
        "--path", &base2, "refs", "widget_target", "--max-results", "5",
    ]);
    assert!(out2.status.success(), "refs failed: stderr={}", String::from_utf8_lossy(&out2.stderr));
    let stdout2 = String::from_utf8_lossy(&out2.stdout).to_string();
    let footer_line2 = stdout2
        .lines()
        .find(|l| l.contains("showing") && l.contains("--max-results"))
        .unwrap_or_else(|| panic!("refs: no truncation footer found; stdout:\n{stdout2}"));
    assert!(
        footer_line2.starts_with("(showing ")
            && footer_line2.contains(" of ")
            && footer_line2.contains("use --max-results to expand")
            && footer_line2.ends_with(')'),
        "refs truncation footer must match `(showing N of M; use --max-results to expand)` shape; \
         got: `{footer_line2}`"
    );
}
