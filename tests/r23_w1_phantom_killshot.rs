

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


fn fixture_same_name_free_fns() -> tempfile::TempDir {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/mod_a/lib.rs"),
        "pub fn foo() -> u32 { 1 }\n",
    );
    write_file(
        &fixture.path().join("src/mod_b/lib.rs"),
        "pub fn foo() -> u32 { 2 }\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod mod_a;\n\
         pub mod mod_b;\n\
         pub fn caller_a() { let _ = mod_a::foo(); }\n\
         pub fn caller_b() { let _ = mod_b::foo(); }\n",
    );
    fixture
}


fn fixture_method_vs_free_fn() -> tempfile::TempDir {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/state/mod.rs"),
        "pub struct Daemon;\n\
         impl Daemon {\n\
             pub fn refresh(&self) { /* method body */ }\n\
         }\n",
    );
    write_file(
        &fixture.path().join("src/api/mod.rs"),
        "pub fn refresh() { /* free fn body */ }\n",
    );
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod state;\n\
         pub mod api;\n\
         use state::Daemon;\n\
         pub fn handler(daemon_state: &Daemon) {\n\
             daemon_state.refresh();\n\
         }\n",
    );
    fixture
}


#[test]
fn regression_callers_bare_name_returns_all_same_name_free_fns_loose() {
    let fixture = fixture_same_name_free_fns();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "foo"]);
    assert!(
        out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("mod_a/lib.rs"),
        "expected mod_a/foo match in loose-mode output. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("mod_b/lib.rs"),
        "expected mod_b/foo match in loose-mode output. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("caller_a") && stdout.contains("caller_b"),
        "expected both caller_a and caller_b. stdout:\n{stdout}"
    );
}


#[test]
fn regression_callers_module_qualified_narrows_to_owning_module() {
    let fixture = fixture_same_name_free_fns();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "mod_a::foo"]);
    assert!(
        out.status.success(),
        "callers should succeed for `mod_a::foo`. stderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("mod_a/lib.rs"),
        "expected mod_a/foo match in narrowed output. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("mod_b/lib.rs:1-1"),
        "PHANTOM: mod_b/foo (line 1) should NOT appear under mod_a::foo query. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("caller_a"),
        "expected caller_a in mod_a::foo output. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("caller_b"),
        "PHANTOM: caller_b only calls mod_b::foo; must not appear under mod_a::foo. stdout:\n{stdout}"
    );
}


#[test]
fn regression_callers_symbol_id_is_authoritative_against_homonyms() {
    let fixture = fixture_same_name_free_fns();
    let base = fixture.path().to_string_lossy().to_string();
    let symbol_id = format!("{}/src/mod_a/lib.rs:1:foo", base);

    let out = run_rustgraph(&[
        "--path", &base, "callers", "--symbol-id", &symbol_id,
    ]);
    assert!(
        out.status.success(),
        "callers --symbol-id should succeed. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("mod_a/lib.rs"),
        "expected mod_a/foo (the symbol-id target) in output. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("caller_a"),
        "expected caller_a (the real caller of mod_a::foo). stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("caller_b"),
        "PHANTOM: caller_b only calls mod_b::foo; --symbol-id of mod_a::foo must drop it. stdout:\n{stdout}"
    );
}


#[test]
fn regression_paths_between_symbol_id_to_is_authoritative() {
    let fixture = fixture_same_name_free_fns();
    let base = fixture.path().to_string_lossy().to_string();
    let mod_a_symbol = format!("{}/src/mod_a/lib.rs:1:foo", base);
    let mod_b_symbol = format!("{}/src/mod_b/lib.rs:1:foo", base);


    let out_real = run_rustgraph(&[
        "--path", &base, "paths-between", "caller_a", &mod_a_symbol,
    ]);
    assert!(
        out_real.status.success(),
        "paths-between to mod_a::foo should succeed. stderr:\n{}",
        String::from_utf8_lossy(&out_real.stderr)
    );
    let stdout_real = String::from_utf8_lossy(&out_real.stdout);
    assert!(
        stdout_real.contains("[1 path(s)]"),
        "expected exactly 1 path to mod_a::foo from caller_a. stdout:\n{stdout_real}"
    );
    assert!(
        stdout_real.contains("mod_a/lib.rs"),
        "expected mod_a/foo in path. stdout:\n{stdout_real}"
    );


    let out_phantom = run_rustgraph(&[
        "--path", &base, "paths-between", "caller_a", &mod_b_symbol,
    ]);

    let stdout_phantom = String::from_utf8_lossy(&out_phantom.stdout);
    assert!(
        stdout_phantom.contains("[0 path(s)]"),
        "PHANTOM: caller_a only calls mod_a::foo; paths-between to mod_b::foo must be empty. stdout:\n{stdout_phantom}"
    );
    assert!(
        !stdout_phantom.contains("Path #1"),
        "PHANTOM: no path should be emitted. stdout:\n{stdout_phantom}"
    );
}


