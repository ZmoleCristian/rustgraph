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

fn write_simple_lib(fixture: &tempfile::TempDir) -> String {
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn alpha() {}
pub fn beta() {}
pub fn gamma() {}
pub struct Item { pub id: u32 }
pub enum Kind { A, B }
"#,
    );
    fixture.path().to_string_lossy().to_string()
}

#[test]
fn analyze_functions_only_excludes_structs_and_enums() {
    let fixture = tempdir().expect("tempdir");
    let base = write_simple_lib(&fixture);

    let out = run_rustgraph(&["--path", &base, "--analyze", "functions", "--json"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(json["functions"].as_array().map(|a| a.len()), Some(3));
    assert!(json["structs"].as_array().is_none_or(|a| a.is_empty()));
    assert!(json["enums"].as_array().is_none_or(|a| a.is_empty()));
}

#[test]
fn analyze_structs_only_returns_only_structs() {
    let fixture = tempdir().expect("tempdir");
    let base = write_simple_lib(&fixture);

    let out = run_rustgraph(&["--path", &base, "--analyze", "structs", "--json"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(json["structs"].as_array().map(|a| a.len()), Some(1));
}

#[test]
fn analyze_enums_only_returns_only_enums() {
    let fixture = tempdir().expect("tempdir");
    let base = write_simple_lib(&fixture);

    let out = run_rustgraph(&["--path", &base, "--analyze", "enums", "--json"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(json["enums"].as_array().map(|a| a.len()), Some(1));
}

#[test]
fn analyze_both_returns_all_categories() {
    let fixture = tempdir().expect("tempdir");
    let base = write_simple_lib(&fixture);

    let out = run_rustgraph(&["--path", &base, "--func", "--struct", "--enum", "--json"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert!(json["functions"].as_array().map(|a| a.len()).unwrap_or(0) >= 1);
    assert!(json["structs"].as_array().map(|a| a.len()).unwrap_or(0) >= 1);
    assert!(json["enums"].as_array().map(|a| a.len()).unwrap_or(0) >= 1);
}

#[test]
fn bare_invocation_prints_help_instead_of_inventory() {
    let out = run_rustgraph(&["--path", "."]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Usage: rustgraph") && stdout.contains("Commands:"),
        "expected --help text, got: {}",
        stdout
    );
}

#[test]
fn func_shortcut_flag_implies_functions_only() {
    let fixture = tempdir().expect("tempdir");
    let base = write_simple_lib(&fixture);
    let out = run_rustgraph(&["--path", &base, "--func", "--json"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(json["functions"].as_array().map(|a| a.len()), Some(3));
}

#[test]
fn search_filter_filters_by_name() {
    let fixture = tempdir().expect("tempdir");
    let base = write_simple_lib(&fixture);
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--analyze",
        "functions",
        "--search",
        "alpha",
        "--json",
    ]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let names: Vec<String> = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["name"].as_str().map(String::from))
        .collect();
    assert!(names.contains(&"alpha".to_string()));
    assert!(!names.contains(&"beta".to_string()));
}

#[test]
fn search_filter_supports_or_via_pipe() {
    let fixture = tempdir().expect("tempdir");
    let base = write_simple_lib(&fixture);
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--analyze",
        "functions",
        "--search",
        "alpha|beta",
        "--json",
    ]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let names: Vec<String> = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["name"].as_str().map(String::from))
        .collect();
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"beta".to_string()));
}

#[test]
fn search_threshold_extreme_filters_more_aggressively() {
    let fixture = tempdir().expect("tempdir");
    let base = write_simple_lib(&fixture);
    let out_loose = run_rustgraph(&[
        "--path",
        &base,
        "--analyze",
        "functions",
        "--search",
        "xyzabc",
        "--search-threshold",
        "0.1",
        "--json",
    ]);
    assert!(out_loose.status.success());
    let json: Value = serde_json::from_slice(&out_loose.stdout).expect("valid json");
    let loose_count = json["functions"].as_array().unwrap().len();

    let out_strict = run_rustgraph(&[
        "--path",
        &base,
        "--analyze",
        "functions",
        "--search",
        "xyzabc",
        "--search-threshold",
        "0.99",
        "--json",
    ]);
    assert!(out_strict.status.success());
    let json: Value = serde_json::from_slice(&out_strict.stdout).expect("valid json");
    let strict_count = json["functions"].as_array().unwrap().len();
    assert!(loose_count >= strict_count);
}

#[test]
fn ensemble_summary_view_includes_structs_and_call_sites_no_code() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Req { pub x: u32 }
pub fn handle(req: Req) -> u32 { req.x }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "handle",
        "--ensemble-view",
        "summary",
        "--json",
    ]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let sections = json["sections"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    assert!(
        !sections.contains(&"code"),
        "summary should NOT include full code body"
    );
    assert!(sections.contains(&"structs"));
    assert!(sections.contains(&"call-sites"));

    assert!(sections.contains(&"dataflow"));

    assert!(sections.contains(&"neighborhood"));

    assert!(!sections.contains(&"lifecycle"));
    assert!(!sections.contains(&"boundaries"));
}

#[test]
fn ensemble_full_view_includes_all_sections() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn solo() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "solo",
        "--ensemble-view",
        "full",
        "--json",
    ]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let sections_count = json["sections"].as_array().map(|a| a.len()).unwrap_or(0);
    assert_eq!(sections_count, 7);
}

#[test]
fn output_flag_writes_file_instead_of_stdout() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn one() {}\n");
    let out_path = fixture.path().join("out.json");
    let base = fixture.path().to_string_lossy().to_string();
    let out_path_str = out_path.to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--analyze",
        "functions",
        "--json",
        "--output",
        &out_path_str,
    ]);
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "stdout should be empty when --output writes to file"
    );

    let written = fs::read_to_string(&out_path).expect("output file");
    let parsed: Value = serde_json::from_str(&written).expect("valid json file");
    assert_eq!(parsed["functions"].as_array().map(|a| a.len()), Some(1));
}

#[test]
fn callers_no_color_in_text_output_by_default() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target() {}
pub fn caller() { target(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "target"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("\u{1b}["),
        "default callers output should be uncolored"
    );
    assert!(stdout.contains("Callers:"));
}

#[test]
fn call_graph_dot_flag_emits_dot_digraph() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn alpha() { beta(); }
pub fn beta() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "call-graph", "--dot"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("digraph call_graph"));
    assert!(stdout.contains("rankdir=LR"));
}

