

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

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}


fn write_xplat_fixture(dir: &tempfile::TempDir) -> String {
    write_file(
        &dir.path().join("src/lib.rs"),
        "#[cfg(target_os = \"macos\")]\n\
         pub fn state_base() -> u32 { 1 }\n\
         #[cfg(not(target_os = \"macos\"))]\n\
         pub fn state_base() -> u32 { 2 }\n\
         #[cfg(target_os = \"macos\")]\n\
         pub fn caller_macos() -> u32 { state_base() }\n\
         pub fn caller_unconditional() -> u32 { state_base() }\n",
    );
    dir.path().to_string_lossy().to_string()
}

#[test]
fn find_text_shows_cfg_tag_for_each_variant() {
    let dir = tempdir().unwrap();
    let root = write_xplat_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "find", "state_base", "--exact"]);
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("[cfg(target_os = \"macos\")]"),
        "expected macos cfg tag, got stdout: {}\nstderr: {}",
        stdout,
        stderr_of(&out)
    );
    assert!(
        stdout.contains("[cfg(not(target_os = \"macos\"))]"),
        "expected non-macos cfg tag, got stdout: {}",
        stdout
    );
}

#[test]
fn find_json_emits_cfg_attrs_array() {
    let dir = tempdir().unwrap();
    let root = write_xplat_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "-j", "find", "state_base", "--exact"]);
    let stdout = stdout_of(&out);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("find -j produces valid JSON");
    let fns = v["functions"].as_array().expect("functions array");
    let state_base_hits: Vec<&serde_json::Value> = fns
        .iter()
        .filter(|f| f["name"] == "state_base")
        .collect();
    assert_eq!(state_base_hits.len(), 2, "expected two state_base hits");
    let mut seen_macos = false;
    let mut seen_not_macos = false;
    for hit in &state_base_hits {
        let cfg_attrs = hit["cfg_attrs"]
            .as_array()
            .expect("cfg_attrs array on each hit");
        let joined: Vec<String> = cfg_attrs
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect();
        let s = joined.join(" && ");
        if s == "target_os = \"macos\"" {
            seen_macos = true;
        }
        if s == "not(target_os = \"macos\")" {
            seen_not_macos = true;
        }
    }
    assert!(seen_macos && seen_not_macos, "missing one cfg variant");
}

#[test]
fn find_no_cfg_renders_unchanged() {


    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn plain_fn() -> u32 { 1 }\n",
    );
    let root = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &root, "find", "plain_fn", "--exact"]);
    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("[cfg("),
        "non-cfg fn must not get cfg annotation, got: {}",
        stdout
    );
    assert!(stdout.contains("plain_fn"), "expected fn in output: {}", stdout);
}

#[test]
fn def_lists_all_when_only_cfg_differs() {
    let dir = tempdir().unwrap();
    let root = write_xplat_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "def", "state_base"]);
    assert!(
        out.status.success(),
        "def should exit 0 for cfg alternates, status: {:?}\nstderr: {}",
        out.status,
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("differ only by cfg gate"),
        "expected cfg-alternates note on stderr, got: {}",
        stderr
    );

    assert!(
        stdout.contains("[cfg(target_os = \"macos\")]"),
        "stdout missing macos cfg tag: {}",
        stdout
    );
    assert!(
        stdout.contains("[cfg(not(target_os = \"macos\"))]"),
        "stdout missing non-macos cfg tag: {}",
        stdout
    );
}

#[test]
fn def_still_errors_on_real_ambiguity_without_cfg() {

    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/a.rs"),
        "pub fn dup() -> u32 { 1 }\n",
    );
    write_file(
        &dir.path().join("src/b.rs"),
        "pub fn dup() -> u32 { 2 }\n",
    );
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub mod a;\npub mod b;\n",
    );
    let root = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &root, "def", "dup"]);
    assert!(
        !out.status.success(),
        "ambiguous-without-cfg must exit 1; stderr: {}\nstdout: {}",
        stderr_of(&out),
        stdout_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("ambiguous") || stderr.contains("candidates"),
        "expected ambiguity message, got: {}",
        stderr
    );
}

#[test]
fn def_unique_no_cfg_renders_unchanged() {


    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn solo() -> u32 { 1 }\n",
    );
    let root = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &root, "def", "solo"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("solo"));
    assert!(
        !stdout.contains("[cfg("),
        "unique no-cfg def must not carry cfg suffix: {}",
        stdout
    );
}

#[test]
fn callers_shows_cfg_on_target_and_caller() {
    let dir = tempdir().unwrap();
    let root = write_xplat_fixture(&dir);
    let out = run_rustgraph(&["-p", &root, "callers", "state_base"]);
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("[cfg(target_os = \"macos\")]")
            || stdout.contains("[cfg(not(target_os = \"macos\"))]"),
        "expected at least one cfg tag in callers output, got: {}",
        stdout
    );

    assert!(
        stdout.contains("caller_macos"),
        "expected caller_macos in output: {}",
        stdout
    );

    let macos_caller_line = stdout
        .lines()
        .find(|l| l.contains("caller_macos"))
        .expect("caller_macos line");
    assert!(
        macos_caller_line.contains("[cfg(target_os = \"macos\")]"),
        "caller_macos line should carry cfg tag, got: {}",
        macos_caller_line
    );

    let plain_caller_line = stdout
        .lines()
        .find(|l| l.contains("caller_unconditional"))
        .expect("caller_unconditional line");
    assert!(
        !plain_caller_line.contains("[cfg("),
        "caller_unconditional line should not carry cfg tag, got: {}",
        plain_caller_line
    );
}

#[test]
fn callers_no_cfg_at_all_renders_unchanged() {

    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub fn tgt() -> u32 { 1 }\npub fn caller() -> u32 { tgt() }\n",
    );
    let root = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &root, "callers", "tgt"]);
    let stdout = stdout_of(&out);
    assert!(
        !stdout.contains("[cfg("),
        "non-cfg callers output must not contain cfg tags, got: {}",
        stdout
    );
}

#[test]
fn cfg_cascades_through_enclosing_mod() {


    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub mod sys {\n  #[cfg(unix)]\n  pub mod nix {\n    pub fn cascaded_fn() -> u32 { 1 }\n  }\n}\n",
    );
    let root = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &root, "find", "cascaded_fn", "--exact"]);
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("[cfg(unix)]"),
        "expected cascaded mod cfg, got: {}",
        stdout
    );
}

#[test]
fn struct_carries_cfg_attrs_in_find_json() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "#[cfg(target_os = \"linux\")]\npub struct Linuxy { pub x: u32 }\npub struct Plain { pub y: u32 }\n",
    );
    let root = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["-p", &root, "-j", "find", "Linuxy", "--exact"]);
    let stdout = stdout_of(&out);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let s = &v["structs"][0];
    let cfg = s["cfg_attrs"].as_array().expect("cfg_attrs array");
    assert_eq!(cfg.len(), 1);
    assert_eq!(cfg[0].as_str().unwrap(), "target_os = \"linux\"");


    let out2 = run_rustgraph(&["-p", &root, "-j", "find", "Plain", "--exact"]);
    let v2: serde_json::Value = serde_json::from_str(&stdout_of(&out2)).expect("valid JSON");
    let plain_cfg = v2["structs"][0]["cfg_attrs"]
        .as_array()
        .expect("cfg_attrs array");
    assert!(plain_cfg.is_empty(), "expected empty cfg_attrs for Plain");
}
