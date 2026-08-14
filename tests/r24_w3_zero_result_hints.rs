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

const ASSOC_FIXTURE: &str = "\
pub struct SessionDocument;
impl SessionDocument {
    pub fn load(_p: &str) -> Self { SessionDocument }
}
fn caller() {
    let _ = SessionDocument::load(\"x\");
}
";

#[test]
fn fix1_refs_leaf_assoc_call_suggests_qualifier() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), ASSOC_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "refs", "load", "--kind", "assoc_call"]);
    assert!(
        out.status.success(),
        "refs (with empty result) should still succeed; stderr={}",
        stderr_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("note: 0 refs for 'load' with --kind assoc_call"),
        "expected `note: 0 refs for 'load' with --kind assoc_call` in stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs SessionDocument --kind assoc_call"),
        "expected `refs SessionDocument --kind assoc_call` suggestion in stderr; got:\n{stderr}"
    );
}

const ASSOC_MULTI_FIXTURE: &str = "\
pub struct SessionDocument;
impl SessionDocument { pub fn load(_p: &str) -> Self { SessionDocument } }
pub struct RolloutDocument;
impl RolloutDocument { pub fn load(_p: &str) -> Self { RolloutDocument } }
pub struct Project;
impl Project { pub fn load(_p: &str) -> Self { Project } }
pub struct Extra;
impl Extra { pub fn load(_p: &str) -> Self { Extra } }
fn caller() {
    let _ = SessionDocument::load(\"a\");
    let _ = RolloutDocument::load(\"b\");
    let _ = Project::load(\"c\");
    let _ = Extra::load(\"d\");
}
";

#[test]
fn fix1_refs_leaf_assoc_call_lists_up_to_three_qualifiers() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), ASSOC_MULTI_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "refs", "load", "--kind", "assoc_call"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);

    assert!(
        stderr.contains("note: 0 refs for 'load' with --kind assoc_call. Try one of:"),
        "expected multi-line `Try one of:` header; got:\n{stderr}"
    );

    let suggestion_lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.trim_start().starts_with("refs ") && l.contains("--kind assoc_call"))
        .collect();
    assert!(
        (1..=3).contains(&suggestion_lines.len()),
        "expected 1..=3 suggestion lines; got {}: {:?}",
        suggestion_lines.len(),
        suggestion_lines
    );
}

const VARIANT_FIXTURE: &str = "\
pub enum Color { Red, Green, Load }
fn caller() { let _ = Color::Load; }
";

#[test]
fn fix1_refs_leaf_variant_suggests_enum() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), VARIANT_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "refs", "Load", "--kind", "variant"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("note: 0 refs for 'Load' with --kind variant"),
        "expected `note: 0 refs for 'Load' with --kind variant` in stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs Color --kind variant"),
        "expected `refs Color --kind variant` suggestion in stderr; got:\n{stderr}"
    );
}

#[test]
fn fix1_refs_no_matching_type_omits_hint() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn xyz() {}\npub struct Other;\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "refs", "load", "--kind", "assoc_call"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("Did you mean: refs"),
        "expected NO `Did you mean: refs` hint when nothing matches; got:\n{stderr}"
    );
    assert!(
        !stderr.contains("Try one of:"),
        "expected NO `Try one of:` block when nothing matches; got:\n{stderr}"
    );

    assert!(
        stderr.contains("no references found for ident 'load'"),
        "expected existing `no references found` line; got:\n{stderr}"
    );
}

#[test]
fn fix1_refs_no_kind_filter_omits_hint() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), ASSOC_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "refs", "nonexistent_xyz123"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("Did you mean: refs"),
        "expected NO `Did you mean` hint without --kind; got:\n{stderr}"
    );
    assert!(
        !stderr.contains("Try one of:"),
        "expected NO `Try one of:` block without --kind; got:\n{stderr}"
    );
}

fn pipe_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn alpha() {}\npub fn beta() {}\n",
    );
    dir
}

#[test]
fn fix2_grep_backslash_pipe_emits_loud_warning() {
    let fixture = pipe_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"a\|b"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);

    assert!(
        stderr.contains("\u{2501}\u{2501}\u{2501}\u{2501}"),
        "expected separator banner (━ row) bracketing the warning; got stderr:\n{stderr}"
    );

    assert!(
        stderr.contains("warning:") && stderr.contains("Rust regex"),
        "expected `warning:` + `Rust regex` tokens; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("did you mean") && stderr.contains("a|b"),
        "expected `did you mean a|b` (no escape); got stderr:\n{stderr}"
    );
}

#[test]
fn fix2_grep_fixed_string_suppresses_pipe_warning() {
    let fixture = pipe_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "-F", r"a\|b"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("warning:"),
        "expected NO `warning:` under -F; got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("did you mean"),
        "expected NO `did you mean` hint under -F; got stderr:\n{stderr}"
    );
}

#[test]
fn fix2_grep_backslash_paren_emits_warning() {
    let fixture = pipe_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"a\(b\)c"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("0 match"),
        "R25-W2 gate fires on 0-match; pin the fixture precondition; got stdout:\n{stdout}"
    );
    let stderr = stderr_of(&out);

    assert!(
        stderr.contains("\u{2501}\u{2501}\u{2501}\u{2501}"),
        "expected separator banner around \\( warning; got stderr:\n{stderr}"
    );

    assert!(
        stderr.contains("note:") && stderr.contains("Rust regex uses bare"),
        "expected `note:` + `Rust regex uses bare (...)` text; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("a(b)c"),
        "expected suggested `a(b)c` (no backslash) in stderr; got:\n{stderr}"
    );
}

#[test]
fn fix2_grep_correct_pipe_pattern_no_warning() {
    let fixture = pipe_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", "a|b"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("warning:") && !stderr.contains("did you mean"),
        "expected NO warning for correct pattern; got stderr:\n{stderr}"
    );

    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("alpha") || stdout.contains("beta"),
        "expected real matches for correct pattern; got stdout:\n{stdout}"
    );
}

#[test]
fn fix2_grep_backslash_pipe_with_json_still_warns() {
    let fixture = pipe_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"a\|b", "--json"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    let stdout = stdout_of(&out);

    assert!(
        stderr.contains("warning:") && stderr.contains("did you mean"),
        "expected warning to fire under --json; got stderr:\n{stderr}"
    );

    assert!(
        stdout.trim_start().starts_with('{'),
        "expected stdout to be a valid JSON object; got stdout:\n{stdout}"
    );
    let _: serde_json::Value =
        serde_json::from_str(&stdout).expect("--json output should parse as JSON");
}

#[test]
fn fix2_grep_backslash_pipe_with_by_function_still_warns() {
    let fixture = pipe_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "grep", r"a\|b", "--by-function"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("warning:") && stderr.contains("did you mean"),
        "expected warning to fire under --by-function; got stderr:\n{stderr}"
    );
}
