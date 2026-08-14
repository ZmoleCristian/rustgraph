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

fn run_substring_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"
pub fn run_cli() {}
pub fn run_trim() {}
pub fn run_pick() {}
pub fn unrelated_helper() {}
"#,
    );
    dir
}

fn no_foo_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn alpha_helper() {}\npub fn beta_loader() {}\npub fn gamma_runner() {}\n",
    );
    dir
}

#[test]
fn fix1_find_run_returns_substring_hits_via_fallback() {
    let fixture = run_substring_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "run"]);
    assert!(
        out.status.success(),
        "find run should succeed via fallback; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);
    for name in ["run_cli", "run_trim", "run_pick"] {
        assert!(
            stdout.contains(name),
            "expected substring hit `{name}` via fallback; got stdout:\n{stdout}"
        );
    }

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("3 fn"),
        "expected `3 fn` in stderr headline; got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_find_run_stderr_contains_fallback_note() {
    let fixture = run_substring_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "run"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("note:") && stderr.contains("falling back to substring"),
        "expected R26-W3 rescue note `note: ... falling back to substring/prefix`; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("'run'"),
        "rescue note should mention the offending query 'run'; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("threshold 0.85"),
        "rescue note should mention the rescue threshold 0.85; got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_find_foo_no_false_rescue_when_no_substring() {
    let fixture = no_foo_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "foo"]);
    assert!(
        !out.status.success(),
        "find foo should exit 1 when fallback also returns 0; status={:?}",
        out.status
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("no match"),
        "expected `no match` header; got stderr:\n{stderr}"
    );

    assert!(
        !stderr.contains("falling back to substring"),
        "rescue note must NOT fire when fallback returns 0 (genuine no-match); got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_find_foo_exact_preserves_strict_contract() {
    let fixture = no_foo_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "foo", "--exact"]);
    assert!(
        !out.status.success(),
        "find foo --exact should exit 1 with no rescue; status={:?}",
        out.status
    );

    let stderr = stderr_of(&out);

    assert!(
        stderr.contains("exact mode"),
        "expected R23-W2 `(exact mode)` no-match copy; got stderr:\n{stderr}"
    );

    assert!(
        !stderr.contains("falling back to substring"),
        "R26-W3 rescue must NOT fire in --exact mode (R23-W2 contract preserved); got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_find_exact_no_fallback_even_when_substring_exists() {
    let fixture = run_substring_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "run", "--exact"]);
    assert!(
        !out.status.success(),
        "find run --exact should exit 1 (no exact `run` even though `run_*` exist); status={:?}",
        out.status
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("exact mode"),
        "expected `(exact mode)` no-match copy; got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("falling back to substring"),
        "rescue note must NOT fire in --exact mode; got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_find_run_rescue_no_contradiction_with_suggestions() {
    let fixture = run_substring_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "run"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("did you mean"),
        "did-you-mean block must NOT fire when fallback rescues (no-match path skipped); got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_find_new_exact_match_still_suppresses_substring_noise() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn new() -> u32 { 0 }
pub fn newest_jsonl() {}
pub fn new_surgery_id() {}
pub fn preload_new() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "new"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stdout = stdout_of(&out);
    let stderr = stderr_of(&out);

    assert!(
        stderr.contains("1 fn"),
        "expected exactly 1 fn in stderr headline (R15 exact-name fast path); got stderr:\n{stderr}\nstdout:\n{stdout}"
    );
    for noise in ["newest_jsonl", "new_surgery_id", "preload_new"] {
        assert!(
            !stdout.contains(noise),
            "R15 fast path violated: substring `{noise}` leaked; got stdout:\n{stdout}"
        );
    }

    assert!(
        !stderr.contains("falling back to substring"),
        "rescue note must NOT fire when strict tier returned hits; got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_multi_term_or_unaffected_by_fallback() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn cut() {}\npub fn pick() {}\npub fn unrelated() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "cut|pick"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("cut"),
        "expected cut; got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("pick"),
        "expected pick; got stdout:\n{stdout}"
    );

    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("falling back to substring"),
        "rescue note must NOT fire on multi-term OR (effective == requested threshold); got stderr:\n{stderr}"
    );
}

#[test]
fn fix1_long_query_no_fallback_note() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn write_file() {}\npub fn writer_buffered() {}\npub fn unrelated() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "write"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("write_file"), "got stdout:\n{stdout}");

    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("falling back to substring"),
        "5-char query must not trigger short-query rescue note; got stderr:\n{stderr}"
    );
}

fn serialize_split_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/api.rs"),
        r#"
