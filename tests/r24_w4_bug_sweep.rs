

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

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}


fn ensemble_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"
pub fn r24_callee(x: u32) -> u32 { x + 1 }

pub fn r24_target(x: u32) -> u32 {
    let y = r24_callee(x);
    y + 2
}

pub fn r24_caller() {
    let _ = r24_target(7);
}
"#,
    );
    dir
}


#[test]
fn fix1_ensemble_default_no_view_still_shows_marker() {
    let fixture = ensemble_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &base, "ensemble", "r24_target"]);
    assert!(
        out.status.success(),
        "ensemble (default) failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("[summary view"),
        "default ensemble (no --view) MUST show the marker block; got:\n{}",
        stdout
    );
    let first_nonempty = stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .expect("at least one non-empty line");
    assert!(
        first_nonempty.starts_with("[summary view"),
        "marker must be the first non-empty line of stdout; got:\n{}\n--- full stdout ---\n{}",
        first_nonempty,
        stdout
    );
}


#[test]
fn fix1_ensemble_explicit_view_summary_suppresses_marker() {
    let fixture = ensemble_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &base, "ensemble", "r24_target", "--view", "summary"]);
    assert!(
        out.status.success(),
        "ensemble --view summary failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("[summary view"),
        "explicit --view summary MUST NOT show the marker block; got:\n{}",
        stdout
    );

    assert!(
        stdout.contains("r24_target"),
        "explicit --view summary still renders the target; got:\n{}",
        stdout
    );
}


#[test]
fn fix1_ensemble_view_full_suppresses_marker() {
    let fixture = ensemble_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &base, "ensemble", "r24_target", "--view", "full"]);
    assert!(
        out.status.success(),
        "ensemble --view full failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("[summary view"),
        "--view full MUST NOT show the marker block; got:\n{}",
        stdout
    );
}


#[test]
fn fix1_ensemble_section_override_uses_showing_hint() {
    let fixture = ensemble_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "-p", &base, "ensemble", "r24_target",
        "--section", "neighborhood",
    ]);
    assert!(
        out.status.success(),
        "ensemble --section failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("[summary view"),
        "--section MUST NOT show the marker block; got:\n{}",
        stdout
    );
    let first_nonempty = stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .expect("at least one non-empty line");
    assert!(
        first_nonempty.starts_with("[showing:"),
        "first line should be the `[showing: ...]` hint; got:\n{}\n--- full stdout ---\n{}",
        first_nonempty,
        stdout
    );
    assert!(
        first_nonempty.contains("neighborhood"),
        "[showing] hint must mention `neighborhood`; got:\n{}",
        first_nonempty
    );
}


fn paths_between_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(&dir.path().join("src/lib.rs"), "pub mod api;\npub mod session;\n");
    write_file(
        &dir.path().join("src/api/mod.rs"),
        r#"
use crate::session::commands::execute_cut;

pub fn handle_request() {
    execute_cut();
}
"#,
    );
    write_file(&dir.path().join("src/session/mod.rs"), "pub mod commands;\n");
    write_file(
        &dir.path().join("src/session/commands/mod.rs"),
        r#"
pub fn execute_cut() {
    println!("cut");
}
"#,
    );
    dir
}


#[test]
fn fix2_paths_between_to_module_full_path_prefix() {
    let fixture = paths_between_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "-p", &base,
        "paths-between", "handle_request",
        "--to-module", "src/session/commands",
    ]);
    assert!(
        out.status.success(),
        "paths-between --to-module 'src/session/commands' failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("[1 path(s)]"),
        "expected exactly 1 path; got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("execute_cut"),
        "path must include the destination fn `execute_cut`; got:\n{}",
        stdout
    );
}


#[test]
fn fix2_paths_between_to_module_no_src_prefix() {
    let fixture = paths_between_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "-p", &base,
        "paths-between", "handle_request",
        "--to-module", "session/commands",
    ]);
    assert!(
        out.status.success(),
        "paths-between --to-module 'session/commands' failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("[1 path(s)]"),
        "expected exactly 1 path; got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("execute_cut"),
        "path must include `execute_cut`; got:\n{}",
        stdout
    );
}


#[test]
fn fix2_paths_between_to_module_single_segment() {
    let fixture = paths_between_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "-p", &base,
        "paths-between", "handle_request",
        "--to-module", "commands",
    ]);
    assert!(
        out.status.success(),
        "paths-between --to-module 'commands' failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("[1 path(s)]"),
        "expected 1 path; got:\n{}",
        stdout
    );
}


#[test]
fn fix2_paths_between_to_module_no_match_errors_clearly() {
    let fixture = paths_between_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "-p", &base,
        "paths-between", "handle_request",
        "--to-module", "no_such_module_xyz",
    ]);
    assert!(
        !out.status.success(),
        "no-match --to-module must exit non-zero; got status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("no_such_module_xyz"),
        "error must mention the missing substring; got:\n{}",
        stderr
    );
}


fn or_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),


        r#"
pub fn parse(_input: &str) -> u32 { 0 }

pub fn analyze_one() {}
pub fn analyze_two() {}
pub fn analyze_three() {}

pub fn unrelated_fn() {}
"#,
    );
    dir
}


#[test]
fn fix3_find_or_unions_hits_across_alternatives() {
    let fixture = or_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &base, "find", "parse|analyze"]);
    assert!(
        out.status.success(),
        "find 'parse|analyze' failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("fn parse"),
        "expected `parse` (exact alt 1); got:\n{}",
        stdout
    );
    for alt in &["analyze_one", "analyze_two", "analyze_three"] {
        assert!(
            stdout.contains(alt),
            "expected `{}` (fuzzy alt 2 union); got:\n{}",
            alt,
            stdout
        );
    }
    assert!(
        !stdout.contains("unrelated_fn"),
        "`unrelated_fn` matches neither alt; must NOT appear; got:\n{}",
        stdout
    );
}


#[test]
fn fix3_find_or_unions_with_func_selector() {
    let fixture = or_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &base, "find", "parse|analyze", "--func"]);
    assert!(
        out.status.success(),
        "find --func failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(stdout.contains("fn parse"), "got:\n{}", stdout);
    assert!(stdout.contains("analyze_one"), "got:\n{}", stdout);
    assert!(stdout.contains("analyze_two"), "got:\n{}", stdout);
    assert!(stdout.contains("analyze_three"), "got:\n{}", stdout);
}


#[test]
fn fix3_find_or_with_exact_returns_both_alts() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn cut() {}\npub fn delete() {}\npub fn unrelated() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &base, "find", "cut|delete", "--exact"]);
    assert!(
        out.status.success(),
        "find 'cut|delete' --exact failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(stdout.contains("fn cut"), "expected `cut`; got:\n{}", stdout);
    assert!(stdout.contains("fn delete"), "expected `delete`; got:\n{}", stdout);
    assert!(
        !stdout.contains("unrelated"),
        "`unrelated` must NOT appear in --exact mode; got:\n{}",
        stdout
    );
}


#[test]
fn fix3_find_single_term_keeps_exact_name_fast_path() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn load() {}
pub fn load_jsonl() {}
pub fn preload() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &base, "find", "load"]);
    assert!(
        out.status.success(),
        "find 'load' failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(stdout.contains("fn load"), "expected exact `load`; got:\n{}", stdout);
    assert!(
        !stdout.contains("load_jsonl"),
        "single-term fast path MUST suppress `load_jsonl`; got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("preload"),
        "single-term fast path MUST suppress `preload`; got:\n{}",
        stdout
    );
}
