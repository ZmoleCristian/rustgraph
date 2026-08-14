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

const CHAIN_FIXTURE: &str = "pub fn a() { b(); }\n\
                             pub fn b() { c(); }\n\
                             pub fn c() {}\n";

const EXTERNAL_FIXTURE: &str = "pub fn a() { b(); let _ = format!(\"x\"); }\n\
                                pub fn b() {}\n";

#[test]
fn regression_call_graph_json_emits_valid_json() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), CHAIN_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph", "--root", "a", "-j"]);
    assert!(
        out.status.success(),
        "call-graph -j should exit 0; got status={:?}, stderr=\n{}",
        out.status.code(),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout did not parse as JSON ({}); stdout was:\n{}\nstderr:\n{}",
            e,
            stdout,
            stderr_of(&out)
        )
    });
    assert!(
        parsed.is_object(),
        "expected top-level JSON object; got {:?}",
        parsed
    );
}

#[test]
fn regression_call_graph_json_includes_kept_functions() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), CHAIN_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph", "--root", "a", "-j"]);
    assert!(out.status.success(), "exit failed: {}", stderr_of(&out));
    let parsed: Value = serde_json::from_str(&stdout_of(&out)).expect("valid JSON");
    let functions = parsed
        .get("functions")
        .and_then(|v| v.as_array())
        .expect("functions[] array present");

    let names: Vec<&str> = functions
        .iter()
        .filter_map(|f| f.get("name").and_then(|n| n.as_str()))
        .collect();
    for expected in ["a", "b", "c"] {
        assert!(
            names.contains(&expected),
            "expected `{}` in functions[].name; got names={:?}\nfull JSON:\n{}",
            expected,
            names,
            stdout_of(&out)
        );
    }
}

#[test]
fn regression_call_graph_json_includes_internal_edges() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), CHAIN_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph", "--root", "a", "-j"]);
    assert!(out.status.success(), "exit failed: {}", stderr_of(&out));
    let parsed: Value = serde_json::from_str(&stdout_of(&out)).expect("valid JSON");
    let functions = parsed
        .get("functions")
        .and_then(|v| v.as_array())
        .expect("functions[]");
    let edges = parsed
        .get("edges")
        .and_then(|v| v.as_array())
        .expect("edges[]");

    let id_of = |name: &str| -> Option<String> {
        for f in functions {
            if f.get("name").and_then(|n| n.as_str()) == Some(name) {
                return f.get("id").and_then(|i| i.as_str()).map(|s| s.to_string());
            }
        }
        None
    };
    let id_a = id_of("a").expect("a in functions[]");
    let id_b = id_of("b").expect("b in functions[]");
    let id_c = id_of("c").expect("c in functions[]");

    let has_edge = |caller: &str, callee: &str| -> bool {
        edges.iter().any(|e| {
            e.get("caller_id").and_then(|c| c.as_str()) == Some(caller)
                && e.get("callee_id").and_then(|c| c.as_str()) == Some(callee)
                && e.get("callee_kind").and_then(|k| k.as_str()) == Some("internal")
        })
    };

    assert!(
        has_edge(&id_a, &id_b),
        "expected internal edge a→b; got edges={:#?}",
        edges
    );
    assert!(
        has_edge(&id_b, &id_c),
        "expected internal edge b→c; got edges={:#?}",
        edges
    );
}

#[test]
fn regression_call_graph_json_omits_external_edges_by_default() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), EXTERNAL_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph", "--root", "a", "-j"]);
    assert!(out.status.success(), "exit failed: {}", stderr_of(&out));
    let parsed: Value = serde_json::from_str(&stdout_of(&out)).expect("valid JSON");
    let edges = parsed
        .get("edges")
        .and_then(|v| v.as_array())
        .expect("edges[]");

    for edge in edges {
        let name = edge
            .get("callee_name")
            .and_then(|n| n.as_str())
            .unwrap_or("");
        assert!(
            !name.contains("format"),
            "expected NO format-related external edge by default; found edge={:#?}\nfull edges:\n{:#?}",
            edge,
            edges
        );
        let kind = edge
            .get("callee_kind")
            .and_then(|k| k.as_str())
            .unwrap_or("");
        assert_ne!(
            kind, "external",
            "expected NO external-kind edges by default; found edge={:#?}",
            edge
        );
    }
}

