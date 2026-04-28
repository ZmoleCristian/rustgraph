

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
fn fix1_find_exact_block_renders_above_fuzzy_block() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Session {
    pub id: u32,
}

pub fn session_load() -> Session { Session { id: 0 } }
pub fn session_save(_s: &Session) {}
pub fn session_close(_s: &Session) {}
pub fn session_resume() -> Session { Session { id: 0 } }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "Session"]);
    assert!(
        out.status.success(),
        "find failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("== Exact name matches =="),
        "expected exact banner; got:\n{stdout}"
    );
    assert!(
        stdout.contains("== Fuzzy matches =="),
        "expected fuzzy banner; got:\n{stdout}"
    );
    let exact_pos = stdout
        .find("struct Session")
        .expect("struct Session must appear in output");
    let first_session_fn = stdout
        .find("fn session_")
        .expect("at least one session_* fn must appear in output");
    assert!(
        exact_pos < first_session_fn,
        "struct Session (exact) must come BEFORE session_* fuzzy hits; got:\n{stdout}"
    );
    let exact_banner_pos = stdout.find("== Exact name matches ==").unwrap();
    assert!(
        exact_banner_pos < exact_pos,
        "exact banner must precede the exact hit; got:\n{stdout}"
    );
}


#[test]
fn fix1_find_omits_banners_when_only_one_partition_present() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),

        "pub fn widget_render() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "widget_render"]);
    assert!(
        out.status.success(),
        "find failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("== Exact name matches =="),
        "single-block output (no fuzzy) should suppress the exact banner; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("== Fuzzy matches =="),
        "no fuzzy hits → no fuzzy banner; got:\n{stdout}"
    );
    assert!(
        stdout.contains("widget_render"),
        "expected widget_render in output; got:\n{stdout}"
    );


    let fixture2 = tempdir().expect("tempdir");
    write_file(
        &fixture2.path().join("src/lib.rs"),
        "pub fn widget_renderer_extended() {}\n",
    );
    let base2 = fixture2.path().to_string_lossy().to_string();
    let out2 = run_rustgraph(&["--path", &base2, "find", "widget_renderer"]);
    assert!(
        out2.status.success(),
        "find2 failed: stderr={}",
        stderr_of(&out2)
    );
    let stdout2 = stdout_of(&out2);
    assert!(
        !stdout2.contains("== Exact name matches =="),
        "no exact hits → no exact banner; got:\n{stdout2}"
    );
    assert!(
        !stdout2.contains("== Fuzzy matches =="),
        "single-block (only fuzzy) should suppress fuzzy banner; got:\n{stdout2}"
    );
}


#[test]
fn fix1_find_json_partitions_into_exact_and_fuzzy_per_kind() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Session;
pub fn session_load() {}
pub fn session_save() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "Session", "-j"]);
    assert!(
        out.status.success(),
        "find -j failed: stderr={}",
        stderr_of(&out)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");

    let structs_exact = json["structs_exact"]
        .as_array()
        .expect("structs_exact array present");
    let structs_fuzzy = json["structs_fuzzy"]
        .as_array()
        .expect("structs_fuzzy array present");
    let fns_exact = json["functions_exact"]
        .as_array()
        .expect("functions_exact array present");
    let fns_fuzzy = json["functions_fuzzy"]
        .as_array()
        .expect("functions_fuzzy array present");

    assert_eq!(
        structs_exact.len(),
        1,
        "expected 1 exact struct match; got: {structs_exact:#?}"
    );
    assert_eq!(structs_exact[0]["name"].as_str(), Some("Session"));
    assert!(
        structs_fuzzy.is_empty(),
        "expected 0 fuzzy struct matches; got: {structs_fuzzy:#?}"
    );
    assert!(
        fns_exact.is_empty(),
        "expected 0 exact fn matches; got: {fns_exact:#?}"
    );
    assert_eq!(
        fns_fuzzy.len(),
        2,
        "expected 2 fuzzy fn matches (session_load + session_save); got: {fns_fuzzy:#?}"
    );


    let structs = json["structs"].as_array().expect("structs (legacy) array");
    assert_eq!(structs.len(), structs_exact.len() + structs_fuzzy.len());
}


