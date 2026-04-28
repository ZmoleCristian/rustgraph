

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
fn regression_refs_max_results_default_caps_at_50() {
    let fixture = tempdir().expect("tempdir");
    let mut src = String::from("pub fn widget_target() {}\n");
    src.push_str("pub fn driver() {\n");
    for _ in 0..60 {
        src.push_str("    let _ = widget_target();\n");
    }
    src.push_str("}\n");
    write_file(&fixture.path().join("src/lib.rs"), &src);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "refs", "widget_target"]);
    assert!(
        out.status.success(),
        "refs failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();


    let body_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains(" [call]") || l.contains(" [path]"))
        .collect();
    assert!(
        body_lines.len() <= 50,
        "default --max-results should cap to 50; got {} lines:\n{}",
        body_lines.len(),
        stdout
    );


    assert!(
        stdout.contains("showing") && stdout.contains("of 60"),
        "expected truncation footer mentioning the original 60; got:\n{stdout}"
    );
    assert!(
        stdout.contains("60 reference(s)"),
        "header should show full count of 60 references; got:\n{stdout}"
    );
}


#[test]
fn regression_refs_max_results_zero_unlimited() {
    let fixture = tempdir().expect("tempdir");
    let mut src = String::from("pub fn widget_target() {}\n");
    src.push_str("pub fn driver() {\n");
    for _ in 0..60 {
        src.push_str("    let _ = widget_target();\n");
    }
    src.push_str("}\n");
    write_file(&fixture.path().join("src/lib.rs"), &src);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "refs",
        "widget_target",
        "--max-results",
        "0",
    ]);
    assert!(
        out.status.success(),
        "refs failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    let body_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains(" [call]") || l.contains(" [path]"))
        .collect();
    assert!(
        body_lines.len() >= 60,
        "expected >=60 ref lines (all 60 unfiltered); got {}\nstdout:\n{}",
        body_lines.len(),
        stdout
    );

    assert!(
        !stdout.contains("[showing"),
        "no truncation footer expected at --max-results 0; got:\n{stdout}"
    );
}


#[test]
fn regression_dead_code_max_results_caps() {
    let fixture = tempdir().expect("tempdir");
    let mut src = String::new();
    for i in 0..55 {
        src.push_str(&format!("pub fn dead_{i:02}() {{}}\n"));
    }
    write_file(&fixture.path().join("src/lib.rs"), &src);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "dead-code",
        "--max-results",
        "10",
    ]);
    assert!(
        out.status.success(),
        "dead-code failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();


    let body_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains("pub fn dead_") && l.contains("0 calls"))
        .collect();
    assert!(
        body_lines.len() <= 10,
        "expected ≤ 10 candidate lines under --max-results 10; got {}:\n{}",
        body_lines.len(),
        stdout
    );

    assert!(
        stdout.contains("showing") && stdout.contains("of 55"),
        "expected truncation footer mentioning original 55; got:\n{stdout}"
    );


    assert!(
        stdout.contains("55 dead") || stdout.contains("55 candidates"),
        "summary should still reflect the un-capped total; got:\n{stdout}"
    );
}


#[test]
fn regression_impls_max_results_caps() {
    let fixture = tempdir().expect("tempdir");
    let mut src = String::new();
    for i in 0..30 {
        src.push_str(&format!("pub struct Wrap{i:02};\n"));
        src.push_str(&format!(
            "impl std::fmt::Display for Wrap{i:02} {{ fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {{ Ok(()) }} }}\n"
        ));
    }
    write_file(&fixture.path().join("src/lib.rs"), &src);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "impls",
        "Display",
        "--max-results",
        "5",
    ]);
    assert!(
        out.status.success(),
        "impls failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();


    let body_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains("Wrap") && l.contains("[handwritten]"))
        .collect();
    assert!(
        body_lines.len() <= 5,
        "expected ≤ 5 impls listed under --max-results 5; got {}:\n{}",
        body_lines.len(),
        stdout
    );

    assert!(
        stdout.contains("showing") && stdout.contains("of 30"),
        "expected truncation footer mentioning original 30; got:\n{stdout}"
    );


    assert!(
        stdout.contains("30 total"),
        "header should still report 30 total impls; got:\n{stdout}"
    );
}
