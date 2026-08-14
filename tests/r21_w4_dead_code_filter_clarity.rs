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
fn regression_in_zeroes_out_emits_loud_note() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn dead_one() {}\npub fn dead_two() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "dead-code", "--in", "nonexistent_subdir"]);
    assert!(
        out.status.success(),
        "dead-code --in failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        stderr.contains("--in 'nonexistent_subdir' filter:") && stderr.contains("2 → 0 dead fn(s)"),
        "expected existing concise prefix line; stderr={stderr}"
    );

    assert!(
        stderr.contains("NOTE:"),
        "expected loud NOTE: line on stderr; stderr={stderr}"
    );
    assert!(
        stderr.contains("--in 'nonexistent_subdir' filtered all 2 dead fn(s) out"),
        "NOTE: should mention the filter zeroed the set; stderr={stderr}"
    );
    assert!(
        stderr.contains("Hint:") && stderr.contains("re-run without --in"),
        "expected hint mentioning re-run without --in; stderr={stderr}"
    );
}

#[test]
fn regression_in_partial_reduction_no_loud_note() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/keep/a.rs"),
        "pub fn dead_keep_one() {}\npub fn dead_keep_two() {}\npub fn dead_keep_three() {}\n",
    );
    write_file(
        &fixture.path().join("src/skip/b.rs"),
        "pub fn dead_skip_one() {}\npub fn dead_skip_two() {}\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod keep { pub mod a; }\npub mod skip { pub mod b; }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "dead-code", "--in", "keep"]);
    assert!(
        out.status.success(),
        "dead-code --in failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        stderr.contains("--in 'keep' filter:") && stderr.contains("→ 3 dead fn(s)"),
        "expected concise prefix line showing 5 → 3 partial reduction; stderr={stderr}"
    );

    assert!(
        !stderr.contains("NOTE:"),
        "did NOT expect loud NOTE: line on partial reduction; stderr={stderr}"
    );
}

#[test]
fn regression_in_on_empty_dead_set_no_loud_note() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn helper() {}\nfn caller() { helper(); }\n#[no_mangle]\npub fn entry() { caller(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "dead-code", "--in", "src/lib.rs"]);
    assert!(
        out.status.success(),
        "dead-code --in failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        stderr.contains("--in 'src/lib.rs' filter:") && stderr.contains("0 → 0 dead fn(s)"),
        "expected concise prefix line for empty-set filter; stderr={stderr}"
    );

    assert!(
        !stderr.contains("NOTE:"),
        "did NOT expect loud NOTE: when dead set was already empty; stderr={stderr}"
    );
}

#[test]
fn regression_exclude_tests_zeroes_out_emits_loud_note() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "#[test]\npub fn dead_test_pub() { assert!(true); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let baseline = run_rustgraph(&["--path", &base, "dead-code"]);
    assert!(
        baseline.status.success(),
        "dead-code baseline failed; stderr={}",
        String::from_utf8_lossy(&baseline.stderr)
    );
    let baseline_stdout = String::from_utf8_lossy(&baseline.stdout);
    assert!(
        baseline_stdout.contains("dead_test_pub"),
        "fixture sanity: expected dead_test_pub flagged dead in baseline; stdout={baseline_stdout}"
    );

    let out = run_rustgraph(&["--path", &base, "dead-code", "--exclude-tests"]);
    assert!(
        out.status.success(),
        "dead-code --exclude-tests failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        stderr.contains("--exclude-tests filter:") && stderr.contains("→ 0 dead fn(s)"),
        "expected concise --exclude-tests prefix line; stderr={stderr}"
    );

    assert!(
        stderr.contains("NOTE:") && stderr.contains("--exclude-tests filtered all"),
        "expected loud NOTE: for --exclude-tests zeroing out; stderr={stderr}"
    );
    assert!(
        stderr.contains("Hint:") && stderr.contains("re-run without --exclude-tests"),
        "expected hint mentioning re-run without --exclude-tests; stderr={stderr}"
    );
}

#[test]
fn regression_changed_zeroes_out_emits_loud_note() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    init_repo(root);
    write_file(
        &root.join("src/lib.rs"),
        "pub fn used() {}\npub fn dead_pre_existing() {}\nfn caller_v1() { used(); }\n",
    );
    commit_all(root, "initial");

    write_file(
        &root.join("src/lib.rs"),
        "pub fn used() {}\npub fn dead_pre_existing() {}\nfn caller_v1() {\n    let _x = 1;\n    used();\n}\n",
    );
    commit_all(root, "touch caller_v1");

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
        ],
    );
    assert!(
        out.status.success(),
        "dead-code --changed failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        stderr.contains("--changed filter:") && stderr.contains("→ 0 dead fn(s)"),
        "expected concise --changed prefix line; stderr={stderr}"
    );

    assert!(
        stderr.contains("NOTE:") && stderr.contains("--changed filtered all"),
        "expected loud NOTE: for --changed zeroing out; stderr={stderr}"
    );
    assert!(
        stderr.contains("Hint:") && stderr.contains("re-run without --changed"),
        "expected hint mentioning re-run without --changed; stderr={stderr}"
    );
}
