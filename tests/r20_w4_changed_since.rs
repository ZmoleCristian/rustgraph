use serde_json::Value;
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

fn run_rustgraph_in(cwd: &Path, args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run rustgraph")
}

fn git(cwd: &Path, args: &[&str]) -> Output {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap_or_else(|e| panic!("failed to run git {:?}: {}", args, e));
    if !out.status.success() {
        panic!(
            "git {:?} failed: stdout={} stderr={}",
            args,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    out
}

fn init_repo(path: &Path) {
    git(path, &["init", "-q", "--initial-branch=main"]);
    git(path, &["config", "user.email", "test@example.com"]);
    git(path, &["config", "user.name", "test"]);
    git(path, &["config", "commit.gpgsign", "false"]);
}

fn commit_all(path: &Path, msg: &str) {
    git(path, &["add", "-A"]);
    git(path, &["commit", "-q", "-m", msg, "--no-verify"]);
}

#[test]
fn regression_changed_filters_find_to_modified_fns() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    init_repo(root);
    write_file(
        &root.join("src/lib.rs"),
        "pub fn a() { println!(\"v1\"); }\npub fn b() { println!(\"b\"); }\n",
    );
    commit_all(root, "initial");

    write_file(
        &root.join("src/lib.rs"),
        "pub fn a() {\n    println!(\"v2\");\n    println!(\"another\");\n}\npub fn b() { println!(\"b\"); }\n",
    );
    commit_all(root, "touch a");

    let out = run_rustgraph_in(
        root,
        &[
            "--no-auto-path",
            "-p",
            ".",
            "find",
            "a|b",
            "--search-threshold",
            "0.5",
            "--changed",
            "--since",
            "HEAD~1",
            "-j",
        ],
    );
    assert!(
        out.status.success(),
        "find --changed should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("expected valid json; got: {} (err: {})", stdout, e);
    });
    let empty = Vec::new();
    let names: Vec<&str> = v["functions"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .map(|f| f["name"].as_str().unwrap_or(""))
        .collect();
    assert!(
        names.contains(&"a"),
        "expected `a` (modified) in results; got: {:?}",
        names
    );
    assert!(
        !names.contains(&"b"),
        "did NOT expect `b` (unchanged) in results; got: {:?}",
        names
    );
}

#[test]
fn regression_changed_filters_callers_to_modified_callers() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    init_repo(root);
    write_file(
        &root.join("src/lib.rs"),
        r#"
pub fn target() { println!("hello"); }
pub fn caller_a() { target(); }
pub fn caller_b() { target(); }
"#,
    );
    commit_all(root, "initial");

    write_file(
        &root.join("src/lib.rs"),
        r#"
pub fn target() { println!("hello"); }
pub fn caller_a() {
    let _x = 1;
    target();
}
pub fn caller_b() { target(); }
"#,
    );
    commit_all(root, "touch caller_a");

    let out = run_rustgraph_in(
        root,
        &[
            "--no-auto-path",
            "-p",
            ".",
            "callers",
            "target",
            "--changed",
            "--since",
            "HEAD~1",
            "-j",
        ],
    );
    assert!(
        out.status.success(),
        "callers --changed should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("expected valid json; got: {} (err: {})", stdout, e);
    });
    let matches = v["matches"].as_array().expect("matches array");
    assert_eq!(matches.len(), 1, "expected 1 target match; got: {}", stdout);
    let empty = Vec::new();
    let caller_names: Vec<&str> = matches[0]["callers"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .map(|c| c["info"]["name"].as_str().unwrap_or(""))
        .collect();
    assert!(
        caller_names.contains(&"caller_a"),
        "expected caller_a in callers list; got: {:?}",
        caller_names
    );
    assert!(
        !caller_names.contains(&"caller_b"),
        "did NOT expect caller_b in callers list; got: {:?}",
        caller_names
    );
}