#[test]
fn call_graph_default_emits_text_tree_header() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() { beta(); }\npub fn beta() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "call-graph"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("rustgraph call-graph --text"),
        "expected text-mode header, got:\n{}",
        stdout
    );
    assert!(!stdout.contains("digraph call_graph"));
}

#[test]
fn call_graph_external_detail_emits_external_nodes() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn run() {
    let _ = std::fs::read_to_string("a.txt");
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "call-graph",
        "--dot",
        "--detail",
        "external",
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ext::"));
}

#[test]
fn callers_unknown_query_exits_nonzero() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn one() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--callers",
        "completely_unknown_xyz",
        "--json",
    ]);
    assert!(!out.status.success(), "should exit non-zero on no match");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no function matches query 'completely_unknown_xyz'"),
        "stderr should explain the failure, got: {stderr}"
    );
}

#[test]
fn ensemble_json_includes_neighborhood_keys() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn caller() { callee(); }
pub fn callee() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--ensemble", "callee", "--json"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let nbh = &json["matches"][0]["neighborhood"];
    assert!(nbh["upstream_total"].as_u64().is_some());
    assert!(nbh["downstream_total"].as_u64().is_some());
    assert!(nbh["call_depth"].as_u64().is_some());
}

#[test]
fn dead_code_text_output_emits_summary_line() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn live() { worker(); }
pub fn worker() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Summary:"));
    assert!(stdout.contains("candidates"));
}

#[test]
fn ensemble_lifecycle_types_can_be_pipe_separated() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct A { pub x: u32 }
pub struct B { pub y: u32 }
pub fn make_a() -> A { A { x: 1 } }
pub fn make_b() -> B { B { y: 2 } }
pub fn run() { let _a = make_a(); let _b = make_b(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "run",
        "--json",
        "--ensemble-view",
        "full",
        "--ensemble-lifecycle-types",
        "A|B",
    ]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let lifecycles = json["matches"][0]["lifecycles"]
        .as_array()
        .expect("lifecycles");
    let names: Vec<&str> = lifecycles
        .iter()
        .filter_map(|l| l["type_name"].as_str())
        .collect();
    assert!(names.contains(&"A"));
    assert!(names.contains(&"B"));
}

#[test]
fn search_threshold_zero_matches_everything() {
    let fixture = tempdir().expect("tempdir");
    let base = write_simple_lib(&fixture);
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--analyze",
        "functions",
        "--search",
        "qrstuv",
        "--search-threshold",
        "0.0",
        "--json",
    ]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(json["functions"].as_array().map(|a| a.len()), Some(3));
}

#[test]
fn dead_code_alias_orphans_works() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn dead() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "orphans", "--json"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let names: Vec<String> = json["dead_functions"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|f| f["info"]["name"].as_str().map(String::from))
        .collect();
    assert!(names.contains(&"dead".to_string()));
}

#[test]
fn json_output_is_pretty_formatted() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn one() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--analyze", "functions", "--json"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);

    let json: Value = serde_json::from_str(&stdout).expect("valid json");
    assert!(json.is_object());
    assert!(
        stdout.starts_with("{\n"),
        "pretty JSON opens the object on its own line"
    );
    assert!(
        stdout.contains("\n  \"functions\": ["),
        "pretty JSON indents keys and separates them with \": \""
    );
}

#[test]
fn ensemble_quick_preset_produces_smaller_call_depth_than_deep() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn root() { level1(); }
pub fn level1() { level2(); }
pub fn level2() { level3(); }
pub fn level3() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let quick = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "root",
        "--ensemble-preset",
        "quick",
        "--json",
    ]);
    let deep = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "root",
        "--ensemble-preset",
        "deep",
        "--json",
    ]);
    assert!(quick.status.success() && deep.status.success());
    let q: Value = serde_json::from_slice(&quick.stdout).expect("quick json");
    let d: Value = serde_json::from_slice(&deep.stdout).expect("deep json");
    let q_depth = q["call_depth"].as_u64().unwrap_or(0);
    let d_depth = d["call_depth"].as_u64().unwrap_or(0);
    assert!(d_depth > q_depth);
}

#[test]
fn ensemble_balanced_preset_is_default() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "pub fn solo() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let no_preset = run_rustgraph(&["--path", &base, "--ensemble", "solo", "--json"]);
    let balanced = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "solo",
        "--ensemble-preset",
        "balanced",
        "--json",
    ]);
    assert!(no_preset.status.success() && balanced.status.success());
    let n: Value = serde_json::from_slice(&no_preset.stdout).expect("no preset json");
    let b: Value = serde_json::from_slice(&balanced.stdout).expect("balanced json");
    assert_eq!(n["call_depth"], b["call_depth"]);
    assert_eq!(n["max_results"], b["max_results"]);
}

#[test]
fn top_level_help_lists_subcommands() {
    let out = run_rustgraph(&["--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("callers"));
    assert!(stdout.contains("ensemble"));
    assert!(stdout.contains("dead-code"));
    assert!(stdout.contains("call-graph"));
    assert!(stdout.contains("tree"));
}

#[test]
fn ensemble_no_match_exits_nonzero() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn anything() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "completely_unknown_xyz",
        "--json",
    ]);
    assert!(!out.status.success(), "should exit non-zero on no match");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no ensemble matches for 'completely_unknown_xyz'"),
        "stderr should explain the failure, got: {stderr}"
    );
}

#[test]
fn callers_subcommand_separate_before_after_context() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target() {}
pub fn alpha() {
    let _x = 1;
    target();
    let _y = 2;
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path", &base, "callers", "-B", "1", "-A", "1", "target", "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let context = json["matches"][0]["callers"][0]["call_sites"][0]["context_lines"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(context.len() >= 2);
}

#[test]
fn color_with_json_flag_keeps_json_pure() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn x() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--analyze",
        "functions",
        "--json",
        "--color",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains('\u{1b}'));
    let _: Value = serde_json::from_slice(&out.stdout).expect("valid json");
}

#[test]
fn empty_project_dead_code_returns_no_dead_functions() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(json["dead_functions"].as_array().map(|a| a.len()), Some(0));
    assert_eq!(json["candidates_total"].as_u64(), Some(0));
}

#[test]
fn empty_project_inventory_returns_zero_counts() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), "");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--analyze", "functions", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(json["functions"].as_array().map(|a| a.len()), Some(0));
}

#[test]
fn callers_context_flag_unifies_before_and_after() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target() {}
pub fn caller() {
    let _ = 1;
    target();
    let _ = 2;
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "-C", "2", "target", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let context = json["matches"][0]["callers"][0]["call_sites"][0]["context_lines"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    assert!(context.len() >= 3);
}

#[test]
fn ensemble_deep_preset_disables_call_site_cap() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target() {}
pub fn caller() { target(); }
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path", &base, "ensemble", "target", "--preset", "deep", "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(json["max_call_sites"].as_u64(), Some(0));
    assert_eq!(json["max_results"].as_u64(), Some(10));
    assert_eq!(json["call_depth"].as_u64(), Some(3));
}