#[test]
fn regression_dotted_receiver_method_call_does_not_leak_to_free_fn() {
    let fixture = fixture_method_vs_free_fn();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "refresh"]);
    assert!(
        out.status.success(),
        "callers exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("api/mod.rs"),
        "expected free fn refresh to appear in match list. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("state/mod.rs"),
        "expected method Daemon::refresh to appear in match list. stdout:\n{stdout}"
    );


    let lines: Vec<&str> = stdout.lines().collect();
    let mut free_fn_caller_count: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if line.contains("api/mod.rs:1-1")
            && let Some(next) = lines.get(i + 1)
            && next.contains("Callers:")
        {

            let count_str: String = next.chars().filter(|c| c.is_ascii_digit()).take(1).collect();
            free_fn_caller_count = count_str.parse().ok();
        }
    }
    assert_eq!(
        free_fn_caller_count, Some(0),
        "PHANTOM: free fn `refresh` should have 0 callers (handler only calls daemon_state.refresh()). stdout:\n{stdout}"
    );


    assert!(
        stdout.contains("handler"),
        "expected handler as caller of Daemon::refresh. stdout:\n{stdout}"
    );
}


#[test]
fn regression_anti_oscillation_bare_name_no_narrowing() {
    let fixture = fixture_same_name_free_fns();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "foo"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(stdout.contains("mod_a/lib.rs"), "anti-oscillation: bare query lost mod_a. stdout:\n{stdout}");
    assert!(stdout.contains("mod_b/lib.rs"), "anti-oscillation: bare query lost mod_b. stdout:\n{stdout}");
    assert!(stdout.contains("caller_a"), "anti-oscillation: lost caller_a. stdout:\n{stdout}");
    assert!(stdout.contains("caller_b"), "anti-oscillation: lost caller_b. stdout:\n{stdout}");
}


#[test]
fn regression_anti_oscillation_method_callers_still_show_method_callers() {
    let fixture = fixture_method_vs_free_fn();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "callers", "refresh"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("state/mod.rs"),
        "anti-oscillation: method match block missing. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("handler"),
        "anti-oscillation: handler should still appear as caller of Daemon::refresh. stdout:\n{stdout}"
    );
}


#[test]
fn regression_call_graph_module_qualified_drops_phantom_edges() {
    let fixture = fixture_same_name_free_fns();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "call-graph", "--root", "caller_a"]);
    assert!(
        out.status.success(),
        "call-graph exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("mod_a/lib.rs"),
        "expected mod_a/foo in caller_a's expansion. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("mod_b/lib.rs"),
        "PHANTOM: mod_b/foo edge under caller_a (caller_a only calls mod_a::foo). stdout:\n{stdout}"
    );
}


#[test]
fn regression_callers_symbol_id_other_homonym_authoritative() {
    let fixture = fixture_same_name_free_fns();
    let base = fixture.path().to_string_lossy().to_string();
    let symbol_id = format!("{}/src/mod_b/lib.rs:1:foo", base);

    let out = run_rustgraph(&[
        "--path", &base, "callers", "--symbol-id", &symbol_id,
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("mod_b/lib.rs"),
        "expected mod_b/foo (the symbol-id target). stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("caller_b"),
        "expected caller_b. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("caller_a"),
        "PHANTOM: caller_a only calls mod_a::foo. stdout:\n{stdout}"
    );
}


#[test]
fn regression_paths_between_bare_to_still_resolves_loose() {
    let fixture = fixture_same_name_free_fns();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "paths-between", "caller_a", "foo"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);


    assert!(
        stdout.contains("mod_a/lib.rs"),
        "expected mod_a/foo in path. stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("mod_b/lib.rs"),
        "PHANTOM: mod_b/foo should not be reachable from caller_a. stdout:\n{stdout}"
    );
}
