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

fn tests_only_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"pub fn ok() {}

#[cfg(test)]
mod tests {
    #[test]
    fn t1() {
        let _ = Some(1).unwrap();
        let _ = Some(2).unwrap();
    }

    #[test]
    fn t2() {
        let _ = Some(3).unwrap();
        let _ = Some(4).unwrap();
    }
}
"#,
    );
    dir
}

fn mixed_prod_and_test_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"pub fn prod_a() {
    let _ = Some(1).unwrap();
}

pub fn prod_b() {
    let _ = Some(2).unwrap();
}

#[cfg(test)]
mod tests {
    #[test]
    fn t1() {
        let _ = Some(3).unwrap();
        let _ = Some(4).unwrap();
    }
}
"#,
    );
    dir
}

fn no_matches_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(&dir.path().join("src/lib.rs"), "pub fn ok() {}\n");
    dir
}

fn jsonl_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn JSONL_writer() {}\npub fn jsonl_reader() {}\n",
    );
    dir
}

#[test]
fn exclude_tests_note_when_filter_drops_all_matches() {
    let fixture = tests_only_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "--exclude-tests",
        "grep",
        "unwrap",
        "--by-function",
    ]);
    assert!(
        out.status.success(),
        "grep should exit 0; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("0 match") || stdout.contains("0 functions"),
        "expected 0-match headline in stdout; got:\n{}",
        stdout
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("Note:")
            && stderr.contains("matches found")
            && stderr.contains("--exclude-tests"),
        "expected --exclude-tests note on stderr; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("4 matches"),
        "expected raw count `4 matches` in note; got:\n{}",
        stderr
    );

    assert!(
        stderr.to_lowercase().contains("hint"),
        "expected `Hint:` follow-up in note; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("src/lib.rs:"),
        "expected at least one src/lib.rs:LINE example in note; got:\n{}",
        stderr
    );
}

#[test]
fn exclude_tests_no_note_when_filter_drops_partial() {
    let fixture = mixed_prod_and_test_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "--exclude-tests",
        "grep",
        "unwrap",
        "--by-function",
    ]);
    assert!(
        out.status.success(),
        "grep should exit 0; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("prod_a") || stdout.contains("prod_b"),
        "expected prod fns kept; got:\n{}",
        stdout
    );

    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("matches found but all dropped"),
        "expected NO 'all dropped' note when partial filter; got:\n{}",
        stderr
    );
}

#[test]
fn exclude_tests_no_note_when_genuinely_zero_matches() {
    let fixture = no_matches_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "--exclude-tests",
        "grep",
        "unwrap",
        "--by-function",
    ]);
    assert!(
        out.status.success(),
        "grep should exit 0; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("0 match"),
        "expected genuinely-zero result; got:\n{}",
        stdout
    );

    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("matches found but all dropped"),
        "expected NO 'all dropped' note when raw==0; got:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("--exclude-tests"),
        "expected NO --exclude-tests reference at all when raw==0; got:\n{}",
        stderr
    );
}

#[test]
fn pipe_hint_fires_up_front_for_jsonl_pattern() {
    let fixture = jsonl_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"JSONL\|jsonl"]);
    assert!(
        out.status.success(),
        "grep should exit 0; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );

    let stderr = stderr_of(&out);

    assert!(
        stderr.contains("warning:") && stderr.contains("Rust regex"),
        "expected up-front `warning:` hint mentioning `Rust regex`; got stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("did you mean") && stderr.contains("JSONL|jsonl"),
        "expected `did you mean JSONL|jsonl` suggestion; got stderr:\n{}",
        stderr
    );

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("0 match"),
        "expected 0 matches (literal pipe doesn't appear in fixture); got:\n{}",
        stdout
    );
}

#[test]
fn pipe_hint_fires_only_once() {
    let fixture = jsonl_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"JSONL\|jsonl"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stderr = stderr_of(&out);
    let occurrences = stderr.matches("did you mean").count();
    assert_eq!(
        occurrences, 1,
        "expected `did you mean` exactly once (fix 2 removes the post-result hint); got {} occurrences in stderr:\n{}",
        occurrences, stderr
    );

    let tip_occurrences = stderr.matches("tip:").count();
    assert_eq!(
        tip_occurrences, 0,
        "expected NO `tip:` prefix (old hint replaced); got {} in stderr:\n{}",
        tip_occurrences, stderr
    );
}

#[test]
fn pipe_hint_suppressed_with_fixed_string() {
    let fixture = jsonl_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "-F", r"JSONL\|jsonl"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("did you mean") && !stderr.contains("Rust regex"),
        "expected NO `\\|` hint when -F is set; got stderr:\n{}",
        stderr
    );
}

#[test]
fn pipe_hint_fires_even_with_matches_present() {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "// hits Foo|Bar marker\npub fn ok() {}\n",
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"Foo\|Bar"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("Foo|Bar"),
        "expected literal `Foo|Bar` to be matched; got stdout:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("0 match"),
        "expected non-zero matches; got stdout:\n{}",
        stdout
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("did you mean") && stderr.contains("Foo|Bar"),
        "expected hint to still fire even with matches; got stderr:\n{}",
        stderr
    );
}
