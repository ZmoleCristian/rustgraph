

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


#[test]
fn find_exact_name_hit_renders_name_tag() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn handle_event() {}\npub fn unrelated() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "handle_event"]);
    assert!(out.status.success(), "find failed: stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("handle_event"),
        "expected hit in stdout; got:\n{stdout}"
    );
    assert!(
        stdout.contains("[name]"),
        "expected `[name]` tag on every hit (R22-W1); got:\n{stdout}"
    );
    assert!(
        !stdout.contains("[sig]"),
        "default mode must not emit `[sig]` (sig fallback gated on --match-signature); got:\n{stdout}"
    );
    assert!(
        !stdout.contains("[path]"),
        "default mode must not emit `[path]`; got:\n{stdout}"
    );
}


#[test]
fn find_fuzzy_name_hit_renders_name_tag() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),


        "pub fn handle_event_extended_v2() {}\npub fn unrelated_helper() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "handle_event_extended"]);
    assert!(out.status.success(), "find failed: stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("handle_event_extended_v2"),
        "expected fuzzy-name hit; got:\n{stdout}"
    );
    assert!(
        stdout.contains("[name]"),
        "fuzzy-name hit should still tag `[name]`; got:\n{stdout}"
    );
}


#[test]
fn find_match_signature_signature_fragment_renders_sig_tag() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct InventoryState;
pub fn handle_event(s: &InventoryState) {}
pub fn build_view() -> InventoryState { InventoryState }
pub fn unrelated_helper() -> u32 { 0 }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--match-signature",
        "find",
        "InventoryState",
        "--func",
    ]);
    assert!(
        out.status.success(),
        "--match-signature failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("handle_event") && stdout.contains("build_view"),
        "expected both signature-text matches; got:\n{stdout}"
    );
    assert!(
        stdout.contains("[sig]"),
        "expected `[sig]` tag on signature-fallback hits (R22-W1); got:\n{stdout}"
    );

    assert!(
        !stdout.contains("[name]"),
        "no fn named InventoryState; expected zero `[name]` tags on --func filtering; got:\n{stdout}"
    );
}


#[test]
fn find_match_signature_path_fragment_renders_path_tag() {
    let fixture = tempdir().expect("tempdir");


    write_file(
        &fixture.path().join("src/sessions/parse.rs"),
        "pub fn parser_one() {}\npub fn parser_two() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--match-signature",
        "find",
        "sessions",
        "--func",
    ]);
    assert!(
        out.status.success(),
        "--match-signature path-fallback failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("[path]"),
        "expected `[path]` tag on path-fallback hit (R22-W1); got:\n{stdout}"
    );


    assert!(
        !stdout.contains("[name]"),
        "path-only hit should not be tagged `[name]`; got:\n{stdout}"
    );
}


#[test]
fn find_json_output_includes_match_kind_field() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Inventory;
pub fn handle_event(s: &Inventory) {}
pub fn unique_handler_fn() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&[
        "--path",
        &base,
        "find",
        "unique_handler_fn",
        "-j",
        "--func",
    ]);
    assert!(out.status.success(), "find failed: stderr={}", stderr_of(&out));
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let funcs = json["functions"].as_array().expect("functions array");
    assert!(!funcs.is_empty(), "expected at least one fn hit; json={json:#?}");
    for f in funcs {
        let mk = f["match_kind"].as_str().expect("match_kind field present");
        assert_eq!(mk, "name", "default-mode hit should be `name`; got {mk:?}");
    }


    let out = run_rustgraph(&[
        "--path",
        &base,
        "--match-signature",
        "find",
        "Inventory",
        "-j",
        "--func",
    ]);
    assert!(
        out.status.success(),
        "--match-signature failed: stderr={}",
        stderr_of(&out)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let funcs = json["functions"].as_array().expect("functions array");
    assert!(!funcs.is_empty(), "expected at least one fn hit; json={json:#?}");
    for f in funcs {
        let mk = f["match_kind"].as_str().expect("match_kind field present");
        assert_eq!(
            mk, "sig",
            "sig-fallback hit should be `sig`; got {mk:?} in {f:#?}"
        );
    }
}


#[test]
fn find_exact_case_insensitive_matches_lowercase_definition() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn handle_event() {}\npub struct ImportantThing;\npub enum SomeColor { Red, Blue }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&["--path", &base, "find", "HANDLE_EVENT", "--exact"]);
    assert!(
        out.status.success(),
        "--exact uppercase query failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("handle_event"),
        "uppercase HANDLE_EVENT should match lowercase handle_event (case-insensitive --exact); got:\n{stdout}"
    );

    assert!(
        stdout.contains("[name]"),
        "--exact hit should still carry `[name]` tag (R22-W1 uniform tagging); got:\n{stdout}"
    );


    let out = run_rustgraph(&["--path", &base, "find", "iMpOrTaNtThInG", "--exact"]);
    assert!(
        out.status.success(),
        "--exact mixed-case struct query failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("ImportantThing"),
        "mixed-case query should match struct (case-insensitive --exact); got:\n{stdout}"
    );


    let out = run_rustgraph(&["--path", &base, "find", "somecolor", "--exact"]);
    assert!(
        out.status.success(),
        "--exact lowercase query failed: stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("SomeColor"),
        "lowercase query should match enum SomeColor (case-insensitive --exact); got:\n{stdout}"
    );
}


#[test]
fn find_exact_json_carries_name_match_kind() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() {}\npub fn beta() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "alpha", "--exact", "-j"]);
    assert!(out.status.success(), "find --exact -j failed: stderr={}", stderr_of(&out));
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let funcs = json["functions"].as_array().expect("functions array");
    assert_eq!(funcs.len(), 1, "expected exactly one alpha hit; got: {funcs:#?}");
    assert_eq!(
        funcs[0]["match_kind"].as_str(),
        Some("name"),
        "--exact JSON hit should carry match_kind=\"name\"; got: {:#?}",
        funcs[0]
    );
}
