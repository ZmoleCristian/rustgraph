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
fn fix1_def_in_filters_same_kind_ambiguity_to_unique_match() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/foo/mod.rs"),
        "pub fn shared_helper() {}\n",
    );
    write_file(
        &fixture.path().join("src/bar/mod.rs"),
        "pub fn shared_helper() {}\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod foo;\npub mod bar;\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "shared_helper", "--in", "src/foo"]);
    assert!(
        out.status.success(),
        "def --in should resolve uniquely: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("src/foo"),
        "expected src/foo path in output; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("src/bar"),
        "src/bar must be filtered out; got:\n{stdout}"
    );
}

#[test]
fn fix1_def_in_filter_excludes_all_candidates_dedicated_message() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn unique_name() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "unique_name", "--in", "nonexistent"]);
    assert!(
        !out.status.success(),
        "def --in with no surviving candidate must fail; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("nonexistent"),
        "error must mention the --in substr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("unique_name"),
        "error must echo the symbol name; got:\n{stderr}"
    );
    assert!(
        stderr.contains("before --in filter"),
        "error should note the pre-filter candidate count; got:\n{stderr}"
    );
}

#[test]
fn fix1_def_ambiguity_hint_mentions_in_flag_and_flag_actually_exists() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/a/mod.rs"), "pub fn dup_fn() {}\n");
    write_file(&fixture.path().join("src/b/mod.rs"), "pub fn dup_fn() {}\n");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod a;\npub mod b;\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "def", "dup_fn"]);
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("--in <PATH_SUBSTR>"),
        "ambiguity hint must mention `--in <PATH_SUBSTR>`; got:\n{stderr}"
    );

    let out2 = run_rustgraph(&["--path", &base, "def", "dup_fn", "--in", "src/a"]);
    let stderr2 = stderr_of(&out2);
    assert!(
        !stderr2.contains("unexpected argument"),
        "`def --in` must be a valid flag (anti-lie pin); got stderr:\n{stderr2}"
    );
    assert!(
        out2.status.success(),
        "`def dup_fn --in src/a` should resolve uniquely; got stderr:\n{stderr2}"
    );
}

#[test]
fn fix2_def_on_struct_field_suggests_refs_with_count_and_classification() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Session {
    pub session_id: u32,
    pub label: String,
}

pub fn make() -> Session {
    Session { session_id: 1, label: "x".into() }
}

pub fn read(s: &Session) -> u32 {
    s.session_id
}

pub fn read_again(s: &Session) -> u32 {
    s.session_id + 1
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "session_id"]);
    assert!(
        !out.status.success(),
        "def on a non-symbol must still exit non-zero; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("no top-level symbol named 'session_id'"),
        "error should clarify there is no top-level symbol; got:\n{stderr}"
    );
    assert!(
        stderr.contains("struct field"),
        "hint should classify as struct field; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs session_id"),
        "hint should suggest `refs session_id`; got:\n{stderr}"
    );

    assert!(
        stderr.contains("ref(s) found"),
        "hint should report a ref count; got:\n{stderr}"
    );
}

#[test]
fn fix2_def_on_enum_variant_suggests_refs_classified_as_variant() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub enum State {
    Pending,
    Active,
}

pub fn pick() -> State { State::Active }
pub fn pick2() -> State { State::Active }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "Active"]);
    assert!(
        !out.status.success(),
        "def on an enum variant should still fail; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("enum variant"),
        "hint should classify as enum variant; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs Active"),
        "hint should suggest `refs Active`; got:\n{stderr}"
    );
}

#[test]
fn fix2_def_unknown_name_no_refs_keeps_legacy_no_match_message() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn alpha() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "totally_made_up_xyz_123"]);
    assert!(
        !out.status.success(),
        "def on an unknown name must fail; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("no exact match"),
        "no-refs path should keep legacy no-match line; got:\n{stderr}"
    );
    assert!(
        !stderr.contains("ref(s) found"),
        "no-refs path must not lie about a ref count; got:\n{stderr}"
    );
}