#[test]
fn callers_with_after_context_only_includes_trailing_lines() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target() {}
pub fn driver() {
    let prev = 1;
    target();
    let after_a = 2;
    let after_b = 3;
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "target", "-A", "2", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let context_lines = &json["matches"][0]["callers"][0]["call_sites"][0]["context_lines"];
    let arr = context_lines.as_array().expect("context lines array");
    let match_idx = arr
        .iter()
        .position(|line| line["is_match"].as_bool() == Some(true))
        .expect("match line present");

    assert_eq!(match_idx, 0, "expected match at start with only -A context");
    assert!(arr.len() > 1);
}

#[test]
fn dead_code_returns_skipped_metric_keys() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn driver() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let skipped = &json["skipped"];
    for key in [
        "reexported_total",
        "cfg_test_total",
        "runtime_entrypoints_total",
        "ambiguous_name_total",
        "framework_handler_refs_total",
    ] {
        assert!(
            skipped.get(key).is_some(),
            "skipped report missing {key}: {skipped:?}"
        );
    }
}

#[test]
fn dead_code_text_output_lists_dead_function_signatures() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn unused_api() {}
pub fn driver() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("unused_api"),
        "expected dead function name in text output, got: {stdout}"
    );
}

#[test]
fn ensemble_query_with_file_line_returns_innermost_function() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn outer() {
    inner();
}

pub fn inner() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let target = format!("{}/src/lib.rs:6", base);
    let out = run_rustgraph(&["--path", &base, "ensemble", &target, "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let matches = json["matches"].as_array().unwrap_or(&Vec::new()).clone();
    assert_eq!(matches.len(), 1, "expected exactly one match for line 6");
    assert_eq!(matches[0]["info"]["name"].as_str(), Some("inner"));

    let rel = run_rustgraph(&["--path", &base, "--ensemble", "src/lib.rs:6", "--json"]);
    assert!(
        rel.status.success(),
        "{}",
        String::from_utf8_lossy(&rel.stderr)
    );
    let rel_json: Value = serde_json::from_slice(&rel.stdout).expect("valid rel json");
    assert_eq!(
        rel_json["matches"][0]["info"]["name"].as_str(),
        Some("inner"),
        "relative path:line query should resolve same as absolute"
    );
}

#[test]
fn analyze_with_invalid_rust_emits_parse_error_to_stderr() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/good.rs"), "pub fn good() {}\n");
    write_file(
        &fixture.path().join("src/broken.rs"),
        "this is not parseable rust",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--analyze", "functions", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("broken.rs"),
        "expected parse error to mention broken.rs, got: {stderr}"
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let functions = json["functions"].as_array().expect("array");
    assert_eq!(functions.len(), 1);
}

#[test]
fn callers_subcommand_with_context_includes_match_marker_in_lines() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target() {}
pub fn driver() {
    let prefix = 1;
    target();
    let suffix = 3;
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "target", "-C", "1", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let context = &json["matches"][0]["callers"][0]["call_sites"][0]["context_lines"];
    let arr = context.as_array().expect("array");
    let match_count = arr
        .iter()
        .filter(|line| line["is_match"].as_bool() == Some(true))
        .count();
    assert_eq!(match_count, 1, "expected exactly one match-marked line");
}

#[test]
fn call_graph_with_empty_project_emits_only_header_and_footer() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("Cargo.toml"), "[package]\n");

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "call-graph", "--dot"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("digraph call_graph"));
    assert!(stdout.contains("rankdir=LR"));

    assert!(!stdout.contains("->"), "expected no edges, got: {stdout}");
}

#[test]
fn regression_grep_by_function_attributes_to_enclosing_fn_not_orphan() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() {\n    let _ = some.unwrap();\n    let _ = other.unwrap();\n}\nfn beta() {\n    let _ = third.unwrap();\n}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "grep", "--by-function", "unwrap"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("alpha"),
        "alpha should be attributed; got: {}",
        stdout
    );
    assert!(
        stdout.contains("beta"),
        "beta should be attributed; got: {}",
        stdout
    );
    assert!(
        stdout.contains("0 orphan") || !stdout.contains("orphan"),
        "no matches should be orphaned; got: {}",
        stdout
    );
}

#[test]
fn regression_callers_homonym_attributes_by_module() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/daemon/mod.rs"),
        "pub fn shared() {}\n",
    );
    write_file(
        &fixture.path().join("src/session/mod.rs"),
        "pub fn shared() {}\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod daemon; pub mod session;\nfn from_daemon() { crate::daemon::shared(); }\nfn from_session() { crate::session::shared(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "shared", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let matches = json["matches"].as_array().unwrap();
    for m in matches {
        let target_path = m["info"]["file_path"].as_str().unwrap();
        let callers = m["callers"].as_array().unwrap();
        let names: Vec<&str> = callers
            .iter()
            .map(|c| c["info"]["name"].as_str().unwrap())
            .collect();
        if target_path.contains("daemon") {
            assert!(
                names.iter().any(|n| *n == "from_daemon"),
                "daemon::shared should be called by from_daemon: {names:?}"
            );
            assert!(
                !names.iter().any(|n| *n == "from_session"),
                "daemon::shared should NOT be called by from_session: {names:?}"
            );
        } else if target_path.contains("session") {
            assert!(
                names.iter().any(|n| *n == "from_session"),
                "session::shared should be called by from_session: {names:?}"
            );
            assert!(
                !names.iter().any(|n| *n == "from_daemon"),
                "session::shared should NOT be called by from_daemon: {names:?}"
            );
        }
    }
}

#[test]
fn regression_callers_symbol_id_bypasses_fuzzy() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\nfn caller() { target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let find_out = run_rustgraph(&["--path", &base, "find", "target", "--json"]);
    assert!(find_out.status.success());
    let find_json: Value = serde_json::from_slice(&find_out.stdout).expect("valid json");
    let symbol_id = find_json["functions"][0]["symbol_id"].as_str().unwrap();

    let callers_out = run_rustgraph(&[
        "--path",
        &base,
        "callers",
        "ignored_query",
        "--symbol-id",
        symbol_id,
        "--json",
    ]);
    assert!(
        callers_out.status.success(),
        "{}",
        String::from_utf8_lossy(&callers_out.stderr)
    );
    let json: Value = serde_json::from_slice(&callers_out.stdout).expect("valid json");
    let matches = json["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["info"]["name"].as_str().unwrap(), "target");
}

#[test]
fn regression_find_text_emits_symbol_id() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct Foo;\npub fn bar() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "Foo", "--show-ids"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[id: struct:"),
        "find --show-ids text should embed symbol_id; got: {stdout}"
    );
}

