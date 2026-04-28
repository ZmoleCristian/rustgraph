

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


fn fixture_strict_zero_with_loose_fallback() -> tempfile::TempDir {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn foo() -> u32 { 1 }\n\
         pub fn caller_a() { let _ = foo(); }\n",
    );
    fixture
}


fn fixture_strict_succeeds() -> tempfile::TempDir {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct Daemon;\n\
         impl Daemon {\n\
             pub fn run(&self) {}\n\
         }\n\
         pub fn driver() {\n\
             let d = Daemon;\n\
             d.run();\n\
         }\n",
    );
    fixture
}


#[test]
fn callers_qualified_zero_falls_back_to_loose_with_stderr_note() {
    let fixture = fixture_strict_zero_with_loose_fallback();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "Bar::foo"]);
    assert!(
        out.status.success(),
        "callers should succeed via loose fallback. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);


    assert!(
        stdout.contains("caller_a"),
        "expected caller_a (loose fallback should rescue). stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    assert!(
        stderr.contains("loose name resolution") || stderr.contains("type-strict matching returned 0"),
        "expected fallback note on stderr. stderr:\n{stderr}"
    );
}


#[test]
fn callers_strict_success_emits_no_fallback_note() {
    let fixture = fixture_strict_succeeds();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "Daemon::run"]);
    assert!(
        out.status.success(),
        "callers should succeed. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);


    assert!(
        stdout.contains("driver"),
        "expected driver as caller of Daemon::run. stdout:\n{stdout}"
    );

    assert!(
        !stderr.contains("loose name resolution"),
        "PHANTOM: strict succeeded; no fallback note should appear. stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("suppressed by --strict-resolution"),
        "PHANTOM: no suppression note when strict succeeded. stderr:\n{stderr}"
    );
}


#[test]
fn callers_strict_resolution_flag_suppresses_fallback() {
    let fixture = fixture_strict_zero_with_loose_fallback();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "--strict-resolution", "callers", "Bar::foo"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);


    assert!(
        !out.status.success(),
        "--strict-resolution should error on strict-zero (no rescue). stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("suppressed by --strict-resolution"),
        "expected --strict-resolution suppression note. stderr:\n{stderr}"
    );

    assert!(
        !stdout.contains("caller_a"),
        "PHANTOM: --strict-resolution should drop caller_a (no fallback). stdout:\n{stdout}"
    );
}


#[test]
fn call_graph_multi_fallback_aggregates_counter() {


    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() {}\n\
         pub fn beta() {}\n\
         pub fn gamma() {}\n\
         pub fn delta() {}\n\
         pub fn driver() {\n\
             // Each call site uses a non-existent Type qualifier — strict\n\
             // zeroes, loose rescue rescues. Counter += 4.\n\
             Nope::alpha();\n\
             Nope::beta();\n\
             Nope::gamma();\n\
             Nope::delta();\n\
         }\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "call-graph", "--root", "driver"]);
    assert!(
        out.status.success(),
        "call-graph should succeed. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);


    let note_line = stderr
        .lines()
        .find(|line| line.contains("call sites") && line.contains("type-strict matching returned 0"))
        .or_else(|| stderr.lines().find(|line| line.contains("loose name resolution")));
    assert!(
        note_line.is_some(),
        "expected loose-fallback note for multi-fallback fixture. stderr:\n{stderr}"
    );

    let n: Option<usize> = note_line
        .and_then(|line| line.strip_prefix("note: "))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok());
    let count = n.expect("expected a leading number in the note");
    assert!(
        count >= 4,
        "expected >= 4 call site(s) flagged as loose-fallback (got {count}). stderr:\n{stderr}"
    );
}


