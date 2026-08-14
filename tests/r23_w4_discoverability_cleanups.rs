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

#[test]
fn fix1_tree_bare_word_with_src_dir_suggests_direct_form() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub mod alpha;\n");

    write_file(&fixture.path().join("src/alpha/mod.rs"), "pub fn x() {}\n");
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "tree", "alpha"]);
    assert!(
        out.status.success(),
        "tree should exit 0 on 0-match prefix; stderr={}",
        stderr_of(&out)
    );
    let stderr = stderr_of(&out);
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("0 file(s)"),
        "expected '0 file(s)' header on miss; got:\n{stdout}"
    );

    assert!(
        stderr.contains("Did you mean: tree src/alpha"),
        "expected `Did you mean: tree src/alpha` suggestion; got:\n{stderr}"
    );

    assert!(
        stderr.contains("note:") && stderr.contains("0 files match 'alpha'"),
        "expected `note: 0 files match 'alpha'` framing; got:\n{stderr}"
    );
}

#[test]
fn fix1_tree_existing_path_no_suggestion() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub mod beta;\n");
    write_file(&fixture.path().join("src/beta/mod.rs"), "pub fn y() {}\n");
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "tree", "src/beta"]);
    assert!(
        out.status.success(),
        "tree src/beta should succeed; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    let stderr = stderr_of(&out);

    assert!(
        stdout.contains("mod.rs"),
        "expected normal tree output for valid prefix; got:\n{stdout}"
    );

    assert!(
        !stderr.contains("Did you mean"),
        "did NOT expect `Did you mean` suggestion on a valid prefix; got:\n{stderr}"
    );
}

fn write_large_fixture(dir: &Path) {
    let mut a = String::new();
    for i in 0..40 {
        a.push_str(&format!("pub fn fn_a_{}() {{}}\n", i));
    }
    let mut b = String::new();
    for i in 0..40 {
        b.push_str(&format!("pub fn fn_b_{}() {{}}\n", i));
    }
    write_file(&dir.join("src/lib.rs"), "pub mod a;\npub mod b;\n");
    write_file(&dir.join("src/a.rs"), &a);
    write_file(&dir.join("src/b.rs"), &b);
}

#[test]
fn fix2_call_graph_unscoped_large_project_errors_without_all() {
    let fixture = tempdir().expect("tempdir");
    write_large_fixture(fixture.path());
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph"]);
    assert!(
        !out.status.success(),
        "expected call-graph (no scope) on large project to FAIL; stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    let stderr = stderr_of(&out);

    for needed in ["--root", "--search", "--all"] {
        assert!(
            stderr.contains(needed),
            "expected error to mention `{needed}`; got:\n{stderr}"
        );
    }
}

#[test]
fn fix2_call_graph_unscoped_large_project_works_with_all() {
    let fixture = tempdir().expect("tempdir");
    write_large_fixture(fixture.path());
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph", "--all"]);
    assert!(
        out.status.success(),
        "expected call-graph --all to succeed on large project; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("call-graph --text"),
        "expected text-mode header in --all output; got first line:\n{}",
        stdout.lines().next().unwrap_or("")
    );
}

#[test]
fn fix2_call_graph_unscoped_small_project_renders_without_all() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn a() {}\npub fn b() { a(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph"]);
    assert!(
        out.status.success(),
        "expected call-graph (no scope) on small project to succeed; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("call-graph --text"),
        "expected text-mode header; got first line:\n{}",
        stdout.lines().next().unwrap_or("")
    );
}

#[test]
fn fix3_call_graph_include_callers_renders_caller_tree() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target_for_test() {}\n\
         pub fn caller_one() { target_for_test(); }\n\
         pub fn caller_two() { target_for_test(); }\n\
         pub fn caller_three() { caller_one(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "call-graph",
        "--root",
        "target_for_test",
        "--include-callers",
        "--depth",
        "2",
    ]);
    assert!(
        out.status.success(),
        "expected call-graph --include-callers to succeed; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("caller_one"),
        "expected `caller_one` in --include-callers output; got:\n{stdout}"
    );
    assert!(
        stdout.contains("caller_two"),
        "expected `caller_two` in --include-callers output; got:\n{stdout}"
    );

    assert!(
        stdout.contains("caller_three"),
        "expected transitive `caller_three` at depth 2; got:\n{stdout}"
    );

    assert!(
        stdout.contains("Callers of target_for_test"),
        "expected `Callers of target_for_test` header in upstream block; got:\n{stdout}"
    );
}

const FAN_FIXTURE: &str = r#"
pub fn down_a() {}
pub fn down_b() {}
pub fn down_c() {}

pub fn target_fan(x: u32) -> u32 {
    down_a();
    down_b();
    down_c();
    x
}

pub fn caller_one() { let _ = target_fan(1); }
pub fn caller_two() { let _ = target_fan(2); }
pub fn caller_three() { let _ = target_fan(3); }
pub fn caller_four() { let _ = target_fan(4); }
pub fn caller_five() { let _ = target_fan(5); }
"#;

#[test]
fn fix4_ensemble_default_summary_uses_compact_neighborhood() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), FAN_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "ensemble", "target_fan"]);
    assert!(
        out.status.success(),
        "ensemble failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("Upstream:") && stdout.contains("caller(s)"),
        "expected compact `Upstream: N caller(s)` line in default Summary; got:\n{stdout}"
    );
    assert!(
        stdout.contains("Downstream:") && stdout.contains("callee(s)"),
        "expected compact `Downstream: N callee(s)` line in default Summary; got:\n{stdout}"
    );

    assert!(
        stdout.contains("+2 more"),
        "expected `+2 more` truncation marker for 5-caller fan; got:\n{stdout}"
    );

    assert!(
        !stdout.contains("Upstream callers:"),
        "did NOT expect verbose `Upstream callers:` block in default Summary; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("Downstream callees:"),
        "did NOT expect verbose `Downstream callees:` block in default Summary; got:\n{stdout}"
    );

    assert!(
        stdout.contains("caller_one"),
        "expected at least one caller name in compact roll-up; got:\n{stdout}"
    );
    assert!(
        stdout.contains("down_a"),
        "expected at least one callee name in compact roll-up; got:\n{stdout}"
    );
}