#[test]
fn regression_callers_method_does_not_pull_unrelated_type_methods() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct Foo;\nimpl Foo { pub fn build() -> Self { Self } }\nfn caller_a() { let _ = Vec::<u32>::build(); }\nfn caller_b() { let _ = Foo::build(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "build", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let matches = json["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1, "should resolve Foo::build only");
    let callers = matches[0]["callers"].as_array().unwrap();
    let names: Vec<&str> = callers
        .iter()
        .map(|c| c["info"]["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"caller_b"),
        "Foo::build caller missing: {names:?}"
    );
    assert!(
        !names.contains(&"caller_a"),
        "Vec::build caller should be dropped: {names:?}"
    );
}

#[test]
fn regression_callers_free_fn_accepts_module_path_qualified_calls() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod inner { pub fn target() {} }\nfn caller_local() { inner::target(); }\nfn caller_full() { crate::inner::target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "target", "--json"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let callers = json["matches"][0]["callers"].as_array().unwrap();
    assert_eq!(
        callers.len(),
        2,
        "both qualified call styles should match: {callers:?}"
    );
}

#[test]
fn regression_callers_in_filter_narrows_caller_results() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/keep/a.rs"),
        "fn caller_keep() { crate::target(); }\n",
    );
    write_file(
        &fixture.path().join("src/drop/b.rs"),
        "fn caller_drop() { crate::target(); }\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\nmod keep; mod drop;\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "callers",
        "target",
        "--callers-in",
        "keep",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let callers = json["matches"][0]["callers"].as_array().unwrap();
    let names: Vec<&str> = callers
        .iter()
        .map(|c| c["info"]["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"caller_keep"));
    assert!(!names.contains(&"caller_drop"));
}

#[test]
fn regression_global_threshold_default_is_strict() {
    use clap::Parser;
    use rustgraph::cli::Args;
    let args = Args::try_parse_from(["rustgraph"]).expect("parse");
    assert_eq!(args.search_threshold, 0.85, "default tightened from 0.6");
}

#[test]
fn regression_callers_does_not_signature_match_struct_name() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct DaemonState;\npub fn handle(d: &DaemonState) {}\npub fn process(state: DaemonState) -> DaemonState { state }\nfn caller() { handle(&DaemonState); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "DaemonState", "--json"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stderr.contains("is a struct")
            || stderr.contains("no function matches")
            || stdout.contains("\"matches\": []"),
        "callers should not signature-match a struct name; stderr={stderr} stdout={stdout}"
    );
}

#[test]
fn regression_also_paths_get_relative_form_with_crate_prefix() {
    let primary = tempdir().expect("primary tempdir");
    let sibling = tempdir().expect("sibling tempdir");
    write_file(
        &primary.path().join("src/lib.rs"),
        "pub fn primary_fn() {}\n#[derive(serde::Serialize)] pub struct PrimaryStruct;\n",
    );
    write_file(
        &sibling.path().join("src/lib.rs"),
        "pub fn sibling_fn() {}\n#[derive(serde::Serialize)] pub struct SiblingStruct;\n",
    );
    let p_base = primary.path().to_string_lossy().to_string();
    let s_base = sibling.path().to_string_lossy().to_string();
    let sibling_label = sibling
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = std::process::Command::new(bin)
        .args([
            "--no-auto-path",
            "--path",
            &p_base,
            "--also",
            &s_base,
            "impls",
            "Serialize",
        ])
        .output()
        .expect("run rustgraph");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("PrimaryStruct"),
        "primary missing: {stdout}"
    );
    assert!(
        stdout.contains("SiblingStruct"),
        "sibling missing: {stdout}"
    );

    let sibling_line: &str = stdout
        .lines()
        .find(|l| l.contains("SiblingStruct"))
        .expect("sibling line");
    assert!(
        sibling_line.contains(&format!("{}/", sibling_label)),
        "sibling should use `<label>/` prefix, got: {sibling_line}"
    );
    assert!(
        !sibling_line.contains(&s_base),
        "sibling line should NOT include absolute path; got: {sibling_line}"
    );
}

#[test]
fn regression_match_signature_opt_in_restores_wide_fuzz() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct DaemonState;\npub fn handle(d: &DaemonState) {}\npub fn process(state: DaemonState) {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "DaemonState", "--json"]);
    assert!(out.status.success());
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let fns = json["functions"].as_array().expect("functions array");
    assert!(
        fns.is_empty(),
        "default find should NOT match fns by signature text; got: {} fn(s)",
        fns.len()
    );

    let out_wide = run_rustgraph(&[
        "--path",
        &base,
        "--match-signature",
        "find",
        "DaemonState",
        "--json",
    ]);
    assert!(out_wide.status.success());
    let json_wide: Value = serde_json::from_slice(&out_wide.stdout).expect("valid json");
    let fns_wide = json_wide["functions"].as_array().expect("functions array");
    assert!(
        fns_wide.len() >= 2,
        "--match-signature should match handle + process; got: {} fn(s)",
        fns_wide.len()
    );
}

#[test]
fn regression_bad_path_emits_clear_error_not_silent_empty() {
    let out = run_rustgraph(&["--path", "/this/path/does/not/exist/xyz/abc/123", "tree"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "should exit non-zero on bad --path; got success. stderr={stderr}"
    );
    assert!(
        stderr.contains("does not exist") || stderr.contains("not exist"),
        "expected a clear 'does not exist' error on stderr; got: {stderr}"
    );
}

#[test]
fn regression_find_exact_name_fast_path_suppresses_substring() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn load() {}
pub fn load_jsonl() {}
pub fn load_session() {}
pub fn preload_data() {}
pub fn unrelated() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "load", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let names: Vec<String> = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        vec!["load".to_string()],
        "fast path violated: {names:?}"
    );
}

