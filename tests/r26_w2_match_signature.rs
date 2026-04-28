

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
    Command::new(bin).args(full).output().expect("run rustgraph")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}


fn state_mixed_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        r#"
pub struct State;
pub fn process_state() {}
pub fn handle_event(s: &State) {}
pub fn build_view() -> State { State }
pub fn unrelated_helper() -> u32 { 0 }
"#,
    );
    dir
}


fn zorblax_sig_only_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn handler(s: &Zorblax) {}\npub fn other_helper() -> u32 { 0 }\n",
    );
    dir
}


#[test]
fn additive_returns_union_when_same_kind_has_name_and_sig() {
    let fixture = state_mixed_fixture();
    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&[
        "-p", &base, "--match-signature", "find", "State", "--func",
    ]);
    assert!(
        out.status.success(),
        "find --match-signature --func should succeed; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);


    assert!(
        stdout.contains("process_state"),
        "expected name hit `process_state`; got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[name]"),
        "expected `[name]` tag for process_state; got stdout:\n{stdout}"
    );


    assert!(
        stdout.contains("handle_event"),
        "ADDITIVE: handle_event (sig) must join the name hit `process_state`; pre-R26 fallback suppressed this; got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("build_view"),
        "ADDITIVE: build_view (sig) must join the name hit `process_state`; pre-R26 fallback suppressed this; got stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("[sig]"),
        "ADDITIVE: at least one [sig] tag must appear in the union; got stdout:\n{stdout}"
    );

    assert!(
        !stdout.contains("unrelated_helper"),
        "unrelated_helper has no State match anywhere; should not appear; got stdout:\n{stdout}"
    );
}


#[test]
fn sig_only_query_preserved() {
    let fixture = zorblax_sig_only_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "--match-signature", "find", "Zorblax"]);
    assert!(
        out.status.success(),
        "find --match-signature should succeed; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("handler") && stdout.contains("[sig]"),
        "expected sig-only hit `handler [sig]`; got stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("[name]"),
        "no fn/struct/enum named Zorblax; expected zero [name] tags; got stdout:\n{stdout}"
    );
}


#[test]
fn default_mode_unchanged_no_match_exits_one() {
    let fixture = zorblax_sig_only_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "find", "Zorblax"]);
    assert!(
        !out.status.success(),
        "expected exit 1 without --match-signature; status={:?}",
        out.status
    );

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("no match for query"),
        "expected `no match for query` error in stderr; got:\n{stderr}"
    );

    assert!(
        !stderr.contains("(--match-signature)"),
        "R26-W2 note keyed on --match-signature; default mode stays quiet; got:\n{stderr}"
    );
}


#[test]
fn default_mode_unchanged_name_only() {
    let fixture = state_mixed_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "find", "State"]);
    assert!(
        out.status.success(),
        "find State default-mode should succeed (struct State exists); stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("struct State") && stdout.contains("[name]"),
        "expected name hit `struct State [name]`; got stdout:\n{stdout}"
    );

    assert!(
        !stdout.contains("handle_event"),
        "default mode: handle_event has no name match; should not appear without --match-signature; got stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("build_view"),
        "default mode: build_view has no name match; should not appear without --match-signature; got stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("[sig]"),
        "default mode must not emit any [sig] tag; got stdout:\n{stdout}"
    );
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("(--match-signature)"),
        "R26-W2 note must not fire in default mode; got stderr:\n{stderr}"
    );
}


#[test]
fn additive_json_carries_match_kind_per_hit() {
    let fixture = state_mixed_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "--match-signature", "find", "State", "-j",
    ]);
    assert!(
        out.status.success(),
        "find --match-signature -j must succeed; status={:?} stderr={}",
        out.status,
        stderr_of(&out)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");


    let structs = json["structs"].as_array().expect("structs array");
    assert!(
        !structs.is_empty(),
        "expected struct hit in additive json; got: {json:#?}"
    );
    for s in structs {
        let mk = s["match_kind"].as_str().expect("match_kind on struct");
        assert_eq!(
            mk, "name",
            "struct State should be tagged `name`; got {mk:?} on {s:#?}"
        );
    }


    let funcs = json["functions"].as_array().expect("functions array");
    assert!(
        funcs.len() >= 3,
        "expected 3 fn hits in additive json (process_state name + handle_event/build_view sig); got: {funcs:#?}"
    );
    let mut by_name: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for f in funcs {
        let name = f["name"].as_str().expect("fn name");
        let mk = f["match_kind"].as_str().expect("match_kind on fn");
        by_name.insert(name, mk);
    }
    assert_eq!(
        by_name.get("process_state").copied(),
        Some("name"),
        "process_state should be tagged `name` (substring of `state`); got: {by_name:?}"
    );
    assert_eq!(
        by_name.get("handle_event").copied(),
        Some("sig"),
        "handle_event should be tagged `sig` (param `&State`); got: {by_name:?}"
    );
    assert_eq!(
        by_name.get("build_view").copied(),
        Some("sig"),
        "build_view should be tagged `sig` (return `-> State`); got: {by_name:?}"
    );
}


#[test]
fn stderr_note_mixed_emits_additive_breakdown() {
    let fixture = state_mixed_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "--match-signature", "find", "State"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("2 name + 2 sig/path matches"),
        "additive: mixed result set should emit `2 name + 2 sig/path matches` note; got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("(--match-signature)"),
        "additive note must credit --match-signature; got stderr:\n{stderr}"
    );

    assert!(
        !stderr.contains("0 name matches; showing"),
        "OLD-format `0 name matches; showing` must not fire when name hits exist; got stderr:\n{stderr}"
    );
}


#[test]
fn stderr_note_silent_when_additive_added_nothing() {
    let dir = tempdir().expect("tempdir");
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub struct State;\npub fn unrelated_helper() {}\n",
    );
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "--match-signature", "find", "State"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));

    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("struct State"),
        "expected struct State in stdout; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("[sig]"),
        "no sig hits expected; got stdout:\n{stdout}"
    );

    let stderr = stderr_of(&out);

    assert!(
        !stderr.contains("(--match-signature)"),
        "no additive note when sig/path tier added nothing; got stderr:\n{stderr}"
    );
}


#[test]
fn r22_w1_sig_tag_still_renders_in_additive_path() {
    let fixture = zorblax_sig_only_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "--match-signature", "find", "Zorblax", "--func",
    ]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("[sig]"),
        "R22-W1: [sig] tag must still render in additive path; got stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("[name]"),
        "no fn named Zorblax; expected zero [name] tags; got stdout:\n{stdout}"
    );
}


#[test]
fn exact_mode_ignores_match_signature_in_additive_path() {
    let fixture = state_mixed_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p", &base, "--match-signature", "find", "State", "--exact",
    ]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("struct State") && stdout.contains("[name]"),
        "expected exact name hit `struct State [name]`; got stdout:\n{stdout}"
    );

    assert!(
        !stdout.contains("handle_event"),
        "--exact bypasses sig tier; handle_event must not appear; got stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("build_view"),
        "--exact bypasses sig tier; build_view must not appear; got stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("[sig]"),
        "--exact mode must never emit [sig] tag; got stdout:\n{stdout}"
    );
}


#[test]
fn help_text_documents_additive_semantic() {


    let out = run_rustgraph(&["find", "--help"]);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        !stdout.contains("Fallback when NAME yields 0 hits"),
        "old fallback wording must be gone from `find --help`; got:\n{stdout}"
    );
}