use serde::Serialize;
#[derive(Serialize)]
pub struct ApiTypeOne;
#[derive(Serialize)]
pub struct ApiTypeTwo;
#[derive(Serialize)]
pub struct ApiTypeThree;
"#,
    );
    write_file(
        &dir.path().join("src/index/mod.rs"),
        r#"
use serde::Serialize;
#[derive(Serialize)]
pub struct IndexTypeOne;
#[derive(Serialize)]
pub struct IndexTypeTwo;
"#,
    );
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub mod api;\npub mod index;\n",
    );
    dir
}

#[test]
fn fix2_impls_in_filter_header_matches_body() {
    let fixture = serialize_split_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "impls", "Serialize", "--in", "src/api"]);
    assert!(
        out.status.success(),
        "impls failed; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("3 derived, 0 hand-written, 3 total"),
        "expected header `3 derived, 0 hand-written, 3 total` post-filter; got:\n{stdout}"
    );

    let derive_lines = stdout.lines().filter(|l| l.contains("[derive]")).count();
    assert_eq!(
        derive_lines, 3,
        "body derive count must match header derived count; got stdout:\n{stdout}"
    );

    for name in ["IndexTypeOne", "IndexTypeTwo"] {
        assert!(
            !stdout.contains(name),
            "expected `{name}` filtered out by --in src/api; got:\n{stdout}"
        );
    }
}

#[test]
fn fix2_impls_in_nonexistent_zero_header() {
    let fixture = serialize_split_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "impls",
        "Serialize",
        "--in",
        "nonexistent_xyz",
    ]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("0 derived, 0 hand-written, 0 total"),
        "expected zero-counts header per wishlist; got stdout:\n{stdout}"
    );

    let body_lines = stdout
        .lines()
        .filter(|l| l.contains("[derive]") || l.contains("[handwritten]"))
        .count();
    assert_eq!(
        body_lines, 0,
        "no body lines expected; got stdout:\n{stdout}"
    );
}

#[test]
fn fix2_impls_unfiltered_header_matches_body() {
    let fixture = serialize_split_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "impls", "Serialize"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("5 derived, 0 hand-written, 5 total"),
        "expected `5 derived, 0 hand-written, 5 total`; got stdout:\n{stdout}"
    );
    let body_lines = stdout
        .lines()
        .filter(|l| l.contains("[derive]") || l.contains("[handwritten]"))
        .count();
    assert_eq!(
        body_lines, 5,
        "body line count must match header total; got stdout:\n{stdout}"
    );
}

#[test]
fn fix2_impls_mixed_kinds_breakdown_consistent() {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"
use serde::Serialize;
use serde::ser::Serializer;

#[derive(Serialize)]
pub struct DerivedOne;
#[derive(Serialize)]
pub struct DerivedTwo;

pub struct HandwrittenOne;
impl Serialize for HandwrittenOne {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { todo!() }
}
"#,
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "impls", "Serialize"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("2 derived, 1 hand-written, 3 total"),
        "expected `2 derived, 1 hand-written, 3 total`; got stdout:\n{stdout}"
    );
    let derive_lines = stdout.lines().filter(|l| l.contains("[derive]")).count();
    let handwritten_lines = stdout
        .lines()
        .filter(|l| l.contains("[handwritten]"))
        .count();
    assert_eq!(
        derive_lines, 2,
        "derive body lines must match header `2 derived`; got stdout:\n{stdout}"
    );
    assert_eq!(
        handwritten_lines, 1,
        "handwritten body lines must match header `1 hand-written`; got stdout:\n{stdout}"
    );
}

#[test]
fn fix2_impls_derived_only_keeps_full_breakdown_header() {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"
use serde::Serialize;
use serde::ser::Serializer;

#[derive(Serialize)]
pub struct DerivedOne;

pub struct HandwrittenOne;
impl Serialize for HandwrittenOne {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { todo!() }
}
"#,
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "impls", "Serialize", "--derived-only"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("1 derived, 1 hand-written, 2 total"),
        "expected full breakdown header even with --derived-only; got stdout:\n{stdout}"
    );

    let derive_lines = stdout.lines().filter(|l| l.contains("[derive]")).count();
    let handwritten_lines = stdout
        .lines()
        .filter(|l| l.contains("[handwritten]"))
        .count();
    assert_eq!(
        derive_lines, 1,
        "expected 1 derived body line; got stdout:\n{stdout}"
    );
    assert_eq!(
        handwritten_lines, 0,
        "--derived-only must filter out handwritten body; got stdout:\n{stdout}"
    );
}