#[test]
fn regression_callers_ambiguous_bails_unless_all() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn handle_a() {}
pub fn handle_b() {}
pub fn handle_c() {}
pub fn handle_d() {}
pub fn handle_e() {}
pub fn handle_f() {}
pub fn handle_g() {}
pub fn handle_h() {}
pub fn handle_i() {}
fn caller() { handle_a(); handle_b(); handle_c(); handle_d(); handle_e(); handle_f(); handle_g(); handle_h(); handle_i(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "handle"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected non-zero exit; stderr={stderr}"
    );
    assert!(
        stderr.contains("ambiguous"),
        "expected 'ambiguous' in stderr; got: {stderr}"
    );
    assert!(
        stderr.contains("--all"),
        "expected --all hint in stderr; got: {stderr}"
    );

    let out_all = run_rustgraph(&["--path", &base, "callers", "handle", "--all", "--json"]);
    assert!(
        out_all.status.success(),
        "expected --all to bypass; stderr={}",
        String::from_utf8_lossy(&out_all.stderr)
    );
    let json: Value = serde_json::from_slice(&out_all.stdout).expect("valid json");
    assert!(json["matches"].as_array().unwrap().len() >= 9);
}

#[test]
fn regression_ensemble_ambiguous_bails_unless_all() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn handle_a() {}
pub fn handle_b() {}
pub fn handle_c() {}
pub fn handle_d() {}
pub fn handle_e() {}
pub fn handle_f() {}
pub fn handle_g() {}
pub fn handle_h() {}
pub fn handle_i() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "ensemble", "handle", "--preset", "deep"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected non-zero exit; stderr={stderr}"
    );
    assert!(
        stderr.contains("ambiguous"),
        "expected 'ambiguous' in stderr; got: {stderr}"
    );
    let out_all = run_rustgraph(&[
        "--path", &base, "ensemble", "handle", "--preset", "deep", "--all", "--json",
    ]);
    assert!(
        out_all.status.success(),
        "expected --all to bypass; stderr={}",
        String::from_utf8_lossy(&out_all.stderr)
    );
}

#[test]
fn regression_ensemble_resolves_relative_paths_via_root_hint() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn snowflake_target() { let _ = 1 + 1; }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let other_cwd = tempdir().expect("other cwd");
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = std::process::Command::new(bin)
        .args(["--path", &base, "ensemble", "snowflake_target", "--json"])
        .current_dir(other_cwd.path())
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "ensemble must resolve relative paths via ROOT_HINT; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(
        json["matches"][0]["info"]["name"].as_str(),
        Some("snowflake_target")
    );
}

#[test]
fn regression_paths_between_resolves_method_receiver_calls() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct State;
impl State {
    pub fn target_method(&self) {}
}
fn caller_via_method(s: &State) { s.target_method(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "paths-between",
        "caller_via_method",
        "target_method",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let paths = json["paths"].as_array().expect("paths");
    assert!(
        !paths.is_empty(),
        "method-receiver path should resolve; got 0 paths"
    );
    let first = paths[0].as_array().unwrap();
    assert_eq!(
        first.last().unwrap()["name"].as_str(),
        Some("target_method")
    );
}

#[test]
fn regression_callers_callers_in_recomputes_total() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/keep/a.rs"),
        "pub fn target() {}\nfn caller_keep() { target(); target(); }\n",
    );
    write_file(
        &fixture.path().join("src/drop/b.rs"),
        "pub fn target() {}\nfn caller_drop() { target(); target(); target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "callers",
        "target",
        "--callers-in",
        "src/keep",
        "--all",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");

    let total: u64 = json["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["call_sites_total"].as_u64().unwrap_or(0))
        .sum();
    assert_eq!(
        total, 2,
        "post-filter total must reflect only kept callers; got {total}"
    );
}

#[test]
fn regression_signatures_normalized_no_token_spam() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "use std::collections::HashMap;\n\
         pub fn snowflake(arg: Option<&str>, m: HashMap<String, Vec<u32>>) -> Result<(), String> { Ok(()) }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "snowflake", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let sig = json["functions"][0]["signature"].as_str().unwrap();
    assert!(!sig.contains(" < "), "ungly ` < ` leaked: {sig}");
    assert!(!sig.contains(" > "), "ungly ` > ` leaked: {sig}");
    assert!(!sig.contains("& str"), "ungly `& str` leaked: {sig}");
    assert!(
        sig.contains("Option<&str>") || sig.contains("Option<& str>"),
        "expected collapsed Option<&str>: {sig}"
    );
}

#[test]
fn regression_tree_also_header_lists_sibling_crates() {
    let primary = tempdir().expect("primary");
    let sibling = tempdir().expect("sibling");
    write_file(&primary.path().join("src/lib.rs"), "pub fn p() {}\n");
    write_file(&sibling.path().join("src/lib.rs"), "pub fn s() {}\n");
    let base = primary.path().to_string_lossy().to_string();
    let also = sibling.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--also", &also, "tree"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("sibling crate"),
        "header must mention sibling crates; got: {}",
        stdout.lines().next().unwrap_or("")
    );
}

#[test]
fn regression_paths_between_dedupes_multi_callsite_paths() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\nfn middle() { target(); target(); target(); }\nfn entry() { middle(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "paths-between",
        "entry",
        "target",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let paths = json["paths"].as_array().expect("paths array");

    assert_eq!(
        paths.len(),
        1,
        "expected 1 unique path (entry→middle→target); got {} copies",
        paths.len()
    );
}

#[test]
fn regression_call_graph_text_hides_external_by_default() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn root_fn() { let v = vec![1, 2]; v.iter().map(|x| x + 1).collect::<Vec<_>>(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out_default = run_rustgraph(&["--path", &base, "call-graph", "--root", "root_fn"]);
    let stdout = String::from_utf8_lossy(&out_default.stdout);
    assert!(
        !stdout.contains("<external>"),
        "default should hide externals; got: {stdout}"
    );
    let out_show = run_rustgraph(&[
        "--path",
        &base,
        "call-graph",
        "--root",
        "root_fn",
        "--show-external",
    ]);
    let stdout_show = String::from_utf8_lossy(&out_show.stdout);
    assert!(
        stdout_show.contains("<external>"),
        "--show-external should expose them; got: {stdout_show}"
    );
}

#[test]
fn regression_call_graph_text_no_cycle_spam() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct A; impl A { pub fn new() -> Self { Self } }\n\
         pub struct B; impl B { pub fn new() -> Self { Self } }\n\
         pub fn root_fn() { A::new(); B::new(); A::new(); B::new(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "call-graph", "--root", "root_fn"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let cycle_count = stdout.matches("[cycle]").count();
    assert_eq!(
        cycle_count, 0,
        "no [cycle] lines should leak through; got {cycle_count}"
    );
}

#[test]
fn regression_refs_catches_enum_variant_qualified_uses() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub enum Color { Red, Green, Blue }
pub fn paint() {
    let c = Color::Red;
    let _ = match c { Color::Green => 1, Color::Blue => 2, _ => 0 };
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "refs", "Color", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let total = json["total_refs"].as_u64().unwrap_or(0);
    let kinds: Vec<String> = json["refs"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|r| r["kind"].as_str().map(String::from))
        .collect();
    let variant_count = kinds.iter().filter(|k| k.as_str() == "variant").count();
    assert!(
        variant_count >= 3,
        "expected 3+ variant refs (Color::Red, Color::Green, Color::Blue); got {variant_count} (total: {total}, kinds: {kinds:?})"
    );
}

#[test]
fn regression_usages_emits_no_callers_stub_for_non_function() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct MyStruct { pub field: u32 }\nfn use_it() { let _ = MyStruct { field: 1 }; }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "usages", "MyStruct", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let labels: Vec<String> = json["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["label"].as_str().unwrap().to_string())
        .collect();
    assert!(
        labels.iter().any(|l| l.contains("callers")),
        "callers section must be present (with skip note); got labels: {labels:?}"
    );
}

#[test]
fn regression_grep_json_uses_file_path_start_line_keys() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() { let _ = 1; }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "grep", "fn ", "-j"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let m = &json["matches"][0];
    assert!(
        m["file_path"].is_string(),
        "missing file_path key; got: {m}"
    );
    assert!(
        m["start_line"].is_number(),
        "missing start_line key; got: {m}"
    );
    assert!(m["path"].is_null(), "old `path` key must be gone; got: {m}");
    assert!(m["line"].is_null(), "old `line` key must be gone; got: {m}");
}

#[test]
fn regression_grep_json_truncation_exits_nonzero() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "grep", "fn ", "-j", "--max-results", "2"]);
    assert!(
        !out.status.success(),
        "must exit non-zero on truncated -j; got success"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("truncated"),
        "must mention truncation; got: {stderr}"
    );
}

