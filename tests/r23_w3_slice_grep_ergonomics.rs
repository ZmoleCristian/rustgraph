

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::{TempDir, tempdir};

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


fn slice_multi_fn_fixture() -> TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "// line 1: top of file\n\
         pub fn alpha() {\n\
         \tlet a = 1;\n\
         \tlet b = 2;\n\
         \tlet c = 3;\n\
         \tlet d = 4;\n\
         \tlet e = 5;\n\
         \tlet f = 6;\n\
         \tlet g = 7;\n\
         \tlet h = 8;\n\
         \tlet i = 9;\n\
         \tlet j = 10;\n\
         \tlet k = 11;\n\
         \tlet l = 12;\n\
         \tlet m = 13;\n\
         \tlet n = 14;\n\
         \tlet o = 15;\n\
         \tlet p = 16;\n\
         \tlet q = 17;\n\
         \tlet r = 18;\n\
         \tlet s = 19;\n\
         \tlet t = 20;\n\
         }\n\
         \n\
         pub fn beta() {\n\
         \tlet x = 99;\n\
         }\n",
    );
    dir
}


fn fifty_line_file_fixture() -> TempDir {
    let dir = tempdir().expect("tempdir");
    let mut content = String::new();
    for i in 1..=50 {
        content.push_str(&format!("// line {} of fifty\n", i));
    }
    write_file(&dir.path().join("src/lib.rs"), &content);
    dir
}


fn slice_line_col_fixture() -> TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/foo.rs"),
        "// header\n\
         pub fn target() {\n\
         \tlet payload = 42;\n\
         \tprintln!(\"{}\", payload);\n\
         }\n",
    );
    dir
}


fn grep_context_fixture() -> TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "// before-3\n\
         // before-2\n\
         // before-1\n\
         let needle = 1;\n\
         // after-1\n\
         // after-2\n\
         // after-3\n",
    );
    dir
}


fn grep_by_file_fixture() -> TempDir {
    let dir = tempdir().expect("tempdir");

    let mut foo = String::new();
    for _ in 0..12 {
        foo.push_str("// needle\n");
    }
    write_file(&dir.path().join("src/foo.rs"), &foo);

    let mut bar = String::new();
    for _ in 0..3 {
        bar.push_str("// needle\n");
    }
    write_file(&dir.path().join("src/bar.rs"), &bar);

    write_file(&dir.path().join("src/baz.rs"), "// needle\n");

    write_file(&dir.path().join("src/unrelated.rs"), "// nothing here\n");
    dir
}


