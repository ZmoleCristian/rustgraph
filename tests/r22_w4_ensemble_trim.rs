

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

fn small_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"
pub fn r22_callee_fn(x: u32) -> u32 { x + 1 }

pub fn r22_target_fn(x: u32) -> u32 {
    let y = r22_callee_fn(x);
    y + 2
}

pub fn r22_caller_fn() {
    let _ = r22_target_fn(99);
}
"#,
    );
    dir
}


#[test]
fn r22_summary_default_starts_with_marker_block() {
    let fixture = small_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "ensemble", "r22_target_fn"]);
    assert!(
        out.status.success(),
        "default ensemble failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);


    let first_nonempty = stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .expect("expected at least one non-empty line of stdout");
    assert!(
        first_nonempty.starts_with("[summary view"),
        "expected first non-empty line to start with `[summary view`; got first line:\n{}\n--- full stdout ---\n{}",
        first_nonempty,
        stdout
    );


    let marker_pos = stdout
        .find("[summary view")
        .expect("expected [summary view marker block in default output");
    let signature_pos = stdout
        .find("r22_target_fn")
        .expect("expected target fn signature line in output");
    assert!(
        marker_pos < signature_pos,
        "expected [summary view] marker to appear BEFORE the target signature; marker_pos={} signature_pos={}\n--- stdout ---\n{}",
        marker_pos,
        signature_pos,
        stdout
    );
}


#[test]
fn r22_view_full_suppresses_marker_at_top() {
    let fixture = small_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "ensemble", "r22_target_fn", "--view", "full",
    ]);
    assert!(
        out.status.success(),
        "--view full ensemble failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);


    assert!(
        !stdout.contains("[summary view"),
        "expected NO `[summary view` block under --view full; got:\n{}",
        stdout
    );

    let first_nonempty = stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .expect("expected at least one non-empty line of stdout");
    assert!(
        first_nonempty.contains("r22_target_fn"),
        "expected first non-empty line to be the target signature under --view full; got:\n{}\n--- full stdout ---\n{}",
        first_nonempty,
        stdout
    );
}


#[test]
fn r22_section_override_uses_showing_hint_not_summary_marker() {
    let fixture = small_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "ensemble", "r22_target_fn",
        "--section", "neighborhood",
    ]);
    assert!(
        out.status.success(),
        "--section ensemble failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);


    assert!(
        !stdout.contains("[summary view"),
        "expected NO `[summary view` block when --section is in effect; got:\n{}",
        stdout
    );

    let first_nonempty = stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .expect("expected at least one non-empty line of stdout");
    assert!(
        first_nonempty.starts_with("[showing:"),
        "expected first non-empty line to start with `[showing:` under --section; got:\n{}\n--- full stdout ---\n{}",
        first_nonempty,
        stdout
    );
    assert!(
        first_nonempty.contains("neighborhood"),
        "expected `[showing:]` line to mention `neighborhood`; got:\n{}",
        first_nonempty
    );
}


#[test]
fn r22_marker_block_content_invariants_held() {
    let fixture = small_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "ensemble", "r22_target_fn"]);
    assert!(
        out.status.success(),
        "default ensemble failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);


    let block_start = stdout
        .find("[summary view")
        .expect("expected `[summary view` block in default output");
    let block_end_rel = stdout[block_start..]
        .find(']')
        .expect("expected the marker block to be terminated by `]`");
    let block = &stdout[block_start..block_start + block_end_rel + 1];

    assert!(
        block.contains("--view full"),
        "expected marker block to mention `--view full`; got block:\n{}",
        block
    );
    assert!(
        block.contains("--view flow"),
        "expected marker block to mention `--view flow`; got block:\n{}",
        block
    );
    assert!(
        block.contains("--section"),
        "expected marker block to mention `--section`; got block:\n{}",
        block
    );

    assert!(
        !block.contains("--ensemble-preset"),
        "marker block must not mention `--ensemble-preset` (top-level legacy flag, not on EnsembleCommand); got block:\n{}",
        block
    );
    assert!(
        !block.to_lowercase().contains("balanced"),
        "marker block must not mention `balanced` (no `EnsembleView::Balanced` variant; would mislead users into typing `--view balanced`); got block:\n{}",
        block
    );
}


#[test]
fn r22_summary_default_still_shows_navigable_triplet() {
    let fixture = small_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "ensemble", "r22_target_fn"]);
    assert!(
        out.status.success(),
        "default ensemble failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("r22_target_fn"),
        "expected target fn name in default summary output; got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("r22_caller_fn"),
        "expected caller name `r22_caller_fn` in default summary output (neighborhood section); got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("r22_callee_fn"),
        "expected callee name `r22_callee_fn` in default summary output (neighborhood / dataflow section); got:\n{}",
        stdout
    );
}