#[test]
fn regression_find_exclude_tests_drops_test_functions() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target_prod() {}
#[cfg(test)]
mod tests {
    #[test]
    fn target_test() {}
    fn target_helper() {}
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "find",
        "target",
        "--exclude-tests",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let names: Vec<String> = json["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        names.contains(&"target_prod".to_string()),
        "prod fn dropped: {names:?}"
    );
    assert!(
        !names.contains(&"target_test".to_string()),
        "#[test] fn leaked: {names:?}"
    );
    assert!(
        !names.contains(&"target_helper".to_string()),
        "cfg(test) helper leaked: {names:?}"
    );
}

#[test]
fn regression_paths_between_resolves_qualified_call_chain() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/main.rs"),
        "pub fn main() { mycrate::deep::target_fn(); }\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target_fn() { let _ = 1; }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "paths-between",
        "main",
        "target_fn",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let paths = json["paths"].as_array().expect("paths array");
    assert!(!paths.is_empty(), "expected at least one path; got: {json}");
    let first = paths[0].as_array().expect("first path");
    assert_eq!(first[0]["name"].as_str(), Some("main"));
    assert_eq!(first.last().unwrap()["name"].as_str(), Some("target_fn"));
}

#[test]
fn regression_usages_combines_callers_and_refs() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() { let _ = 1; }\nfn caller() { target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "usages", "target", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let sections = json["sections"].as_array().expect("sections");
    let labels: Vec<&str> = sections
        .iter()
        .map(|s| s["label"].as_str().unwrap())
        .collect();
    assert!(
        labels.contains(&"callers"),
        "missing callers; got: {labels:?}"
    );

    assert!(
        labels.iter().any(|l| l.starts_with("refs")),
        "missing refs section; got: {labels:?}"
    );
    assert!(json["total"].as_u64().unwrap() >= 1);
}

#[test]
fn regression_find_no_match_emits_did_you_mean() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn loader() {}\npub fn loaderr() {}\npub fn unrelated() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "find", "loadirx"]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    let has_hint = stderr.contains("did you mean") || stderr.contains("relaxed to 0.7");
    assert!(has_hint, "missing hint: {stderr}");
}

#[test]
fn regression_call_graph_text_mode_renders_tree() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn root_fn() { mid_fn(); }\npub fn mid_fn() { leaf_fn(); }\npub fn leaf_fn() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "call-graph", "--root", "root_fn"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("root_fn"), "missing root: {stdout}");
    assert!(stdout.contains("mid_fn"), "missing mid: {stdout}");

    assert!(stdout.contains("──"), "no tree connectors: {stdout}");
    assert!(!stdout.contains("digraph"), "should not be DOT: {stdout}");
}

#[test]
fn regression_callers_in_alias_dropped_in_favor_of_target_in() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/keep/a.rs"),
        "pub fn helper() {}\nfn caller_keep() { helper(); }\n",
    );
    write_file(
        &fixture.path().join("src/drop/b.rs"),
        "pub fn helper() {}\nfn caller_drop() { helper(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let bad = run_rustgraph(&["--path", &base, "callers", "helper", "--in", "keep"]);
    assert!(
        !bad.status.success(),
        "expected --in to no longer work on callers"
    );

    let good = run_rustgraph(&[
        "--path",
        &base,
        "callers",
        "helper",
        "--target-in",
        "keep",
        "--json",
    ]);
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    let json: Value = serde_json::from_slice(&good.stdout).expect("valid json");
    let m = json["matches"].as_array().unwrap();
    assert_eq!(m.len(), 1, "got: {}", json);
    assert!(m[0]["info"]["file_path"].as_str().unwrap().contains("keep"));
}

#[test]
fn regression_grep_truncation_notice_on_stderr() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "grep", "fn ", "--max-results", "2"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("showing") && stderr.contains("--max-results"),
        "missing truncation notice in unified `(showing N of M; use --max-results to expand)` form: {stderr}"
    );
}

#[test]
fn regression_callers_three_matches_pass_through() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn handle_a() {}
pub fn handle_b() {}
pub fn handle_c() {}
fn caller() { handle_a(); handle_b(); handle_c(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "handle", "--json"]);
    assert!(
        out.status.success(),
        "3 matches must NOT bail; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn regression_subcommand_short_help_hides_globals() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = Command::new(bin)
        .args(["callers", "-h"])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("--path"),
        "callers -h should NOT show --path (global), got:\n{stdout}"
    );
    assert!(
        !stdout.contains("--search-threshold"),
        "callers -h should NOT show --search-threshold (global), got:\n{stdout}"
    );
    assert!(
        stdout.contains("--target-in") || stdout.contains("--symbol-id"),
        "callers -h SHOULD show its own flags, got:\n{stdout}"
    );
}

