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

#[test]
fn regression_ensemble_summary_default_is_navigable() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn navigable_callee_fn(x: u32) -> u32 { x + 1 }

pub fn navigable_target_fn(x: u32) -> u32 {
    let y = navigable_callee_fn(x);
    y + 2
}

pub fn navigable_caller_fn() {
    let _ = navigable_target_fn(99);
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "ensemble", "navigable_target_fn"]);
    assert!(
        out.status.success(),
        "default ensemble failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("navigable_target_fn"),
        "expected target fn name in output, got:\n{}",
        stdout
    );

    assert!(
        stdout.contains("navigable_caller_fn"),
        "expected caller name 'navigable_caller_fn' in default summary; got:\n{}",
        stdout
    );

    assert!(
        stdout.contains("navigable_callee_fn"),
        "expected callee name 'navigable_callee_fn' in default summary; got:\n{}",
        stdout
    );

    let lower = stdout.to_lowercase();
    assert!(
        lower.contains("summary view"),
        "expected 'summary view' marker block label; got:\n{}",
        stdout
    );

    assert!(
        lower.contains("flow"),
        "expected 'flow' keyword in truncation marker; got:\n{}",
        stdout
    );
    assert!(
        lower.contains("full"),
        "expected 'full' keyword in truncation marker; got:\n{}",
        stdout
    );
    assert!(
        lower.contains("--section"),
        "expected '--section' keyword in truncation marker; got:\n{}",
        stdout
    );
}

#[test]
fn regression_ensemble_summary_shows_truncation_markers() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn down_a() {}
pub fn down_b() {}
pub fn down_c() {}

pub fn target_fan(x: u32) -> u32 {
    down_a();
    down_b();
    down_c();
    x
}

pub fn caller_one() { let _ = target_fan(1); }
pub fn caller_two() { let _ = target_fan(2); }
pub fn caller_three() { let _ = target_fan(3); }
pub fn caller_four() { let _ = target_fan(4); }
pub fn caller_five() { let _ = target_fan(5); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "ensemble", "target_fan"]);
    assert!(
        out.status.success(),
        "ensemble failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    let lower = stdout.to_lowercase();
    assert!(
        lower.contains("summary view"),
        "expected 'summary view' marker label; got:\n{}",
        stdout
    );

    let any_upgrade =
        lower.contains("flow") || lower.contains("full") || lower.contains("--section");
    assert!(
        any_upgrade,
        "expected at least one of 'flow', 'full', '--section' in marker; got:\n{}",
        stdout
    );
}

#[test]
fn regression_ensemble_full_view_does_not_show_markers() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn full_callee() {}
pub fn full_target() { full_callee(); }
pub fn full_caller() { full_target(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "ensemble", "full_target", "--view", "full"]);
    assert!(
        out.status.success(),
        "ensemble --view full failed: stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("summary view"),
        "expected NO 'summary view' marker block under --view full; got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("sections hidden"),
        "expected NO 'sections hidden' marker text under --view full; got:\n{}",
        stdout
    );
}

#[test]
fn regression_ensemble_default_view_is_summary_not_full() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn body_of_summary() {
    let x = 1;
    let y = 2;
    let z = x + y;
    let _ = z * 3;
}
pub fn caller_keeps_summary_alive() { body_of_summary(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let default_out = run_rustgraph(&["-p", &base, "ensemble", "body_of_summary"]);
    assert!(
        default_out.status.success(),
        "default ensemble failed: {}",
        stderr_of(&default_out)
    );
    let default_stdout = stdout_of(&default_out);
    assert!(
        !default_stdout.contains("\nCode:\n"),
        "default summary view should NOT include the full code body; got:\n{}",
        default_stdout
    );

    let full_out = run_rustgraph(&["-p", &base, "ensemble", "body_of_summary", "--view", "full"]);
    assert!(
        full_out.status.success(),
        "ensemble --view full failed: {}",
        stderr_of(&full_out)
    );
    let full_stdout = stdout_of(&full_out);
    assert!(
        full_stdout.contains("\nCode:\n"),
        "expected Code section in --view full output; got:\n{}",
        full_stdout
    );
}

#[test]
fn regression_call_graph_text_hides_external_callees_in_tree() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn root_fn_for_test() {
    internal_fn_keep();
    let _: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
    let _ = format!("noise");
}

pub fn internal_fn_keep() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph", "--root", "root_fn_for_test"]);
    assert!(
        out.status.success(),
        "call-graph failed: {}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("internal_fn_keep"),
        "expected internal callee in default tree; got:\n{}",
        stdout
    );

    assert!(
        !stdout.contains("HashMap"),
        "expected 'HashMap' to be hidden in default tree (external callee); got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("format!") && !stdout.contains(" format ") && !stdout.contains("→ format"),
        "expected 'format!' to be hidden in default tree (external callee); got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("<external>"),
        "expected NO `<external>` rollup line in default tree; got:\n{}",
        stdout
    );

    let out_ext = run_rustgraph(&[
        "-p",
        &base,
        "call-graph",
        "--root",
        "root_fn_for_test",
        "--show-external",
    ]);
    assert!(
        out_ext.status.success(),
        "call-graph --show-external failed: {}",
        stderr_of(&out_ext)
    );
    let stdout_ext = stdout_of(&out_ext);
    assert!(
        stdout_ext.contains("<external>"),
        "expected `<external>` rollup line under --show-external; got:\n{}",
        stdout_ext
    );

    assert!(
        stdout_ext.contains("HashMap") || stdout_ext.contains("format"),
        "expected at least one external callee under --show-external; got:\n{}",
        stdout_ext
    );
}

#[test]
fn regression_callers_depth_2_shows_call_site_line_per_node() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
fn target_for_d2() {}
fn lvl1_for_d2() { target_for_d2(); }
fn lvl2_for_d2() { lvl1_for_d2(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "callers", "target_for_d2", "--depth", "2"]);
    assert!(
        out.status.success(),
        "callers --depth 2 failed: {}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("[d=1, 1 site(s)]"),
        "expected '[d=1, 1 site(s)]' summary; got:\n{}",
        stdout
    );

    assert!(
        stdout.contains("[d=2, 1 site(s)]"),
        "expected '[d=2, 1 site(s)]' summary; got:\n{}",
        stdout
    );

    let saw_lvl1_callsite = stdout
        .lines()
        .any(|l| l.contains("target_for_d2()") && !l.contains("[d="));
    assert!(
        saw_lvl1_callsite,
        "expected a call-site line showing `target_for_d2()` text under d=1 node; got:\n{}",
        stdout
    );

    let saw_lvl2_callsite = stdout
        .lines()
        .any(|l| l.contains("lvl1_for_d2()") && !l.contains("[d="));
    assert!(
        saw_lvl2_callsite,
        "expected a call-site line showing `lvl1_for_d2()` text under d=2 node; got:\n{}",
        stdout
    );

    let saw_line_3 = stdout
        .lines()
        .any(|l| l.contains("3:") && l.contains("target_for_d2") && !l.contains("[d="));
    let saw_line_4 = stdout
        .lines()
        .any(|l| l.contains("4:") && l.contains("lvl1_for_d2") && !l.contains("[d="));
    assert!(
        saw_line_3,
        "expected call-site line `3:<col> - ... target_for_d2 ...`; got:\n{}",
        stdout
    );
    assert!(
        saw_line_4,
        "expected call-site line `4:<col> - ... lvl1_for_d2 ...`; got:\n{}",
        stdout
    );
}
