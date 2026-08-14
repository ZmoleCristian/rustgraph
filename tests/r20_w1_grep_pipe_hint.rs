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

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn pipe_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn HashMap_factory() {}\npub fn BTreeMap_factory() {}\n",
    );
    dir
}

#[test]
fn regression_grep_escaped_pipe_zero_match_emits_hint() {
    let fixture = pipe_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"HashMap\|BTreeMap"]);

    assert!(
        out.status.success(),
        "grep with 0 matches should still exit 0; status={:?}, stderr={}",
        out.status,
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("0 match"),
        "expected '0 match(es)' header confirming zero hits; got stdout:\n{}",
        stdout
    );

    let stderr = stderr_of(&out);
    let lower_stderr = stderr.to_lowercase();

    assert!(
        lower_stderr.contains("tip") || lower_stderr.contains("did you mean"),
        "expected `\\|` hint on stderr (tip / did you mean); got stderr:\n{}\nstdout:\n{}",
        stderr,
        stdout
    );

    assert!(
        stderr.contains("HashMap|BTreeMap"),
        "expected hint to suggest `HashMap|BTreeMap` (no escape); got stderr:\n{}",
        stderr
    );
}

#[test]
fn regression_grep_unescaped_pipe_works_silently() {
    let fixture = pipe_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "HashMap|BTreeMap"]);
    assert!(
        out.status.success(),
        "grep with unescaped pipe failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("HashMap_factory"),
        "expected 'HashMap_factory' to be matched; got stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("BTreeMap_factory"),
        "expected 'BTreeMap_factory' to be matched; got stdout:\n{}",
        stdout
    );
    assert!(
        stdout.contains("2 match"),
        "expected 2 matches; got stdout:\n{}",
        stdout
    );

    let stderr = stderr_of(&out);
    let lower_stderr = stderr.to_lowercase();

    assert!(
        !lower_stderr.contains("did you mean") && !lower_stderr.contains("escape"),
        "expected NO `\\|` hint on stderr when pipe is unescaped; got stderr:\n{}",
        stderr
    );
}

#[test]
fn regression_grep_escaped_pipe_with_matches_still_emits_hint() {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "// hits Foo|Bar marker\npub fn ok() {}\n",
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"Foo\|Bar"]);
    assert!(
        out.status.success(),
        "grep failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("Foo|Bar"),
        "expected literal `Foo|Bar` match; got stdout:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("0 match"),
        "expected non-zero matches; got stdout:\n{}",
        stdout
    );

    let stderr = stderr_of(&out);
    let lower_stderr = stderr.to_lowercase();

    assert!(
        lower_stderr.contains("did you mean"),
        "expected `\\|` hint to fire even with matches > 0 (R21-W3); got stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Foo|Bar"),
        "expected hint to suggest `Foo|Bar` (no escape); got stderr:\n{}",
        stderr
    );
}
