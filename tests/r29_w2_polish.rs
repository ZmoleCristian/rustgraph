

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


#[test]
fn callers_flat_emits_path_line_name_per_caller() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn target() {}\nfn caller_a() { target(); }\nfn caller_b() { target(); }\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "target", "--flat"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);

    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(!lines.is_empty(), "expected at least one flat entry; got: {stdout}");
    for line in &lines {
        let parts: Vec<&str> = line.splitn(3, ':').collect();
        assert_eq!(parts.len(), 3, "expected path:line:name format; got: {line}");
    }

    assert!(stdout.contains("caller_a"), "caller_a missing: {stdout}");
    assert!(stdout.contains("caller_b"), "caller_b missing: {stdout}");
}


#[test]
fn callers_flat_depth2_deduplicates() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),


        "pub fn target() {}\nfn mid() { target(); }\nfn shared_caller() { mid(); target(); }\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "target", "--flat", "--depth", "2"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();

    let shared_count = lines.iter().filter(|l| l.contains("shared_caller")).count();
    assert_eq!(shared_count, 1, "shared_caller should appear once (dedup); got: {stdout}");
}


#[test]
fn callers_flat_conflicts_with_json() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn target() {}\nfn caller() { target(); }\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "target", "--flat", "-j"]);

    assert!(!out.status.success(), "expected error for --flat -j combo");
}


#[test]
fn callers_flat_with_depth_includes_transitive() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn target() {}\nfn level1() { target(); }\nfn level2() { level1(); }\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "target", "--flat", "--depth", "2"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("level1"), "level1 missing: {stdout}");
    assert!(stdout.contains("level2"), "level2 missing: {stdout}");
}


#[test]
fn find_auto_relax_fires_at_default_threshold() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),


        "pub fn fetch_data() {}\npub fn store_data() {}\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "feech_deta"]);
    let stderr = stderr_of(&out);
    let stdout = stdout_of(&out);

    assert!(
        stderr.contains("relaxed to 0.7"),
        "expected auto-relax note on stderr; got: {stderr}"
    );

    assert!(
        stdout.contains("relaxed to 0.7"),
        "expected auto-relax note on stdout (dual-emit); got: {stdout}"
    );

    assert!(out.status.success(), "expected exit 0 when relaxed candidates found");
}


#[test]
fn find_auto_relax_skips_when_threshold_explicit() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn fetch_data() {}\n",
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path", &base, "--search-threshold", "0.95", "find", "feech_deta",
    ]);
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("relaxed to 0.7"),
        "auto-relax should NOT fire when threshold != 0.85; got: {stderr}"
    );
    assert!(!out.status.success(), "expected non-zero exit for genuine no-match");
}


#[test]
fn find_auto_relax_skips_short_query() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn run_event() {}\n",
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "xyz"]);
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("relaxed to 0.7"),
        "auto-relax should NOT fire for 3-char query; got: {stderr}"
    );
}


#[test]
fn find_auto_relax_respects_selection_filter() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),


        "pub struct FetchData;\npub fn unrelated() {}\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "feech_data", "--func"]);

    assert!(!out.status.success(), "expected non-zero: only struct candidate filtered by --func");
    let stderr = stderr_of(&out);

    if stderr.contains("relaxed to 0.7") {
        assert!(!out.status.success(), "auto-relax with 0 fn candidates after filter must exit non-zero");
    }
}


#[test]
fn grep_unwrap_paren_pattern_is_valid() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"
pub fn foo(x: Option<i32>) -> i32 {
    x.unwrap()
}
pub fn bar() {}
"#,
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "grep", r"\.unwrap\(\)"]);
    assert!(out.status.success(), r"\.unwrap\(\) should be valid pattern; stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("unwrap"), "expected match on unwrap(); got: {stdout}");
}


#[test]
fn grep_unwrap_word_boundary_pattern_is_valid() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"
pub fn foo(x: Option<i32>) -> i32 {
    x.unwrap()
}
pub fn bar() {}
"#,
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "grep", r"\.unwrap\b"]);
    assert!(out.status.success(), r"\.unwrap\b should be valid; stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("unwrap"), "expected match; got: {stdout}");
}


#[test]
fn callers_ambiguity_shows_preview_for_2_matches() {
    let dir = tempdir().unwrap();


    write_file(
        &dir.path().join("src/alpha.rs"),
        "pub fn run_cli() -> i32 {\n    let x = 1;\n    x + 2\n}\n",
    );
    write_file(
        &dir.path().join("src/beta.rs"),
        "pub fn run_cli() -> u32 {\n    let y = 99;\n    y\n}\n",
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path", &base, "callers", "run_cli", "--ambiguity-cap", "1",
    ]);
    assert!(!out.status.success(), "expected non-zero (ambiguous)");
    let stderr = stderr_of(&out);

    assert!(stderr.contains("| "), "expected body preview lines (| prefix); got: {stderr}");

    assert!(stderr.contains("--symbol-id"), "expected --symbol-id hints; got: {stderr}");

    assert!(stderr.contains("ambiguous: 2"), "expected 'ambiguous: 2' header; got: {stderr}");
}


#[test]
fn callers_ambiguity_no_preview_when_many_matches() {
    let dir = tempdir().unwrap();

    for (i, letter) in ["a","b","c","d","e","f"].iter().enumerate() {
        write_file(
            &dir.path().join(format!("src/{letter}.rs")),
            &format!("pub fn shared_fn() -> u{i}  {{ 0 }}\n"),
        );
    }
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path", &base, "callers", "shared_fn", "--ambiguity-cap", "1",
    ]);
    assert!(!out.status.success(), "expected non-zero (ambiguous)");
    let stderr = stderr_of(&out);

    assert!(!stderr.contains("| "), "expected no preview when >5 matches; got: {stderr}");

    assert!(stderr.contains("--symbol-id"), "expected candidate list; got: {stderr}");
}


#[test]
fn ensemble_ambiguity_shows_preview_for_2_matches() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/a.rs"),
        "pub fn dispatch() {\n    let ctx = 1;\n    println!(\"{ctx}\");\n}\n",
    );
    write_file(
        &dir.path().join("src/b.rs"),
        "pub fn dispatch() {\n    let msg = \"hello\";\n    println!(\"{msg}\");\n}\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path", &base, "ensemble", "dispatch", "--ambiguity-cap", "1",
    ]);
    assert!(!out.status.success(), "expected non-zero (ambiguous)");
    let stderr = stderr_of(&out);
    assert!(stderr.contains("| "), "expected body preview (| prefix) for ensemble; got: {stderr}");
    assert!(stderr.contains("--symbol-id"), "expected --symbol-id hints; got: {stderr}");
}
