

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


fn write_members_fixture(dir: &tempfile::TempDir) -> String {
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub struct Foo {\n    pub x: u32,\n    pub y: String,\n    pub z: Vec<u8>,\n}\n\
         pub enum Bar { A, B }\n\
         pub mod usage;\n\
         pub mod other;\n",
    );
    write_file(
        &dir.path().join("src/usage/mod.rs"),
        "use crate::Foo;\n\
         pub fn read_x(f: &Foo) -> u32 { f.x }\n\
         pub fn read_y(f: &Foo) -> String { f.y.clone() }\n\
         pub fn write_z(mut f: Foo) {\n    f.z.push(1);\n    f.z.push(2);\n}\n\
         pub fn use_all(f: &Foo) {\n    let _ = f.x + 1;\n    let _ = &f.y;\n    let _ = &f.z;\n}\n",
    );
    write_file(
        &dir.path().join("src/other/mod.rs"),
        "use crate::Foo;\n\
         pub fn other_x(f: &Foo) -> u32 { f.x }\n",
    );
    dir.path().to_string_lossy().to_string()
}


#[test]
fn members_struct_lists_three_field_sections_with_counts_and_sites() {
    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);

    let out = run_rustgraph(&["-p", &base, "members", "Foo"]);
    assert!(
        out.status.success(),
        "members Foo should succeed; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);


    assert!(
        stdout.contains("members 'Foo'") && stdout.contains("3 field(s)"),
        "expected header `members 'Foo': 3 field(s)`; got:\n{stdout}"
    );


    assert!(
        stdout.contains("x:") && stdout.contains("y:") && stdout.contains("z:"),
        "expected x/y/z field sections; got:\n{stdout}"
    );


    assert!(
        stdout.contains("x: 3 access"),
        "expected `x: 3 access(es)`; got:\n{stdout}"
    );

    assert!(
        stdout.contains("y: 2 access"),
        "expected `y: 2 access(es)`; got:\n{stdout}"
    );

    assert!(
        stdout.contains("z: 3 access"),
        "expected `z: 3 access(es)`; got:\n{stdout}"
    );


    assert!(
        stdout.contains("usage/mod.rs"),
        "expected sites in usage/mod.rs; got:\n{stdout}"
    );
    assert!(
        stdout.contains("f.x") && stdout.contains("f.y") && stdout.contains("f.z"),
        "expected field-access source fragments; got:\n{stdout}"
    );
}


#[test]
fn members_in_filter_restricts_sites_to_matching_paths() {
    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);

    let out = run_rustgraph(&["-p", &base, "members", "Foo", "--in", "src/usage"]);
    assert!(
        out.status.success(),
        "members --in should succeed; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);


    assert!(
        stdout.contains("usage/mod.rs"),
        "expected usage path; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("other/mod.rs"),
        "other/mod.rs should be filtered out; got:\n{stdout}"
    );
    assert!(
        stdout.contains("x: 2 access"),
        "expected x: 2 (was 3 before filter); got:\n{stdout}"
    );
}


#[test]
fn members_max_results_caps_each_field_section_independently() {
    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);

    let out = run_rustgraph(&["-p", &base, "members", "Foo", "-n", "1"]);
    assert!(
        out.status.success(),
        "members -n 1 should succeed; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);


    assert!(
        stdout.contains("showing 1 of 3"),
        "expected `(showing 1 of 3; ...)` for x and z; got:\n{stdout}"
    );
    assert!(
        stdout.contains("use --max-results to expand"),
        "expected --max-results hint; got:\n{stdout}"
    );


    assert!(
        stdout.contains("x: 3 access"),
        "header count should still show full 3; got:\n{stdout}"
    );
    assert!(
        stdout.contains("z: 3 access"),
        "header count should still show full 3; got:\n{stdout}"
    );
}


#[test]
fn members_json_returns_structured_envelope() {
    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);

    let out = run_rustgraph(&["-p", &base, "members", "Foo", "--json"]);
    assert!(
        out.status.success(),
        "members --json should succeed; stderr={}",
        stderr_of(&out)
    );
    let stdout = stdout_of(&out);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).expect("--json output should parse as JSON");

    assert_eq!(
        v["type"].as_str(),
        Some("Foo"),
        "expected envelope.type == Foo; got:\n{stdout}"
    );
    assert_eq!(
        v["fields_with_accesses"].as_u64(),
        Some(3),
        "expected fields_with_accesses == 3; got:\n{stdout}"
    );
    assert_eq!(
        v["fields_total"].as_u64(),
        Some(3),
        "expected fields_total == 3; got:\n{stdout}"
    );

    let fields = v["fields"].as_array().expect("fields should be an array");
    assert_eq!(fields.len(), 3, "expected 3 field entries");


    let x_field = fields
        .iter()
        .find(|f| f["name"].as_str() == Some("x"))
        .expect("x field present");
    assert_eq!(x_field["access_count"].as_u64(), Some(3));
    assert!(x_field["sites"].is_array());

    let y_field = fields
        .iter()
        .find(|f| f["name"].as_str() == Some("y"))
        .expect("y field present");
    assert_eq!(y_field["access_count"].as_u64(), Some(2));

    let z_field = fields
        .iter()
        .find(|f| f["name"].as_str() == Some("z"))
        .expect("z field present");
    assert_eq!(z_field["access_count"].as_u64(), Some(3));


    let site = &x_field["sites"][0];
    assert!(site["file_path"].is_string());
    assert!(site["line"].is_u64());
    assert!(site["col"].is_u64());
    assert!(site["line_text"].is_string());
}