#[test]
fn regression_changed_default_since_is_head_minus_one() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    init_repo(root);
    write_file(&root.join("src/lib.rs"), "pub fn first_commit_fn() {}\n");
    commit_all(root, "initial");
    write_file(
        &root.join("src/lib.rs"),
        "pub fn first_commit_fn() {}\npub fn second_commit_fn() {}\n",
    );
    commit_all(root, "add second_commit_fn");

    let out = run_rustgraph_in(
        root,
        &[
            "--no-auto-path",
            "-p",
            ".",
            "find",
            "first_commit_fn|second_commit_fn",
            "--changed",
            "-j",
        ],
    );
    assert!(
        out.status.success(),
        "find --changed (default since) should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("expected valid json; got: {} (err: {})", stdout, e);
    });
    let empty = Vec::new();
    let names: Vec<&str> = v["functions"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .map(|f| f["name"].as_str().unwrap_or(""))
        .collect();
    assert!(
        names.contains(&"second_commit_fn"),
        "expected second_commit_fn (default --since HEAD~1) in results; got: {:?}",
        names
    );
    assert!(
        !names.contains(&"first_commit_fn"),
        "did NOT expect first_commit_fn (unchanged since HEAD~1); got: {:?}",
        names
    );
}

#[test]
fn regression_changed_errors_in_non_git_dir() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    write_file(&root.join("src/lib.rs"), "pub fn alone() {}\n");

    let out = run_rustgraph_in(
        root,
        &[
            "--no-auto-path",
            "-p",
            ".",
            "find",
            "alone",
            "--changed",
            "--since",
            "main",
        ],
    );
    assert!(
        !out.status.success(),
        "find --changed in non-git dir must exit non-zero; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(
        stderr.contains("git"),
        "stderr should mention 'git'; got: {}",
        stderr
    );
}

#[test]
fn regression_changed_with_zero_changes_returns_empty() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    init_repo(root);
    write_file(&root.join("src/lib.rs"), "pub fn x() {}\n");
    commit_all(root, "initial");

    let out = run_rustgraph_in(
        root,
        &[
            "--no-auto-path",
            "-p",
            ".",
            "find",
            "x",
            "--changed",
            "--since",
            "HEAD",
        ],
    );
    assert!(
        out.status.success(),
        "find --changed --since HEAD should exit 0; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        stderr.contains("0 files") || stderr.contains("0 changed"),
        "expected stderr to note '0 files'/'0 changed'; got stderr={}",
        stderr
    );

    assert!(
        !stdout.contains("- fn x"),
        "expected empty find result with --changed --since HEAD; got stdout={}",
        stdout
    );
}

#[test]
fn regression_changed_filters_dead_code_to_modified_dead_fns() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    init_repo(root);
    write_file(
        &root.join("src/lib.rs"),
        r#"
pub fn used() {}
pub fn dead_pre_existing() {}
fn caller() { used(); }
"#,
    );
    commit_all(root, "initial");

    write_file(
        &root.join("src/lib.rs"),
        r#"
pub fn used() {}
pub fn dead_pre_existing() {}
pub fn dead_new() {}
fn caller() { used(); }
"#,
    );
    commit_all(root, "add dead_new");

    let out = run_rustgraph_in(
        root,
        &[
            "--no-auto-path",
            "-p",
            ".",
            "dead-code",
            "--changed",
            "--since",
            "HEAD~1",
            "-j",
        ],
    );
    assert!(
        out.status.success(),
        "dead-code --changed should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!("expected valid json; got: {} (err: {})", stdout, e);
    });
    let empty = Vec::new();
    let dead: Vec<&str> = v["dead_functions"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .map(|d| d["info"]["name"].as_str().unwrap_or(""))
        .collect();
    assert!(
        dead.contains(&"dead_new"),
        "expected dead_new (new dead fn) in dead-code results; got: {:?}",
        dead
    );
    assert!(
        !dead.contains(&"dead_pre_existing"),
        "did NOT expect dead_pre_existing (already dead, untouched) in --changed results; got: {:?}",
        dead
    );
}