#[test]
fn slice_around_centers_window_on_line_with_long_alias() {
    let fixture = slice_multi_fn_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let target = format!("{}/src/lib.rs:12", base);

    let out = run_rustgraph(&["-p", &base, "slice", &target, "--around", "3"]);
    assert!(
        out.status.success(),
        "slice --around expected ok; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);

    assert!(
        stdout.contains(":9-15"),
        "expected line range 9-15 in output; got:\n{}",
        stdout
    );

    assert!(
        !stdout.contains(":2-23"),
        "expected NOT to include the full alpha fn span 2-23; got:\n{}",
        stdout
    );
}


#[test]
fn slice_around_short_alias_dash_c_works() {
    let fixture = slice_multi_fn_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let target = format!("{}/src/lib.rs:12", base);

    let out_long = run_rustgraph(&["-p", &base, "slice", &target, "--around", "3"]);
    let out_short = run_rustgraph(&["-p", &base, "slice", &target, "-C", "3"]);
    assert!(out_long.status.success(), "long form failed: {}", stderr_of(&out_long));
    assert!(out_short.status.success(), "short form failed: {}", stderr_of(&out_short));


    assert_eq!(
        stdout_of(&out_long),
        stdout_of(&out_short),
        "long and short forms must produce identical output"
    );
}


#[test]
fn slice_around_with_explicit_range_expands_each_side() {
    let fixture = slice_multi_fn_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let target = format!("{}/src/lib.rs:10-12", base);

    let out = run_rustgraph(&["-p", &base, "slice", &target, "--around", "3"]);
    assert!(
        out.status.success(),
        "slice range --around expected ok; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);

    assert!(
        stdout.contains(":7-15"),
        "expected expanded range 7-15 in output; got:\n{}",
        stdout
    );
}


#[test]
fn slice_without_around_returns_full_enclosing_fn() {
    let fixture = slice_multi_fn_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let target = format!("{}/src/lib.rs:12", base);

    let out = run_rustgraph(&["-p", &base, "slice", &target]);
    assert!(
        out.status.success(),
        "default slice expected ok; stderr={}",
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);


    assert!(
        stdout.contains("let a = 1") && stdout.contains("let t = 20"),
        "expected full enclosing fn body; got:\n{}",
        stdout
    );

    assert!(
        !stdout.contains(":9-15"),
        "expected the full fn span, not a 7-line window; got:\n{}",
        stdout
    );
}


#[test]
fn slice_line_col_form_ignores_col_and_matches_line_only_form() {
    let fixture = slice_line_col_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let target_line_only = format!("{}/src/foo.rs:3", base);
    let target_with_col = format!("{}/src/foo.rs:3:8", base);

    let out_line = run_rustgraph(&["-p", &base, "slice", &target_line_only]);
    let out_col = run_rustgraph(&["-p", &base, "slice", &target_with_col]);

    assert!(
        out_line.status.success(),
        "line-only form failed: stderr={}",
        stderr_of(&out_line)
    );
    assert!(
        out_col.status.success(),
        "line:col form failed: stderr={}",
        stderr_of(&out_col)
    );


    let stdout_line = stdout_of(&out_line);
    let stdout_col = stdout_of(&out_col);
    assert_eq!(
        stdout_line, stdout_col,
        "expected `path:LINE` and `path:LINE:COL` to produce identical output"
    );
    assert!(
        stdout_line.contains("target"),
        "expected fn name 'target' in output; got:\n{}",
        stdout_line
    );
}


#[test]
fn slice_range_fully_past_eof_errors_with_actionable_stderr() {
    let fixture = fifty_line_file_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let target = format!("{}/src/lib.rs:999-1000", base);

    let out = run_rustgraph(&["-p", &base, "slice", &target]);
    assert!(
        !out.status.success(),
        "expected non-zero exit for past-EOF range; got success with stdout:\n{}",
        stdout_of(&out)
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("past EOF"),
        "expected stderr to mention 'past EOF'; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("999") && stderr.contains("1000"),
        "expected the requested range numbers in stderr; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("50"),
        "expected file length (50) in stderr; got:\n{}",
        stderr
    );
}


#[test]
fn slice_range_partial_overlap_clamps_with_stderr_note() {
    let fixture = fifty_line_file_fixture();
    let base = fixture.path().to_string_lossy().to_string();
    let target = format!("{}/src/lib.rs:45-100", base);

    let out = run_rustgraph(&["-p", &base, "slice", &target]);
    assert!(
        out.status.success(),
        "expected zero exit for partial-overlap clamp; stderr={}",
        stderr_of(&out)
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("note: clamped"),
        "expected stderr 'note: clamped' warning; got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("45-100") && stderr.contains("45-50"),
        "expected requested vs clamped range in stderr; got:\n{}",
        stderr
    );

    let stdout = stdout_of(&out);

    assert!(
        stdout.contains(":45-50"),
        "expected clamped range 45-50 in output header; got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("// line 45 of fifty") && stdout.contains("// line 50 of fifty"),
        "expected slice content lines 45..50 in output; got:\n{}",
        stdout
    );
}


#[test]
fn grep_context_dash_c_shows_n_before_and_after() {
    let fixture = grep_context_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "-F", "needle", "-C", "2"]);
    assert!(
        out.status.success(),
        "grep -C expected ok; stderr={}",
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);

    assert!(stdout.contains("before-2"), "expected before-2 in output; got:\n{}", stdout);
    assert!(stdout.contains("after-2"), "expected after-2 in output; got:\n{}", stdout);
    assert!(
        !stdout.contains("before-3"),
        "expected before-3 NOT in output (outside -C 2 window); got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("after-3"),
        "expected after-3 NOT in output (outside -C 2 window); got:\n{}",
        stdout
    );

    assert!(
        stdout.contains("> 4: let needle = 1;") || stdout.contains(">      4 | let needle = 1;"),
        "expected match line marked with `>`; got:\n{}",
        stdout
    );
}


