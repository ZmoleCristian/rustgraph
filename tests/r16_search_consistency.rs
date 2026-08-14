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

fn fn_names_from_json(stdout: &[u8]) -> Vec<String> {
    let v: Value = serde_json::from_slice(stdout).expect("valid json");
    let mut names: Vec<String> = v["functions"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|f| f["name"].as_str().map(|s| s.to_string()))
        .collect();
    names.sort();
    names.dedup();
    names
}

#[test]
fn regression_find_and_global_search_return_consistent_results() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn load() {}
pub fn load_jsonl() {}
pub fn load_pins() {}
pub fn preload() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let find_out = run_rustgraph(&["find", "load", "-p", &base, "-j"]);
    assert!(
        find_out.status.success(),
        "find failed: stderr={}",
        String::from_utf8_lossy(&find_out.stderr)
    );

    let search_out = run_rustgraph(&["--path", &base, "--search", "load", "-j"]);
    assert!(
        search_out.status.success(),
        "global --search failed: stderr={}",
        String::from_utf8_lossy(&search_out.stderr)
    );

    let find_names = fn_names_from_json(&find_out.stdout);
    let search_names = fn_names_from_json(&search_out.stdout);

    assert_eq!(
        find_names, search_names,
        "find and global --search disagree for query 'load':\n  \
         find    -> {find_names:?}\n  \
         search  -> {search_names:?}\n\
         Both flows should run through the same match_kind / R15 fast-path."
    );
}

#[test]
fn regression_global_search_honors_short_query_precision() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn new() {}
pub fn newest_jsonl() {}
pub fn new_session() {}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "--search", "new", "-j"]);
    assert!(
        out.status.success(),
        "global --search failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let names = fn_names_from_json(&out.stdout);
    assert_eq!(
        names,
        vec!["new".to_string()],
        "global --search did not honor R15 short-query precision: got {names:?}; \
         expected only [\"new\"] (exact-only fast path for queries <=3 chars)."
    );
}

#[test]
fn regression_callers_depth_2_with_context_either_honors_or_warns() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn c() {
    // marker_inside_c
}
pub fn b() {
    // marker_above_call_in_b
    c();
    // marker_below_call_in_b
}
pub fn a() {
    // marker_above_call_in_a
    b();
    // marker_below_call_in_a
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["callers", "c", "--depth", "2", "-C", "2", "-p", &base]);
    assert!(
        out.status.success(),
        "callers failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    let context_honored = stdout.contains("marker_above_call_in_b")
        || stdout.contains("marker_below_call_in_b")
        || stdout.contains("marker_above_call_in_a")
        || stdout.contains("marker_below_call_in_a")
        || stdout.contains("c();")
        || stdout.contains("b();");

    let stderr_lc = stderr.to_lowercase();
    let warned = (stderr_lc.contains("ignored")
        || stderr_lc.contains("ignore")
        || stderr_lc.contains("not supported")
        || stderr_lc.contains("unsupported"))
        && (stderr_lc.contains("-c")
            || stderr_lc.contains("context")
            || stderr_lc.contains("depth"));

    assert!(
        context_honored || warned,
        "callers --depth 2 -C 2 silently dropped -C: neither context lines \
         appeared in stdout nor a warning fired in stderr.\n\
         stdout=\n{stdout}\n\
         stderr=\n{stderr}"
    );
}

#[test]
fn regression_callers_depth_2_json_documents_context_or_omits() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn c() {
    // marker_inside_c
}
pub fn b() {
    // marker_above_call_in_b
    c();
    // marker_below_call_in_b
}
pub fn a() {
    // marker_above_call_in_a
    b();
    // marker_below_call_in_a
}
"#,
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["callers", "c", "--depth", "2", "-C", "2", "-p", &base, "-j"]);
    assert!(
        out.status.success(),
        "callers -j failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let v: Value = serde_json::from_str(&stdout).expect("valid json");

    fn has_context_field(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                if map.contains_key("context") || map.contains_key("context_lines") {
                    return true;
                }
                map.values().any(has_context_field)
            }
            Value::Array(arr) => arr.iter().any(has_context_field),
            _ => false,
        }
    }

    assert!(
        has_context_field(&v),
        "callers --depth 2 -C 2 -j returned no `context`/`context_lines` field anywhere; \
         the -C flag was silently dropped in transitive JSON output. \
         JSON:\n{}",
        serde_json::to_string_pretty(&v).unwrap_or_default()
    );
}