#[test]
fn regression_call_graph_json_includes_external_with_show_external() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), EXTERNAL_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "call-graph",
        "--root",
        "a",
        "-j",
        "--show-external",
    ]);
    assert!(out.status.success(), "exit failed: {}", stderr_of(&out));
    let parsed: Value = serde_json::from_str(&stdout_of(&out)).expect("valid JSON");
    let edges = parsed
        .get("edges")
        .and_then(|v| v.as_array())
        .expect("edges[]");

    let has_external_null_id = edges.iter().any(|e| {
        let kind = e.get("callee_kind").and_then(|k| k.as_str()) == Some("external");
        let id_is_null = matches!(e.get("callee_id"), Some(v) if v.is_null());
        kind && id_is_null
    });
    assert!(
        has_external_null_id,
        "expected at least one edge with callee_kind=external and callee_id=null under \
         --show-external; got edges:\n{:#?}",
        edges
    );
}

#[test]
fn regression_call_graph_json_scope_metadata() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), CHAIN_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "call-graph",
        "--root",
        "a",
        "--depth",
        "2",
        "-j",
    ]);
    assert!(out.status.success(), "exit failed: {}", stderr_of(&out));
    let parsed: Value = serde_json::from_str(&stdout_of(&out)).expect("valid JSON");
    let scope = parsed.get("scope").expect("scope object present");

    assert_eq!(
        scope.get("root").and_then(|v| v.as_str()),
        Some("a"),
        "expected scope.root == 'a'; full scope: {:#?}",
        scope
    );
    assert_eq!(
        scope.get("depth").and_then(|v| v.as_i64()),
        Some(2),
        "expected scope.depth == 2; full scope: {:#?}",
        scope
    );
    let kept = scope
        .get("kept_function_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        kept >= 2,
        "expected scope.kept_function_count >= 2; got {} (full scope: {:#?})",
        kept,
        scope
    );
}

#[test]
fn regression_call_graph_json_writes_to_output_file() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), CHAIN_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out_dir = tempdir().expect("tempdir for output");
    let out_file = out_dir.path().join("call_graph.json");
    let out_path = out_file.to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "-p",
        &base,
        "call-graph",
        "--root",
        "a",
        "-j",
        "-o",
        &out_path,
    ]);
    assert!(out.status.success(), "exit failed: {}", stderr_of(&out));

    assert!(
        out_file.exists(),
        "expected JSON file at {}; stderr was:\n{}",
        out_file.display(),
        stderr_of(&out)
    );
    assert!(
        stdout_of(&out).trim().is_empty(),
        "expected EMPTY stdout when -o is set; got:\n{}",
        stdout_of(&out)
    );
    let body = fs::read_to_string(&out_file).expect("read output file");
    let parsed: Value = serde_json::from_str(&body).unwrap_or_else(|e| {
        panic!(
            "output file content did not parse as JSON ({}); body:\n{}",
            e, body
        )
    });
    assert!(
        parsed.get("functions").is_some(),
        "expected `functions` key in file body: {:#?}",
        parsed
    );
}

#[test]
fn regression_call_graph_dot_still_works_without_json() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), CHAIN_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph", "--root", "a", "--dot"]);
    assert!(
        out.status.success(),
        "exit failed: status={:?}, stderr={}",
        out.status.code(),
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.trim_start().starts_with("digraph"),
        "expected DOT output starting with `digraph`; got:\n{}",
        stdout
    );
}
