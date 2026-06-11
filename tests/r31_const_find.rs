//! R31: the redstring field-report fixes.
//!
//! An agent ran `find KIND_META` (a `pub const`) and got `struct KindMeta`
//! back as a bare `[name]` hit — consts were unindexed and the Levenshtein
//! neighbor wore an exact hit's clothes. Refining to `KIND_META static`
//! produced a hard error whose hint (`--search-threshold`) wasn't reachable
//! from the MCP schema. These tests pin every layer of the fix.

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

/// The redstring layout: a PascalCase struct and a SCREAMING_SNAKE const one
/// Levenshtein deletion apart, with the const table spanning multiple lines.
fn redstring_fixture(dir: &Path) {
    write_file(
        &dir.join("src/entity.rs"),
        "pub struct KindMeta {\n\
         \x20   pub label: &'static str,\n\
         }\n\
         \n\
         pub const KIND_META: [KindMeta; 3] = [\n\
         \x20   KindMeta { label: \"alpha\" },\n\
         \x20   KindMeta { label: \"beta\" },\n\
         \x20   KindMeta { label: \"gamma\" },\n\
         ];\n\
         \n\
         static REGISTRY: u32 = 0;\n\
         \n\
         impl KindMeta {\n\
         \x20   pub const MAX_LABEL: usize = 16;\n\
         }\n",
    );
}


#[test]
fn find_const_by_exact_name_returns_const_not_struct_homograph() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "KIND_META"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("pub const KIND_META: [KindMeta; 3]"),
        "const must be found with its signature: {stdout}"
    );
    // The const spans the full initializer so the agent Reads the right range.
    assert!(
        stdout.contains(":5-9"),
        "const line range must span the table (5-9): {stdout}"
    );
    // The struct homograph may stay visible, but only below the exact block
    // and wearing its similarity score — never dressed as an exact hit.
    let exact_pos = stdout.find("pub const KIND_META").unwrap();
    if let Some(struct_pos) = stdout.find("struct KindMeta") {
        assert!(
            struct_pos > exact_pos,
            "exact const must render above the fuzzy struct: {stdout}"
        );
        assert!(
            stdout.contains("== Fuzzy matches =="),
            "struct homograph must sit under the fuzzy header: {stdout}"
        );
        assert!(
            stdout.contains("[name ~0."),
            "fuzzy struct must carry a similarity tag: {stdout}"
        );
    }
}


#[test]
fn find_static_and_assoc_const_are_indexed() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "REGISTRY"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains("static REGISTRY: u32"),
        "statics must be findable: {}",
        stdout_of(&out)
    );

    let out = run_rustgraph(&["--path", &base, "find", "MAX_LABEL"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains("MAX_LABEL"),
        "impl-associated consts must be findable: {}",
        stdout_of(&out)
    );
}


#[test]
fn find_const_kind_filter_narrows_to_consts() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "kind", "--const"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("KIND_META"), "const filter keeps consts: {stdout}");
    assert!(
        !stdout.contains("struct KindMeta"),
        "--const must exclude structs: {stdout}"
    );
}


#[test]
fn find_fuzzy_only_result_announces_no_exact_match_and_scores() {
    // The literal Event-2 failure from the field report: only the struct
    // exists, the query is the (missing) const's exact name, and the struct
    // sneaks past the 0.85 threshold at 0.889. It must NOT render as a bare
    // `[name]` hit anymore.
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub struct KindMeta { pub label: &'static str }\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "KIND_META", "--struct"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("no exact name match for 'KIND_META'"),
        "fuzzy-only output must lead with the no-exact disclaimer: {stdout}"
    );
    assert!(
        stdout.contains("[name ~0.89]"),
        "fuzzy name hits must carry a similarity tag: {stdout}"
    );
    assert!(
        !stdout.contains("] - struct KindMeta\n["),
        "no bare [name] tag on a fuzzy hit: {stdout}"
    );
}


#[test]
fn find_exact_hit_keeps_plain_name_tag() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "KIND_META"]);
    let stdout = stdout_of(&out);
    assert!(stdout.contains("[name]"), "exact hits keep the bare tag: {stdout}");
    assert!(
        !stdout.contains("no exact name match"),
        "exact hit must not render the fuzzy disclaimer: {stdout}"
    );
}


#[test]
fn find_whitespace_query_refuses_with_steering_and_token_suggestions() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "KIND_META static"]);
    assert!(
        !out.status.success(),
        "whitespace query must exit non-zero; stdout: {}",
        stdout_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(stderr.contains("contains whitespace"), "got: {stderr}");
    assert!(
        stderr.contains("KIND_META [const]"),
        "suggestions from the longest token must surface the real target: {stderr}"
    );
    assert!(
        !stderr.contains("--search-threshold"),
        "tripwire must not suggest threshold tuning for a phrase query: {stderr}"
    );
}


#[test]
fn find_or_query_with_spaces_around_pipe_still_works() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    // `a | b` has whitespace but each pipe-term is a single identifier — legal.
    let out = run_rustgraph(&["--path", &base, "find", "KIND_META | REGISTRY"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("KIND_META"), "{stdout}");
    assert!(stdout.contains("REGISTRY"), "{stdout}");
}


#[test]
fn find_no_match_hint_is_surface_neutral() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "zzzzqqqq"]);
    assert!(!out.status.success());
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("lower the threshold"),
        "hint must name the concept, not a CLI-only flag: {stderr}"
    );
    assert!(
        !stderr.contains("--search-threshold"),
        "no-match hint must stay actionable from the MCP surface: {stderr}"
    );
}


#[test]
fn find_json_partitions_consts_and_scores_fuzzy_hits() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "-j", "find", "KIND_META"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).expect("valid JSON");

    let consts_exact = v["consts_exact"].as_array().unwrap();
    assert_eq!(consts_exact.len(), 1, "{v}");
    assert_eq!(consts_exact[0]["name"].as_str(), Some("KIND_META"));
    assert_eq!(consts_exact[0]["kind"].as_str(), Some("const"));
    assert!(
        consts_exact[0]["symbol_id"]
            .as_str()
            .unwrap()
            .starts_with("const:"),
        "{v}"
    );
    assert!(
        consts_exact[0]["match_score"].is_null(),
        "exact hits carry no score: {v}"
    );
}


#[test]
fn callers_on_const_redirects_to_read_and_refs() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "KIND_META"]);
    assert!(!out.status.success(), "consts have no callers; must steer");
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("is a const") && stderr.contains("Read tool"),
        "callers on a const must steer to Read/refs: {stderr}"
    );
}


#[test]
fn inventory_lists_consts_with_count_summary() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "inventory", "--const"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("KIND_META"), "{stdout}");
    assert!(stdout.contains("Consts: 3"), "{stdout}");
    assert!(!stdout.contains("struct KindMeta"), "{stdout}");
}


#[test]
fn tree_counts_consts_per_file() {
    let dir = tempdir().unwrap();
    redstring_fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "tree"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("3 const"),
        "tree must annotate const counts when present: {stdout}"
    );
}
