

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

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn write_derive_fixture(dir: &tempfile::TempDir) -> String {
    write_file(
        &dir.path().join("src/lib.rs"),
        "use serde::Serialize;\n\
         #[derive(Debug, Clone, Serialize)]\n\
         pub struct A { pub x: u32 }\n\
         #[derive(Debug, Serialize)]\n\
         pub struct B { pub y: String }\n\
         #[derive(Serialize)]\n\
         pub struct C;\n\
         pub struct Plain { pub z: u8 }\n",
    );
    dir.path().to_string_lossy().to_string()
}

#[test]
fn refs_zero_result_redirects_to_impls_for_derive_traits() {
    let dir = tempdir().unwrap();
    let root = write_derive_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "refs", "Serialize"]);
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("looks like a trait"),
        "expected trait redirect, got stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("impls Serialize"),
        "expected suggestion `impls Serialize`, got stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("3 time(s)"),
        "expected derive count = 3, got stderr: {}",
        stderr
    );
}

#[test]
fn refs_zero_result_redirects_for_debug_too() {
    let dir = tempdir().unwrap();
    let root = write_derive_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "refs", "Debug"]);
    let stderr = stderr_of(&out);
    assert!(stderr.contains("impls Debug"), "stderr: {}", stderr);
    assert!(stderr.contains("2 time(s)"), "stderr: {}", stderr);
}

#[test]
fn refs_does_not_redirect_for_known_struct() {
    let dir = tempdir().unwrap();
    let root = write_derive_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "refs", "A"]);
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("looks like a trait"),
        "should NOT redirect for known struct, stderr: {}",
        stderr
    );
}

#[test]
fn refs_does_not_redirect_for_lowercase_ident() {
    let dir = tempdir().unwrap();
    let root = write_derive_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "refs", "nonexistent_fn"]);
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("looks like a trait"),
        "should NOT redirect for lowercase ident, stderr: {}",
        stderr
    );
}

#[test]
fn refs_does_not_redirect_when_kind_filter_set() {
    let dir = tempdir().unwrap();
    let root = write_derive_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "refs", "Serialize", "--kind", "path"]);
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("looks like a trait"),
        "should NOT redirect when --kind filter is explicit, stderr: {}",
        stderr
    );
}

#[test]
fn refs_zero_result_hint_dual_emits_to_stdout_text_mode() {


    let dir = tempdir().unwrap();
    let root = write_derive_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "refs", "Serialize"]);
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("looks like a trait"),
        "trait redirect must be ON STDOUT (text mode), got stdout: {}",
        stdout
    );
    assert!(
        stdout.contains("impls Serialize"),
        "stdout: {}",
        stdout
    );
}

#[test]
fn refs_zero_result_hint_skips_stdout_in_json_mode() {


    let dir = tempdir().unwrap();
    let root = write_derive_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "-j", "refs", "Serialize"]);
    let stdout = stdout_of(&out);
    let stderr = stderr_of(&out);
    assert!(
        !stdout.contains("looks like a trait"),
        "JSON mode stdout must NOT contain free-form notes, got stdout: {}",
        stdout
    );
    assert!(
        stderr.contains("looks like a trait"),
        "JSON mode keeps hints on stderr, got stderr: {}",
        stderr
    );
    assert!(
        stdout.trim_start().starts_with('{'),
        "stdout must be valid JSON, got: {}",
        stdout
    );
}

#[test]
fn refs_kind_field_hint_dual_emits_to_stdout() {


    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub struct Foo { pub x: u32, pub y: String }\npub fn bar(f: &Foo) -> u32 { f.x }\n",
    );
    let root = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &root, "refs", "Foo", "--kind", "field"]);
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("members Foo"),
        "members redirect must be ON STDOUT, got stdout: {}",
        stdout
    );
}

#[test]
fn refs_does_not_redirect_when_no_derive_sites() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub struct Foo;\npub fn bar() {}\n",
    );
    let root = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &root, "refs", "Nonexistent"]);
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("looks like a trait"),
        "should NOT redirect when no derive sites exist, stderr: {}",
        stderr
    );
}