#[test]
fn fix2_find_exact_with_pipe_or_returns_each_alt() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn cut() {}
pub fn delete() {}
pub fn unrelated() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "cut|delete", "--exact"]);
    assert!(
        out.status.success(),
        "find 'cut|delete' --exact must succeed (pipe-OR): stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("fn cut"),
        "expected `cut` in output (pipe-OR alt 1); got:\n{stdout}"
    );
    assert!(
        stdout.contains("fn delete"),
        "expected `delete` in output (pipe-OR alt 2); got:\n{stdout}"
    );
    assert!(
        !stdout.contains("unrelated"),
        "unrelated fn must not appear (exact mode is strict); got:\n{stdout}"
    );
}


#[test]
fn fix3_find_exact_header_omits_threshold_annotation() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() {}\npub fn beta() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "alpha", "--exact"]);
    assert!(
        out.status.success(),
        "find --exact failed: stderr={}",
        stderr_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("threshold"),
        "--exact header must not mention threshold; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("exact mode"),
        "expected `exact mode` annotation in --exact header; got stderr:\n{stderr}"
    );
}


#[test]
fn fix3_find_exact_no_match_error_omits_threshold() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn alpha() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "xyz_no_such_thing", "--exact"]);
    assert!(
        !out.status.success(),
        "no-match in --exact mode must still exit non-zero; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("(threshold "),
        "no-match in --exact must not print `(threshold X.XX)`; got:\n{stderr}"
    );
    assert!(
        stderr.contains("exact mode"),
        "no-match in --exact should reference `exact mode`; got:\n{stderr}"
    );
}


#[test]
fn fix4_def_same_kind_ambiguity_suggests_in_and_symbol_id() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/a/mod.rs"),
        "pub fn normalize_worktree_path() {}\n",
    );
    write_file(
        &fixture.path().join("src/b/mod.rs"),
        "pub fn normalize_worktree_path() {}\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod a;\npub mod b;\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "normalize_worktree_path"]);
    assert!(
        !out.status.success(),
        "def must fail with ambiguity (>1 match); status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("--in <PATH_SUBSTR>"),
        "same-kind ambiguity should suggest --in; got:\n{stderr}"
    );
    assert!(
        stderr.contains("--symbol-id"),
        "same-kind ambiguity should suggest --symbol-id; got:\n{stderr}"
    );

    assert!(
        !stderr.contains("--func/--struct/--enum"),
        "same-kind ambiguity should NOT suggest --func/--struct/--enum; got:\n{stderr}"
    );
    assert!(
        stderr.contains("(2 fn candidates)"),
        "expected count + kind label `(2 fn candidates)`; got:\n{stderr}"
    );
}


#[test]
fn fix4_def_mixed_kind_ambiguity_keeps_kind_filter_hint() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct Mixed;\npub fn Mixed() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "Mixed"]);
    assert!(
        !out.status.success(),
        "def Mixed must fail with mixed-kind ambiguity; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("--func/--struct/--enum"),
        "mixed-kind ambiguity must keep the --func/--struct/--enum hint; got:\n{stderr}"
    );
}


#[test]
fn fix5_paths_between_header_reflects_to_module_substring() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod foo;

pub fn handle_request() {
    foo::process();
}
"#,
    );
    write_file(
        &fixture.path().join("src/foo/mod.rs"),
        "pub fn process() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "paths-between",
        "handle_request",
        "--to-module",
        "src/foo",
    ]);
    assert!(
        out.status.success(),
        "paths-between failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("src/foo"),
        "header must mention the module substring `src/foo`; got:\n{stdout}"
    );

    assert!(
        !stdout.contains("→ ''"),
        "header must not contain the bug-shape `→ ''`; got:\n{stdout}"
    );


    assert!(
        stdout.contains("module:src/foo"),
        "header should label as `module:src/foo` to disambiguate from a single fn TO; got:\n{stdout}"
    );
}