#[test]
fn anti_oscillation_r23_w1_bare_name_loose_preserved() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/mod_a/lib.rs"),
        "pub fn foo() -> u32 { 1 }\n",
    );
    write_file(
        &fixture.path().join("src/mod_b/lib.rs"),
        "pub fn foo() -> u32 { 2 }\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod mod_a;\n\
         pub mod mod_b;\n\
         pub fn caller_a() { let _ = mod_a::foo(); }\n\
         pub fn caller_b() { let _ = mod_b::foo(); }\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "foo"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(stdout.contains("mod_a/lib.rs"), "anti-oscillation: bare query lost mod_a. stdout:\n{stdout}");
    assert!(stdout.contains("mod_b/lib.rs"), "anti-oscillation: bare query lost mod_b. stdout:\n{stdout}");
    assert!(stdout.contains("caller_a"), "anti-oscillation: lost caller_a. stdout:\n{stdout}");
    assert!(stdout.contains("caller_b"), "anti-oscillation: lost caller_b. stdout:\n{stdout}");
}


#[test]
fn anti_oscillation_r19_w2_strict_qualifier_preserved() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/daemon.rs"),
        "pub struct Daemon;\n\
         impl Daemon {\n\
             pub fn new() -> Self { Daemon }\n\
         }\n",
    );
    write_file(
        &fixture.path().join("src/session.rs"),
        "pub struct Session;\n\
         impl Session {\n\
             pub fn new() -> Self { Session }\n\
         }\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod daemon;\n\
         pub mod session;\n\
         use daemon::Daemon;\n\
         pub fn driver() { let _ = Daemon::new(); }\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "Daemon::new"]);
    assert!(
        out.status.success(),
        "callers Daemon::new should succeed (strict match). stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);


    assert!(
        stdout.contains("daemon.rs"),
        "anti-oscillation: expected Daemon::new match. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("session.rs"),
        "PHANTOM: Session::new should not appear under Daemon::new query. stdout:\n{stdout}"
    );

    assert!(
        !stderr.contains("loose name resolution"),
        "PHANTOM: strict succeeded; no fallback note. stderr:\n{stderr}"
    );
}


#[test]
fn paths_between_strict_resolution_no_op_when_strict_succeeds() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\n\
         pub fn driver() { target(); }\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out_default = run_rustgraph(&["--path", &base, "paths-between", "driver", "target"]);
    let out_strict = run_rustgraph(&["--path", &base, "--strict-resolution", "paths-between", "driver", "target"]);
    assert!(out_default.status.success());
    assert!(out_strict.status.success());
    let stdout_default = String::from_utf8_lossy(&out_default.stdout);
    let stdout_strict = String::from_utf8_lossy(&out_strict.stdout);
    let stderr_default = String::from_utf8_lossy(&out_default.stderr);
    let stderr_strict = String::from_utf8_lossy(&out_strict.stderr);


    assert!(stdout_default.contains("[1 path(s)]"), "default mode missing path. stdout:\n{stdout_default}");
    assert!(stdout_strict.contains("[1 path(s)]"), "strict mode missing path. stdout:\n{stdout_strict}");

    assert!(!stderr_default.contains("loose name resolution"), "default unexpectedly emitted note. stderr:\n{stderr_default}");
    assert!(!stderr_strict.contains("loose name resolution"), "strict unexpectedly emitted note. stderr:\n{stderr_strict}");
    assert!(!stderr_strict.contains("suppressed by --strict-resolution"), "strict unexpectedly emitted suppression note. stderr:\n{stderr_strict}");
}


#[test]
fn cli_accepts_strict_resolution_flag() {
    let fixture = fixture_strict_succeeds();
    let base = fixture.path().to_string_lossy().to_string();


    let out_before = run_rustgraph(&["--path", &base, "--strict-resolution", "callers", "Daemon::run"]);
    assert!(
        out_before.status.success(),
        "expected --strict-resolution before subcommand to parse. stderr:\n{}",
        String::from_utf8_lossy(&out_before.stderr)
    );


    let out_after = run_rustgraph(&["--path", &base, "callers", "Daemon::run", "--strict-resolution"]);
    assert!(
        out_after.status.success(),
        "expected --strict-resolution after subcommand to parse. stderr:\n{}",
        String::from_utf8_lossy(&out_after.stderr)
    );
}
