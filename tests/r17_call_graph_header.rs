

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


fn parse_edge_count(stdout: &str) -> Option<usize> {
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("rustgraph call-graph --text") {
            continue;
        }

        let after_open = trimmed.split_once('[')?.1;
        let after_comma = after_open.split_once(", ")?.1;
        let count_str = after_comma.split_whitespace().next()?;
        return count_str.parse::<usize>().ok();
    }
    None
}


fn count_rendered_children(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|l| l.contains("├──") || l.contains("└──"))
        .count()
}

const LEAF_FIXTURE: &str = "pub fn leaf_fn() {\n\
    let v: Vec<i32> = (0..10).collect();\n\
    let _: i32 = v.iter().sum();\n\
    println!(\"{}\", v.len());\n\
}\n";


#[test]
fn regression_call_graph_header_count_matches_rendered_internal() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), LEAF_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph", "--root", "leaf_fn"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    let header_count = parse_edge_count(&stdout).unwrap_or_else(|| {
        panic!(
            "could not parse rustgraph header from stdout: {stdout}\n----stderr----\n{stderr}"
        )
    });
    let rendered_children = count_rendered_children(&stdout);

    assert_eq!(
        header_count, rendered_children,
        "header edge-count ({}) should match rendered tree-children ({}); \
         old bug printed e.g. 19 edges over an empty tree.\nstdout:\n{}\nstderr:\n{}",
        header_count, rendered_children, stdout, stderr
    );

    assert_eq!(
        header_count, 0,
        "leaf_fn only calls std/iter chains; expected 0 internal edges, got {} (stdout: {})",
        header_count, stdout
    );
}


#[test]
fn regression_call_graph_header_hints_at_all_external() {
    let fixture = tempdir().expect("tempdir");
    write_file(&fixture.path().join("src/lib.rs"), LEAF_FIXTURE);
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "call-graph", "--root", "leaf_fn"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}\n{stderr}");

    let lower = combined.to_lowercase();
    assert!(
        lower.contains("all external") || lower.contains("show-external"),
        "expected 'all external' or 'show-external' hint when leaf has only external callees; \
         got combined output:\n{combined}"
    );
}