#[test]
fn fix3_def_struct_redirect_includes_read_impls_refs() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct MyStruct { pub x: u32 }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "MyStruct"]);
    assert!(
        !out.status.success(),
        "def <Struct> should redirect (exit 1); status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("is a struct"),
        "redirect must classify as struct; got:\n{stderr}"
    );
    assert!(
        stderr.contains("Read ") && stderr.contains("Read tool"),
        "redirect must steer to the Read tool on the path:line; got:\n{stderr}"
    );
    assert!(
        stderr.contains("impls MyStruct"),
        "redirect must mention `impls MyStruct`; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs MyStruct"),
        "redirect must mention `refs MyStruct`; got:\n{stderr}"
    );

    assert!(
        stdout_of(&out).trim().is_empty(),
        "redirect prints to stderr; stdout must be empty; got stdout:\n{}",
        stdout_of(&out)
    );
}

#[test]
fn fix3_def_enum_redirect_includes_impls_and_refs() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub enum MyEnum { A, B }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "MyEnum"]);
    assert!(
        !out.status.success(),
        "def <Enum> should redirect (exit 1); status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("is an enum"),
        "redirect must classify as enum; got:\n{stderr}"
    );
    assert!(
        stderr.contains("impls MyEnum"),
        "redirect must mention `impls MyEnum`; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs MyEnum"),
        "redirect must mention `refs MyEnum`; got:\n{stderr}"
    );
}

#[test]
fn fix3_def_fn_unique_match_stays_success_no_redirect() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn my_fn() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "my_fn"]);
    assert!(
        out.status.success(),
        "def on a unique fn must still succeed (no redirect); stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("my_fn"),
        "stdout should carry the path:line:name line; got:\n{stdout}"
    );
    assert!(
        stdout.contains("[fn]"),
        "stdout should tag as [fn]; got:\n{stdout}"
    );
}

#[test]
fn fix3_def_struct_json_keeps_payload_no_redirect() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct JsonStruct;\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "JsonStruct", "-j"]);
    assert!(
        out.status.success(),
        "def -j on a struct should still succeed (JSON payload); stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("def -j must emit valid JSON");
    assert_eq!(v["name"].as_str(), Some("JsonStruct"));
    assert_eq!(v["kind"].as_str(), Some("struct"));
}

#[test]
fn fix3_def_mixed_kind_ambiguity_with_struct_mentions_impls() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct Mixed3;\n#[allow(non_snake_case)] pub fn Mixed3() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "Mixed3"]);
    assert!(
        !out.status.success(),
        "def on mixed-kind ambiguity must fail; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("--func/--struct/--enum"),
        "mixed-kind ambiguity must keep the kind-filter hint (R23-W2); got:\n{stderr}"
    );
    assert!(
        stderr.contains("impls Mixed3"),
        "mixed-kind ambiguity that includes a struct must also mention impls; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs Mixed3"),
        "mixed-kind ambiguity should also mention refs; got:\n{stderr}"
    );
}

#[test]
fn fix3_def_same_kind_struct_ambiguity_mentions_impls() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/a/mod.rs"),
        "pub struct DupStruct;\n",
    );
    write_file(
        &fixture.path().join("src/b/mod.rs"),
        "pub struct DupStruct;\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod a;\npub mod b;\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "def", "DupStruct"]);
    assert!(
        !out.status.success(),
        "def same-kind struct ambiguity must fail; status={:?}",
        out.status
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("--in <PATH_SUBSTR>"),
        "same-kind ambiguity must keep the --in hint (R23-W2); got:\n{stderr}"
    );
    assert!(
        stderr.contains("impls DupStruct"),
        "same-kind struct ambiguity must mention impls (R24-W2); got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs DupStruct"),
        "same-kind struct ambiguity must mention refs; got:\n{stderr}"
    );
}
