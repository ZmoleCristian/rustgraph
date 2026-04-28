

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


fn write_split_display_fixture() -> tempfile::TempDir {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub mod keep;\npub mod drop_;\n",
    );
    write_file(
        &fixture.path().join("src/keep/mod.rs"),
        "pub struct A;\nimpl std::fmt::Display for A { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) } }\n",
    );
    write_file(
        &fixture.path().join("src/drop_/mod.rs"),
        "pub struct B;\nimpl std::fmt::Display for B { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { Ok(()) } }\n",
    );
    fixture
}


#[test]
fn regression_impls_in_filter_narrows_to_path_substring() {
    let fixture = write_split_display_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "impls",
        "Display",
        "--in",
        "keep",
    ]);
    assert!(
        out.status.success(),
        "impls failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();


    assert!(
        stdout.lines().any(|l| l.contains(" A ") && l.contains("[handwritten]")),
        "expected `A` impl from src/keep/ to be present; got:\n{stdout}"
    );

    assert!(
        !stdout.lines().any(|l| l.contains(" B ") && l.contains("[handwritten]")),
        "expected `B` impl from src/drop/ to be filtered out by --in keep; got:\n{stdout}"
    );
}


#[test]
fn regression_impls_in_filter_zero_match_hint() {
    let fixture = write_split_display_fixture();
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&[
        "--path",
        &base,
        "impls",
        "Display",
        "--in",
        "totally_made_up_substr_xyz",
    ]);
    assert!(
        out.status.success(),
        "impls --in <nonexistent> should exit 0 (zero-match is not an error); status={:?}, stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );


    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("--in") || combined.contains("widen") || combined.contains("matching"),
        "expected --in zero-match hint mentioning the filter; got:\n{combined}"
    );
}
