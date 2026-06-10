

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