#[test]
fn regression_subcommand_long_help_shows_globals() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = Command::new(bin)
        .args(["callers", "--help"])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("--path"),
        "callers --help (long) SHOULD show --path, got:\n{stdout}"
    );
    assert!(
        stdout.contains("--search-threshold"),
        "callers --help SHOULD show --search-threshold, got:\n{stdout}"
    );
}

#[test]
fn regression_usage_error_dumps_contextual_help() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");

    let out = Command::new(bin)
        .args(["callers", "foo", "--definitely-not-a-real-flag"])
        .output()
        .expect("run");
    assert!(!out.status.success(), "should exit non-zero");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Usage:") && stderr.contains("callers"),
        "expected callers usage dumped to stderr, got:\n{stderr}"
    );
}

#[test]
fn regression_refs_kind_help_lists_variant() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = Command::new(bin)
        .args(["refs", "--help"])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("variant"),
        "refs --help should list 'variant' kind, got:\n{stdout}"
    );
}

#[test]
fn regression_usages_skips_call_kind_refs_when_target_is_fn() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() { let _ = 1; }\nfn caller() { target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "usages", "target", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let sections = json["sections"].as_array().expect("sections");
    let refs_section = sections
        .iter()
        .find(|s| s["label"].as_str().unwrap_or("").starts_with("refs"))
        .expect("refs section");
    let entries = refs_section["entries"].as_array().expect("entries");
    assert!(
        entries.iter().all(|e| e["kind"].as_str() != Some("call")),
        "refs section should drop kind=call when target is a fn, got: {entries:?}"
    );
}

#[test]
fn regression_callers_symbol_id_without_positional_query() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\nfn caller() { target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let find_out = run_rustgraph(&["--path", &base, "find", "target", "--func", "--json"]);
    let find_json: Value = serde_json::from_slice(&find_out.stdout).expect("find json");
    let symbol_id = find_json["functions"][0]["symbol_id"]
        .as_str()
        .expect("symbol_id from find")
        .to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "callers",
        "--symbol-id",
        &symbol_id,
        "--json",
    ]);
    assert!(
        out.status.success(),
        "callers --symbol-id (no positional) must succeed, got stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn regression_paths_between_in_filter_scopes_resolution() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/keep/a.rs"),
        "pub fn entry() { target(); }\npub fn target() {}\n",
    );
    write_file(
        &fixture.path().join("src/drop/b.rs"),
        "pub fn entry() { target(); }\npub fn target() {}\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod keep { pub mod a; }\npub mod drop { pub mod b; }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "paths-between",
        "entry",
        "target",
        "--target-in",
        "keep",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let paths = json["paths"].as_array().expect("paths array");
    assert!(!paths.is_empty(), "expected at least one path, got: {json}");
    for path in paths {
        for node in path.as_array().unwrap() {
            assert!(
                node["file_path"].as_str().unwrap().contains("keep"),
                "all path nodes should be in keep/, got: {node}"
            );
        }
    }
}

#[test]
fn regression_paths_between_show_call_sites_annotates_hops() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn entry() {\n    let _ = 1;\n    middle();\n}\npub fn middle() { target(); }\npub fn target() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "paths-between",
        "entry",
        "target",
        "--show-call-sites",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let paths = json["paths"].as_array().expect("paths array");
    assert!(!paths.is_empty());
    let first_path = paths[0].as_array().expect("first path");

    let entry_node = &first_path[0];
    assert!(
        entry_node["call_site_line"].as_u64().is_some(),
        "entry node should have call_site_line set when --show-call-sites; got: {entry_node}"
    );
}

#[test]
fn regression_find_does_not_double_fn_name() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn build_catalog(arg: u32) -> u32 { arg }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "find", "build_catalog", "--func"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("fn build_catalog fn build_catalog"),
        "fn name should not appear twice; got:\n{stdout}"
    );
    assert!(
        stdout.contains("build_catalog"),
        "expected the function name to render, got:\n{stdout}"
    );
}

#[test]
fn regression_dead_code_in_filter_recomputes_summary() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/keep/mod.rs"),
        "pub fn dead_in_keep() {}\n",
    );
    write_file(
        &fixture.path().join("src/drop/mod.rs"),
        "pub fn dead_in_drop() {}\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod keep; pub mod drop;\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "dead-code", "--in", "keep", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let dead_total = json["dead_functions_total"].as_u64().unwrap_or(0);
    let dead_arr_len = json["dead_functions"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(
        dead_total as usize, dead_arr_len,
        "dead_functions_total ({dead_total}) must match the filtered dead_functions length ({dead_arr_len})"
    );
}

#[test]
fn regression_refs_dedups_call_and_path_at_same_span() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\nfn caller() { target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "refs", "target", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let refs = json["refs"].as_array().expect("refs array");

    use std::collections::HashSet;
    let mut seen: HashSet<(String, u64)> = HashSet::new();
    for r in refs {
        let key = (
            r["file_path"].as_str().unwrap_or("").to_string(),
            r["line"].as_u64().unwrap_or(0),
        );
        assert!(
            seen.insert(key.clone()),
            "duplicate (file, line) {key:?} in refs output"
        );
    }

    let call_entry = refs.iter().find(|r| r["line"].as_u64() == Some(2));
    if let Some(entry) = call_entry {
        assert_eq!(entry["kind"].as_str(), Some("call"));
    }
}

#[test]
fn regression_usages_dedups_method_kind_against_callers() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn target() {}\nfn caller() { target(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "usages", "target", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let sections = json["sections"].as_array().expect("sections");
    let callers_lines: Vec<(String, u64)> = sections
        .iter()
        .find(|s| s["label"].as_str() == Some("callers"))
        .map(|s| {
            s["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    (
                        e["file_path"].as_str().unwrap().to_string(),
                        e["line"].as_u64().unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let refs_section = sections
        .iter()
        .find(|s| {
            s["label"]
                .as_str()
                .map(|l| l.starts_with("refs"))
                .unwrap_or(false)
        })
        .expect("refs section");
    for entry in refs_section["entries"].as_array().unwrap() {
        let key = (
            entry["file_path"].as_str().unwrap().to_string(),
            entry["line"].as_u64().unwrap(),
        );
        assert!(
            !callers_lines.contains(&key),
            "refs section must not duplicate callers (file, line) pair {key:?}"
        );
    }
}

#[test]
fn regression_search_threshold_filters_substring_matches() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn Daemon() {}\npub fn daemon_v1_sessions_returns_json_list() {}\npub fn build_daemon_handler() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "find",
        "Daemon",
        "--func",
        "--search-threshold",
        "0.99",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let funcs = json["functions"].as_array().unwrap();

    assert_eq!(
        funcs.len(),
        1,
        "expected only exact match at 0.99, got: {funcs:?}"
    );
    assert_eq!(funcs[0]["name"].as_str(), Some("Daemon"));
}

#[test]
fn regression_callers_resolves_through_pub_use_alias() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/inner.rs"),
        "pub fn handle_get() {}\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod inner;\npub use inner::handle_get as handle_session_get;\nfn router() { handle_session_get(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "handle_get", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let matches = json["matches"].as_array().expect("matches");
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one match for handle_get"
    );
    let callers = matches[0]["callers"].as_array().expect("callers");
    assert!(
        callers
            .iter()
            .any(|c| c["info"]["name"].as_str() == Some("router")),
        "expected `router` in callers via alias resolution; got: {callers:?}"
    );
}

#[test]
fn regression_paths_between_walks_framework_handler_refs() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/handler.rs"),
        "pub fn target_handler() {}\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod handler;
pub fn build_router() {
    let _ = axum::routing::get(handler::target_handler);
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "paths-between",
        "build_router",
        "target_handler",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let paths = json["paths"].as_array().expect("paths");
    assert!(
        !paths.is_empty(),
        "expected paths-between to walk through axum::get(handler::...) ref; got: {json}"
    );
}

#[test]
fn regression_callers_on_struct_redirects_to_refs() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct DaemonState { pub id: u32 }\npub fn unrelated_fn() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "callers", "DaemonState"]);
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("is a struct") && stderr.contains("rustgraph refs DaemonState"),
        "expected struct redirect; got: {stderr}"
    );
}

