

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


fn paths_between_fixture_n_paths(n: usize) -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    let mut src = String::new();

    src.push_str("pub fn end() {}\n");

    let calls: String = (0..n).map(|i| format!("middle_{}(); ", i)).collect();
    src.push_str(&format!("pub fn start() {{ {}end(); }}\n", calls));
    for i in 0..n {
        src.push_str(&format!("pub fn middle_{}() {{ end(); }}\n", i));
    }
    write_file(&dir.path().join("src/lib.rs"), &src);
    dir
}


#[test]
fn fix1_header_shows_truncation_indicator_when_cap_hit() {

    let fixture = paths_between_fixture_n_paths(12);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "paths-between", "start", "end"]);
    assert!(
        out.status.success(),
        "paths-between should succeed; stderr:\n{}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);


    assert!(
        stdout.contains("search clamped"),
        "R26-W1+R27 Fix 1: header must contain 'search clamped' when cap fires; got:\n{stdout}"
    );
    assert!(
        stdout.contains("--max-results 0"),
        "R26-W1 Fix 1: header must mention '--max-results 0' when cap fires; got:\n{stdout}"
    );
}


#[test]
fn fix1_unlimited_max_results_shows_clean_count() {
    let fixture = paths_between_fixture_n_paths(12);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "paths-between", "start", "end", "--max-results", "0"]);
    assert!(
        out.status.success(),
        "paths-between --max-results 0 should succeed; stderr:\n{}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        !stdout.contains("search clamped"),
        "R26-W1+R27 Fix 1: unlimited mode must NOT contain 'search clamped' indicator; got:\n{stdout}"
    );

    assert!(
        stdout.contains("path(s)]"),
        "R26-W1 Fix 1: unlimited mode header should contain 'path(s)]'; got:\n{stdout}"
    );
}


#[test]
fn fix1_under_cap_shows_clean_count() {

    let fixture = paths_between_fixture_n_paths(2);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "paths-between", "start", "end"]);
    assert!(
        out.status.success(),
        "paths-between should succeed; stderr:\n{}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        !stdout.contains("search clamped"),
        "R26-W1+R27 Fix 1: under-cap run must NOT contain 'search clamped' indicator; got:\n{stdout}"
    );
    assert!(
        stdout.contains("path(s)]"),
        "R26-W1 Fix 1: under-cap header should contain 'path(s)]'; got:\n{stdout}"
    );
}


fn ambiguous_fn_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");

    write_file(
        &dir.path().join("src/alpha.rs"),
        "pub fn common() {}\n",
    );
    write_file(
        &dir.path().join("src/beta.rs"),
        "pub fn common() {}\n",
    );
    dir
}


#[test]
fn fix2_ambiguity_candidate_lines_are_copy_pasteable() {
    let fixture = ambiguous_fn_fixture();
    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&["-p", &base, "callers", "common", "--ambiguity-cap", "1"]);
    assert!(
        !out.status.success(),
        "callers should error when ambiguous; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);

    assert!(
        stderr.contains("symbol ids as the target"),
        "R26-W1 Fix 2: ambiguity error must tell the caller to re-run with a symbol id as target; got stderr:\n{stderr}"
    );

    let candidate_ids: Vec<String> = stderr
        .lines()
        .skip_while(|line| !line.contains("ambiguous:"))
        .skip(1)
        .map(|line| line.trim().to_string())
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('(')
                && !line.starts_with('|')
                && line.chars().filter(|&c| c == ':').count() >= 2
        })
        .collect();
    assert!(
        !candidate_ids.is_empty(),
        "R26-W1 Fix 2: expected bare path:line:name candidate lines; got:\n{stderr}"
    );

    for id in &candidate_ids {
        let id = id.split_whitespace().next().unwrap_or(id);
        let rerun = run_rustgraph(&["-p", &base, "callers", id, "--ambiguity-cap", "1"]);
        let rerun_stderr = stderr_of(&rerun);
        assert!(
            rerun.status.success(),
            "R26-W1 Fix 2: suggested symbol id '{}' must round-trip as the positional target; got stderr:\n{rerun_stderr}",
            id
        );
        assert!(
            !rerun_stderr.contains("ambiguous:"),
            "R26-W1 Fix 2: symbol id '{}' must resolve unambiguously; got stderr:\n{rerun_stderr}",
            id
        );
    }
}


fn grep_fixture_with_test_and_non_test() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"pub fn real_fn() {
    let needle = "target";
    let _ = needle;
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_fn() {
        let needle = "target";
        let _ = needle;
    }
}
"#,
    );
    dir
}


#[test]
fn fix3_by_file_exclude_tests_no_function_summary_header() {
    let fixture = grep_fixture_with_test_and_non_test();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "grep", "target", "--by-file", "--exclude-tests",
    ]);
    assert!(
        out.status.success(),
        "grep --by-file --exclude-tests should succeed; stderr:\n{}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        !stdout.contains("summary by enclosing function"),
        "R26-W1 Fix 3: --by-file mode must NOT emit 'summary by enclosing function' header; got:\n{stdout}"
    );
}


#[test]
fn fix3_by_function_still_emits_function_summary() {
    let fixture = grep_fixture_with_test_and_non_test();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "grep", "target", "--by-function",
    ]);
    assert!(
        out.status.success(),
        "grep --by-function should succeed; stderr:\n{}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("summary by enclosing function"),
        "Fix 3 sanity: --by-function must still emit the function summary header; got:\n{stdout}"
    );
}


fn target_in_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/keep/a.rs"),
        "pub fn common() {}\npub fn caller_keep() { common(); }\n",
    );
    write_file(
        &dir.path().join("src/drop/b.rs"),
        "pub fn common() {}\npub fn caller_drop() { common(); }\n",
    );
    dir
}


#[test]
fn fix4_target_in_before_cap_check_succeeds_when_narrowed() {
    let fixture = target_in_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "callers", "common", "--target-in", "keep", "--ambiguity-cap", "1",
    ]);
    assert!(
        out.status.success(),
        "R26-W1 Fix 4: callers with --target-in narrowing to 1 should succeed even with cap=1; stderr:\n{}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("keep"),
        "R26-W1 Fix 4: output should reference the keep/ file; got:\n{stdout}"
    );

    assert!(
        !stdout.contains("drop"),
        "R26-W1 Fix 4: output should not reference drop/ (filtered by --target-in); got:\n{stdout}"
    );
}


#[test]
fn fix4_target_in_that_leaves_too_many_still_errors() {
    let fixture = target_in_fixture();
    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&[
        "-p", &base, "callers", "common", "--target-in", "src", "--ambiguity-cap", "1",
    ]);
    assert!(
        !out.status.success(),
        "R26-W1 Fix 4 complementary: cap should still fire when --target-in doesn't narrow enough; stdout:\n{}",
        stdout_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("ambiguous") || stderr.contains("cap"),
        "R26-W1 Fix 4: error should mention ambiguity/cap; got:\n{stderr}"
    );
}
