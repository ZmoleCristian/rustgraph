

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


fn fixture_homonym_methods() -> tempfile::TempDir {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct Daemon { pub x: u32 }\n\
         impl Daemon { pub fn run_cli() -> u32 { 1 } }\n\
         pub struct Session { pub y: u32 }\n\
         impl Session { pub fn run_cli() -> u32 { 2 } }\n\
         pub fn caller_d() { let _ = Daemon::run_cli(); }\n\
         pub fn caller_s() { let _ = Session::run_cli(); }\n",
    );
    fixture
}


#[test]
fn regression_callers_bare_name_returns_all_matches_loose() {
    let fixture = fixture_homonym_methods();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "run_cli"]);
    assert!(
        out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("src/lib.rs:2-2"),
        "expected Daemon::run_cli (lib.rs:2) match. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("src/lib.rs:4-4"),
        "expected Session::run_cli (lib.rs:4) match. stdout:\n{stdout}"
    );

    assert!(
        stdout.contains("caller_d") && stdout.contains("caller_s"),
        "expected both caller_d and caller_s in loose-mode output. stdout:\n{stdout}"
    );
}


#[test]
fn regression_callers_qualified_type_method_only_returns_owning_type() {
    let fixture = fixture_homonym_methods();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "Daemon::run_cli"]);
    assert!(
        out.status.success(),
        "callers should succeed for `Daemon::run_cli`. stderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("src/lib.rs:2"),
        "expected Daemon::run_cli (lib.rs:2) in qualified output. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("src/lib.rs:4-4"),
        "Session::run_cli (lib.rs:4-4) is a phantom homonym; strict-qualified resolution must drop it. stdout:\n{stdout}"
    );

    assert!(
        stdout.contains("caller_d"),
        "expected caller_d in Daemon::run_cli output. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("caller_s"),
        "caller_s only calls Session::run_cli; must not appear under Daemon::run_cli. stdout:\n{stdout}"
    );
}


#[test]
fn regression_paths_between_qualified_to_resolves_only_target_type() {
    let fixture = fixture_homonym_methods();
    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&[
        "--path", &base, "paths-between", "caller_d", "Daemon::run_cli",
    ]);
    assert!(
        out.status.success(),
        "paths-between should succeed for qualified TO. stderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("[1 path(s)]"),
        "expected exactly 1 path for caller_d → Daemon::run_cli. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("src/lib.rs:2"),
        "expected Daemon::run_cli (line 2) in path. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("src/lib.rs:4"),
        "Session::run_cli (line 4) is a phantom; strict-qualified resolution must drop it. stdout:\n{stdout}"
    );


    let out2 = run_rustgraph(&[
        "--path", &base, "paths-between", "caller_d", "run_cli",
    ]);
    assert!(
        out2.status.success(),
        "paths-between should succeed for loose TO. stderr:\n{}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let stdout2 = String::from_utf8_lossy(&out2.stdout);
    assert!(
        stdout2.contains("[1 path(s)]"),
        "PHANTOM EDGE: caller_d only calls Daemon::run_cli; expected exactly 1 path, got:\n{stdout2}"
    );
    assert!(
        !stdout2.contains("src/lib.rs:4"),
        "PHANTOM EDGE: Session::run_cli (line 4) appeared in path from caller_d, which only calls Daemon::run_cli. stdout:\n{stdout2}"
    );
}


#[test]
fn regression_paths_between_anti_oscillation_loose_target_still_resolves() {
    let fixture = fixture_homonym_methods();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path", &base, "paths-between", "caller_s", "run_cli",
    ]);
    assert!(
        out.status.success(),
        "paths-between should succeed for caller_s with loose TO. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("[1 path(s)]"),
        "expected exactly 1 path for caller_s → run_cli. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("src/lib.rs:4"),
        "expected Session::run_cli (line 4) in path from caller_s. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("src/lib.rs:2"),
        "Daemon::run_cli (line 2) is unreachable from caller_s; must not appear. stdout:\n{stdout}"
    );


    let out2 = run_rustgraph(&["--path", &base, "callers", "run_cli"]);
    assert!(
        out2.status.success(),
        "callers should succeed for bare `run_cli`. stderr:\n{}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let stdout2 = String::from_utf8_lossy(&out2.stdout);
    assert!(
        stdout2.contains("src/lib.rs:2-2") && stdout2.contains("src/lib.rs:4-4"),
        "bare query must keep loose match (both run_cli's). stdout:\n{stdout2}"
    );
}