#[test]
fn regression_ensemble_on_enum_redirects_to_refs() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub enum Color { Red, Blue }\npub fn unrelated() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "ensemble", "Color"]);
    assert!(!out.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("is an enum") && stderr.contains("rustgraph refs Color"),
        "expected enum redirect; got: {stderr}"
    );
}

#[test]
fn regression_callers_exclude_tests_drops_test_callers() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target() {}
pub fn prod_caller() { target(); }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_caller() { target(); }
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--exclude-tests",
        "callers",
        "target",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let callers: Vec<&str> = json["matches"][0]["callers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["info"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        callers,
        vec!["prod_caller"],
        "test_caller leaked: {callers:?}"
    );
}

#[test]
fn regression_usages_exclude_tests_drops_test_callers_and_refs() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target() {}
pub fn prod_caller() { target(); }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_caller() { target(); }
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--exclude-tests",
        "usages",
        "target",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    let sections = json["sections"].as_array().expect("sections");
    let mut fnames: Vec<String> = Vec::new();
    for s in sections {
        for entry in s["entries"].as_array().unwrap_or(&vec![]) {
            if let Some(name) = entry["enclosing"].as_str() {
                fnames.push(name.to_string());
            }
        }
    }
    assert!(
        !fnames.iter().any(|n| n == "test_caller"),
        "test fn leaked: {fnames:?}"
    );
}

#[test]
fn regression_callers_ambiguity_cap_configurable() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn handle_a() {}
pub fn handle_b() {}
pub fn handle_c() {}
pub fn handle_d() {}
pub fn handle_e() {}
pub fn handle_f() {}
pub fn handle_g() {}
pub fn handle_h() {}
pub fn handle_i() {}
pub fn caller() { handle_a(); handle_b(); handle_c(); handle_d(); handle_e(); handle_f(); handle_g(); handle_h(); handle_i(); }
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let bail = run_rustgraph(&["--path", &base, "callers", "handle"]);
    assert!(
        !bail.status.success(),
        "expected default cap=7 to bail on 9 matches"
    );

    let pass = run_rustgraph(&["--path", &base, "callers", "handle", "--ambiguity-cap", "9"]);
    assert!(
        pass.status.success(),
        "cap=9 should let 9 matches pass: {}",
        String::from_utf8_lossy(&pass.stderr)
    );
}

#[test]
fn regression_call_graph_positional_acts_as_root() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn entry() { mid(); }\npub fn mid() { leaf(); }\npub fn leaf() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "call-graph", "entry"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("entry") && stdout.contains("mid"),
        "tree missing nodes: {stdout}"
    );
}

#[test]
fn regression_grep_by_function_json_omits_matches_array() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn alpha() { let _ = 1.unwrap(); }\npub fn beta() { let _ = 2.unwrap(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "grep",
        r"\.unwrap\(\)",
        "--by-function",
        "-j",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert!(
        json.get("matches").is_none(),
        "matches[] should be omitted in --by-function: {json}"
    );
    assert!(
        json.get("summary_by_function").is_some(),
        "summary_by_function missing"
    );
    assert_eq!(json["by_function"], serde_json::json!(true));
}

#[test]
fn regression_refs_in_filter_reports_scoped_file_count() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/keep/a.rs"),
        "pub fn target() {}\nfn caller() { target(); }\n",
    );
    write_file(
        &fixture.path().join("src/drop/b.rs"),
        "pub fn unrelated() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "refs", "target", "--in", "keep"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("across 1 file(s)"),
        "expected scoped count of 1; got: {stdout}"
    );
}

#[test]
fn regression_tree_prefix_strips_parent_levels() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/inner/a.rs"), "pub fn one() {}\n");
    write_file(&fixture.path().join("src/inner/b.rs"), "pub fn two() {}\n");
    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "tree", "src/inner"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stdout.contains("a.rs"), "missing a.rs: {stdout}");
    assert!(stdout.contains("b.rs"), "missing b.rs: {stdout}");
    let body_lines: Vec<&str> = stdout.lines().skip(1).collect();
    let body = body_lines.join("\n");
    assert!(
        !body.contains("src") && !body.contains("inner"),
        "parent levels leaked into tree body: {body}"
    );
}

#[test]
fn regression_paths_between_hints_reverse_when_forward_empty() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn a() { b(); }\npub fn b() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "paths-between", "b", "a"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("reverse direction has path") && stderr.contains("paths-between a b"),
        "missing reverse hint: {stderr}"
    );
}

#[test]
fn regression_def_exact_match_returns_single_line() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub fn handle() {}\npub fn handle_session() {}\npub fn handle_request() {}\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "def", "handle"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("handle [fn]"),
        "missing single-line def: {stdout}"
    );

    let nope = run_rustgraph(&["--path", &base, "def", "handl"]);
    assert!(
        !nope.status.success(),
        "expected no exact match for 'handl'"
    );
}

#[test]
fn regression_version_flag_works() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustgraph"))
        .args(["--version"])
        .output()
        .expect("run");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("rustgraph "), "version output: {stdout}");
}