#[test]
fn grep_context_dash_b_dash_a_asymmetric_window() {
    let fixture = grep_context_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "-F", "needle", "-B", "1", "-A", "2"]);
    assert!(
        out.status.success(),
        "grep -B/-A expected ok; stderr={}",
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);

    assert!(stdout.contains("before-1"), "expected before-1; got:\n{}", stdout);
    assert!(
        !stdout.contains("before-2"),
        "expected before-2 NOT in output (outside -B 1); got:\n{}",
        stdout
    );

    assert!(stdout.contains("after-1"), "expected after-1; got:\n{}", stdout);
    assert!(stdout.contains("after-2"), "expected after-2; got:\n{}", stdout);
    assert!(
        !stdout.contains("after-3"),
        "expected after-3 NOT in output (outside -A 2); got:\n{}",
        stdout
    );
}


#[test]
fn grep_by_file_emits_file_rollup_not_per_line() {
    let fixture = grep_by_file_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "-F", "needle", "--by-file"]);
    assert!(
        out.status.success(),
        "grep --by-file expected ok; stderr={}",
        stderr_of(&out)
    );

    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("foo.rs: 12 matches"),
        "expected `foo.rs: 12 matches` line; got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("bar.rs: 3 matches"),
        "expected `bar.rs: 3 matches` line; got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("baz.rs: 1 match"),
        "expected `baz.rs: 1 match` line (singular); got:\n{}",
        stdout
    );

    assert!(
        stdout.contains("total: 16 matches across 3 files"),
        "expected `total: 16 matches across 3 files` line; got:\n{}",
        stdout
    );

    assert!(
        !stdout.contains("unrelated.rs:"),
        "expected unrelated.rs (zero matches) NOT in rollup; got:\n{}",
        stdout
    );


    let body_lines: Vec<&str> = stdout
        .lines()

        .filter(|l| !l.starts_with("rustgraph") && !l.starts_with("total:") && !l.contains(": 1 match") && !l.contains(": 3 matches") && !l.contains(": 12 matches"))
        .collect();

    let leaked_per_line = body_lines.iter().any(|l| l.contains("// needle"));
    assert!(
        !leaked_per_line,
        "expected per-line `// needle` content NOT in --by-file output; got:\n{}",
        stdout
    );
}


#[test]
fn grep_by_file_and_by_function_are_mutually_exclusive() {
    let fixture = grep_by_file_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "-F", "needle", "--by-file", "--by-function"]);
    assert!(
        !out.status.success(),
        "expected non-zero exit when --by-file + --by-function combined; stdout:\n{}",
        stdout_of(&out)
    );

    let stderr = stderr_of(&out);

    assert!(
        stderr.to_lowercase().contains("cannot be used")
            || stderr.to_lowercase().contains("conflicts")
            || stderr.contains("--by-function")
            || stderr.contains("--by-file"),
        "expected stderr mentioning the conflict; got:\n{}",
        stderr
    );
}


#[test]
fn grep_by_file_json_envelope_has_by_file_array() {
    let fixture = grep_by_file_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "-F", "needle", "--by-file", "--json"]);
    assert!(
        out.status.success(),
        "grep --by-file --json expected ok; stderr={}",
        stderr_of(&out)
    );

    let parsed: Value =
        serde_json::from_str(&stdout_of(&out)).expect("JSON output must be parseable");

    let by_file = parsed
        .get("by_file")
        .expect("expected `by_file` envelope key")
        .as_array()
        .expect("`by_file` must be an array");

    assert_eq!(
        by_file.len(),
        3,
        "expected 3 file entries in by_file array; got: {:?}",
        by_file
    );


    let first = by_file.first().expect("array non-empty");
    assert_eq!(
        first.get("count").and_then(|v| v.as_u64()),
        Some(12),
        "expected first entry count=12 (sorted desc); got: {:?}",
        first
    );
    assert!(
        first.get("path").and_then(|v| v.as_str()).unwrap_or("").contains("foo.rs"),
        "expected first entry path to contain foo.rs; got: {:?}",
        first
    );


    assert!(
        parsed.get("matches").is_none(),
        "expected `matches` key absent in --by-file JSON; got: {:?}",
        parsed
    );
}