#[test]
fn fix4_ensemble_view_full_keeps_verbose_neighborhood() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), FAN_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "ensemble", "target_fan", "--view", "full"]);
    assert!(
        out.status.success(),
        "ensemble --view full failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("Upstream callers:"),
        "expected verbose `Upstream callers:` block under --view full; got:\n{stdout}"
    );
    assert!(
        stdout.contains("Downstream callees:"),
        "expected verbose `Downstream callees:` block under --view full; got:\n{stdout}"
    );

    assert!(
        !stdout.contains("Upstream:") || !stdout.contains("caller(s) ("),
        "did NOT expect compact `Upstream: N caller(s) (...)` line under --view full; got:\n{stdout}"
    );
}

#[test]
fn fix4_ensemble_section_neighborhood_keeps_verbose_form() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), FAN_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "ensemble",
        "target_fan",
        "--section",
        "neighborhood",
    ]);
    assert!(
        out.status.success(),
        "ensemble --section neighborhood failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("Upstream callers:"),
        "expected verbose `Upstream callers:` block under --section neighborhood; got:\n{stdout}"
    );
}

const ASSOC_FIXTURE: &str = "\
pub struct SessionDocument;
impl SessionDocument {
    pub fn load(_path: &str) -> Self { SessionDocument }
}
fn caller() {
    let _ = SessionDocument::load(\"x\");
}
";

#[test]
fn fix5_refs_leaf_with_assoc_call_kind_returns_zero() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), ASSOC_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "refs", "load", "--kind", "assoc_call"]);
    assert!(
        out.status.success(),
        "refs (with empty result) should still succeed; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("0 reference(s)"),
        "expected `0 reference(s)` for `refs load --kind assoc_call` (kind matches qualifier, not leaf); got:\n{stdout}"
    );
}

#[test]
fn fix5_refs_qualifier_with_assoc_call_kind_returns_match() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), ASSOC_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "refs",
        "SessionDocument",
        "--kind",
        "assoc_call",
    ]);
    assert!(
        out.status.success(),
        "refs SessionDocument --kind assoc_call failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("[assoc_call]"),
        "expected `[assoc_call]` tag in refs output; got:\n{stdout}"
    );
}

#[test]
fn fix5_refs_help_documents_assoc_call_qualifier_gotcha() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = Command::new(bin)
        .args(["refs", "--help"])
        .output()
        .expect("run rustgraph refs --help");
    assert!(
        out.status.success(),
        "rustgraph refs --help should succeed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    assert!(
        stdout.contains("QUALIFIER"),
        "expected `--kind` help to spell out QUALIFIER vs leaf gotcha; got:\n{stdout}"
    );
}
