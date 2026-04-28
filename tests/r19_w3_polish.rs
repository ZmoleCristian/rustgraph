

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
fn regression_grep_max_results_caps() {


    let fixture = tempdir().expect("tempdir");
    let body: String = (0..5).map(|i| format!("// needle {}\n", i)).collect();
    write_file(&fixture.path().join("src/lib.rs"), &body);
    let base = fixture.path().to_string_lossy().to_string();

    let out =
        run_rustgraph(&["--path", &base, "grep", "needle", "--max-results", "2"]);
    assert!(
        out.status.success(),
        "grep --max-results failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();


    assert!(
        stdout.contains("5 match(es)"),
        "expected '5 match(es)' (true total) in header; got:\n{stdout}"
    );

    assert!(
        stdout.contains("showing 2 of 5") && stdout.contains("--max-results"),
        "expected '(showing 2 of 5; use --max-results to expand)' footer; got:\n{stdout}"
    );
}


fn write_hashmap_btreemap_fixture(fixture: &tempfile::TempDir) -> String {
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn HashMap_factory() {}\npub fn BTreeMap_factory() {}\n",
    );
    fixture.path().to_string_lossy().to_string()
}

#[test]
fn regression_grep_escaped_pipe_emits_hint() {
    let fixture = tempdir().expect("tempdir");
    let base = write_hashmap_btreemap_fixture(&fixture);


    let out = run_rustgraph(&["--path", &base, "grep", r"HashMap\|BTreeMap"]);


    assert!(
        out.status.success(),
        "grep with 0 matches should exit 0; status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        stdout.contains("0 match(es)"),
        "expected '0 match(es)' in output; got:\n{stdout}"
    );

    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        stderr.contains("\\|"),
        "expected hint mentioning `\\|` in stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("|") && (stderr.contains("OR") || stderr.contains("did you mean")),
        "expected hint about `|` (OR / did you mean) in stderr; got:\n{stderr}"
    );

    assert!(
        stderr.contains("HashMap|BTreeMap"),
        "expected the suggested unescaped pattern `HashMap|BTreeMap` in stderr; got:\n{stderr}"
    );
}


#[test]
fn regression_grep_unescaped_pipe_works_normally() {
    let fixture = tempdir().expect("tempdir");
    let base = write_hashmap_btreemap_fixture(&fixture);
    let out = run_rustgraph(&["--path", &base, "grep", "HashMap|BTreeMap"]);
    assert!(
        out.status.success(),
        "grep failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    assert!(
        stdout.contains("HashMap_factory"),
        "expected HashMap_factory in output; got:\n{stdout}"
    );
    assert!(
        stdout.contains("BTreeMap_factory"),
        "expected BTreeMap_factory in output; got:\n{stdout}"
    );
    assert!(
        stdout.contains("2 match(es)"),
        "expected '2 match(es)' in header; got:\n{stdout}"
    );


    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        !stderr.contains("did you mean"),
        "no `did you mean` hint should fire on unescaped pipe; stderr=\n{stderr}"
    );
}


fn write_session_fixture(fixture: &tempfile::TempDir) -> String {
    write_file(&fixture.path().join("src/session/a.rs"), "pub fn alpha() {}\n");
    write_file(&fixture.path().join("src/session/b.rs"), "pub fn beta() {}\n");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod session;\n",
    );
    write_file(
        &fixture.path().join("src/session/mod.rs"),
        "pub mod a;\npub mod b;\n",
    );
    fixture.path().to_string_lossy().to_string()
}

#[test]
fn regression_tree_prefix_no_match_suggests_alternative() {
    let fixture = tempdir().expect("tempdir");
    let base = write_session_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "tree", "session"]);
    assert!(
        out.status.success(),
        "tree should exit 0 even on 0-match prefix; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();


    assert!(
        stdout.contains("0 file(s)"),
        "expected '0 file(s)' in header; got:\n{stdout}"
    );

    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        stderr.contains("src/session"),
        "expected `src/session` suggestion in stderr; got:\n{stderr}"
    );


    assert!(
        stderr.contains("note:")
            && (stderr.contains("0 files match")
                || stderr.contains("matched 0 files")),
        "expected `note: ...` framing reporting 0 files; got:\n{stderr}"
    );
}

#[test]
fn regression_tree_prefix_no_match_does_not_auto_retry() {
    let fixture = tempdir().expect("tempdir");
    let base = write_session_fixture(&fixture);

    let out = run_rustgraph(&["--path", &base, "tree", "session"]);
    assert!(out.status.success(), "tree should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();


    assert!(
        !stdout.contains("a.rs"),
        "tree should NOT auto-retry with the suggested prefix; stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("b.rs"),
        "tree should NOT auto-retry with the suggested prefix; stdout:\n{stdout}"
    );
}