#[test]
fn members_on_enum_errors_with_redirect_to_refs_kind_variant() {
    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);

    let out = run_rustgraph(&["-p", &base, "members", "Bar"]);
    assert!(
        !out.status.success(),
        "members on enum should exit non-zero; stdout={}",
        stdout_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("'Bar' is an enum, not a struct"),
        "expected enum-not-struct error; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs Bar --kind variant"),
        "expected `refs Bar --kind variant` redirect; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs Bar"),
        "expected `refs Bar` redirect for type uses; got:\n{stderr}"
    );
}


#[test]
fn members_on_nonexistent_struct_suggests_similar_names() {
    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);


    let out = run_rustgraph(&["-p", &base, "members", "Fooz"]);
    assert!(
        !out.status.success(),
        "members on nonexistent should exit non-zero; stdout={}",
        stdout_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("no struct named 'Fooz'"),
        "expected `no struct named 'Fooz'` error; got:\n{stderr}"
    );
    assert!(
        stderr.contains("Did you mean") && stderr.contains("Foo"),
        "expected did-you-mean suggestion mentioning Foo; got:\n{stderr}"
    );
}

#[test]
fn members_on_nonexistent_unrelated_name_errors_without_did_you_mean() {

    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);

    let out = run_rustgraph(&["-p", &base, "members", "ZZZ_unrelated_xyz_qq"]);
    assert!(!out.status.success(), "should exit non-zero");
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("no struct named"),
        "expected `no struct named` error; got:\n{stderr}"
    );

    assert!(
        stderr.contains("find ZZZ_unrelated_xyz_qq"),
        "expected `find <name>` fallback hint; got:\n{stderr}"
    );
}


#[test]
fn bonus_refs_type_kind_field_zero_hits_suggests_members() {
    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);


    let out = run_rustgraph(&["-p", &base, "refs", "Foo", "--kind", "field"]);
    assert!(
        out.status.success(),
        "refs (with empty result) should still succeed; stderr={}",
        stderr_of(&out)
    );
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("note: 0 refs for 'Foo' with --kind field"),
        "expected `note: 0 refs for 'Foo' with --kind field`; got:\n{stderr}"
    );
    assert!(
        stderr.contains("members Foo"),
        "expected `members Foo` redirect in note; got:\n{stderr}"
    );
}

#[test]
fn bonus_refs_unknown_name_kind_field_omits_members_hint() {


    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);

    let out = run_rustgraph(&["-p", &base, "refs", "WhollyUnknownName123", "--kind", "field"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("try: members"),
        "expected NO `try: members` suggestion for unknown name; got:\n{stderr}"
    );

    assert!(
        stderr.contains("no references found for ident 'WhollyUnknownName123'"),
        "expected existing `no references found` line; got:\n{stderr}"
    );
}

#[test]
fn bonus_refs_enum_kind_field_omits_members_hint() {


    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);

    let out = run_rustgraph(&["-p", &base, "refs", "Bar", "--kind", "field"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        !stderr.contains("members Bar"),
        "expected NO `members Bar` suggestion for enum; got:\n{stderr}"
    );
}


#[test]
fn anti_oscillation_refs_kind_field_on_field_name_still_works() {


    let fixture = tempdir().expect("tempdir");
    let base = write_members_fixture(&fixture);

    let out = run_rustgraph(&["-p", &base, "refs", "x", "--kind", "field"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stdout = stdout_of(&out);

    assert!(
        stdout.contains("3 reference(s)") || stdout.contains("[field]"),
        "expected non-zero `[field]` refs for x; got:\n{stdout}"
    );
}

#[test]
fn anti_oscillation_refs_assoc_call_runtime_hint_still_works() {


    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct SessionDocument;\n\
         impl SessionDocument {\n    pub fn load(_p: &str) -> Self { SessionDocument }\n}\n\
         fn caller() { let _ = SessionDocument::load(\"x\"); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["-p", &base, "refs", "load", "--kind", "assoc_call"]);
    assert!(out.status.success(), "stderr={}", stderr_of(&out));
    let stderr = stderr_of(&out);
    assert!(
        stderr.contains("note: 0 refs for 'load' with --kind assoc_call"),
        "R24-W3 assoc_call hint should still fire; got:\n{stderr}"
    );
    assert!(
        stderr.contains("refs SessionDocument --kind assoc_call"),
        "R24-W3 qualifier suggestion should still fire; got:\n{stderr}"
    );
}


#[test]
fn members_appears_in_top_level_help_cheat_sheet() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustgraph"))
        .arg("--help")
        .output()
        .expect("run rustgraph --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("members <Type>"),
        "expected `members <Type>` in --help cheat-sheet; got:\n{text}"
    );
}

#[test]
fn members_subcommand_has_long_help() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustgraph"))
        .args(["members", "--help"])
        .output()
        .expect("run rustgraph members --help");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("--in") && text.contains("--max-results"),
        "expected --in + --max-results documented; got:\n{text}"
    );

    assert!(
        text.contains("--kind field") || text.contains("per-field"),
        "expected gap explanation; got:\n{text}"
    );
}
