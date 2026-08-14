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

#[test]
fn regression_call_graph_no_phantom_type_method_edges() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct A { pub x: u32 }\n\
         impl A { pub fn new() -> Self { A { x: 0 } } }\n\
         pub struct B { pub y: u32 }\n\
         impl B { pub fn new() -> Self { B { y: 0 } } }\n\
         pub fn root() { let _ = A::new(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "call-graph", "--root", "root"]);
    assert!(
        out.status.success(),
        "call-graph exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("new") && stdout.contains("src/lib.rs:2"),
        "expected A::new (src/lib.rs:2) in rendered tree.\nstdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("src/lib.rs:4"),
        "B::new (src/lib.rs:4) should NOT appear; phantom edge bug must be gone.\nstdout:\n{stdout}"
    );
}

#[test]
fn regression_call_graph_no_phantom_method_call_edges() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct Foo { v: u32 }\n\
         impl Foo { pub fn compute(x: u32) -> u32 { x + 1 } }\n\
         pub struct Bar { v: u32 }\n\
         impl Bar { pub fn compute(x: u32) -> u32 { x + 2 } }\n\
         pub fn driver() { let _ = Foo::compute(5); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "call-graph", "--root", "driver"]);
    assert!(
        out.status.success(),
        "call-graph exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("compute") && stdout.contains("src/lib.rs:2"),
        "expected Foo::compute (src/lib.rs:2) in rendered tree.\nstdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("src/lib.rs:4"),
        "Bar::compute (src/lib.rs:4) should NOT appear; phantom edge bug must be gone.\nstdout:\n{stdout}"
    );
}

#[test]
fn regression_callers_path_line_on_struct_redirects_to_refs() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct DaemonState {\n\
             pub id: u32,\n\
         }\n\
         pub fn use_state(s: &DaemonState) -> u32 { s.id }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "src/lib.rs:2"]);
    assert!(
        !out.status.success(),
        "callers should exit non-zero (struct, not fn). stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), stderr);
    assert!(
        combined.contains("is a struct"),
        "expected `is a struct` redirect text; got combined output:\n{combined}"
    );
    assert!(
        combined.contains("rustgraph refs") || combined.contains("rustgraph usages"),
        "expected refs/usages redirect text; got combined output:\n{combined}"
    );
}

#[test]
fn regression_call_graph_qualified_name_strict_match_anti_oscillation() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct Net;\n\
         impl Net { pub fn connect() -> u32 { 1 } }\n\
         pub struct Db;\n\
         impl Db { pub fn connect() -> u32 { 2 } }\n\
         pub fn boot() { let _ = Net::connect(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "call-graph", "--root", "boot"]);
    assert!(
        out.status.success(),
        "call-graph exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("connect") && stdout.contains("src/lib.rs:2"),
        "expected Net::connect (src/lib.rs:2) under boot.\nstdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("src/lib.rs:4"),
        "Db::connect (src/lib.rs:4) is a phantom; strict-qualified resolution must drop it.\nstdout:\n{stdout}"
    );

    let connect_child_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| (l.contains("├──") || l.contains("└──")) && l.contains("connect"))
        .collect();
    assert_eq!(
        connect_child_lines.len(),
        1,
        "expected exactly one `connect` tree-child under boot; got {}: {:?}\nfull stdout:\n{stdout}",
        connect_child_lines.len(),
        connect_child_lines
    );
}
