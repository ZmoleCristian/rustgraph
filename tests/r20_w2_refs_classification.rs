

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
    Command::new(bin).args(full).output().expect("run rustgraph")
}


#[test]
fn regression_refs_struct_assoc_fn_not_classified_as_variant() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub struct SessionDocument;\n\
         impl SessionDocument { pub fn load() -> Self { SessionDocument } }\n\
         pub fn caller() { let _ = SessionDocument::load(); }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "refs", "SessionDocument", "-j"]);
    assert!(
        out.status.success(),
        "refs failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    let refs = json["refs"]
        .as_array()
        .expect("refs array")
        .clone();


    let assoc_call_sites: Vec<&Value> = refs
        .iter()
        .filter(|r| {
            r["line_text"]
                .as_str()
                .map(|t| t.contains("SessionDocument::load"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !assoc_call_sites.is_empty(),
        "expected at least one ref hit for `SessionDocument::load`; refs payload:\n{:#?}",
        refs
    );


    for site in &assoc_call_sites {
        let kind = site["kind"].as_str().unwrap_or("");
        assert_ne!(
            kind, "variant",
            "`SessionDocument::load` site must NOT be classified as `variant` (SessionDocument is a struct, not an enum); site={:#?}",
            site
        );

        assert!(
            matches!(kind, "assoc_call" | "type" | "path" | "call"),
            "unexpected kind `{}` for SessionDocument::load; site={:#?}",
            kind,
            site
        );
    }
}


#[test]
fn regression_refs_enum_variant_correctly_classified() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "pub enum Color { Red, Blue }\n\
         pub fn pick() { let _ = Color::Red; }\n",
    );
    let base = fixture.path().to_string_lossy().to_string();

    let out = run_rustgraph(&["--path", &base, "refs", "Color", "-j"]);
    assert!(
        out.status.success(),
        "refs failed; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    let refs = json["refs"]
        .as_array()
        .expect("refs array")
        .clone();

    let variant_sites: Vec<&Value> = refs
        .iter()
        .filter(|r| {
            r["line_text"]
                .as_str()
                .map(|t| t.contains("Color::Red"))
                .unwrap_or(false)
                && r["kind"].as_str() == Some("variant")
        })
        .collect();

    assert!(
        !variant_sites.is_empty(),
        "expected at least one variant ref for `Color::Red` (positive check that fix 3 didn't break legit enum-variant detection); refs payload:\n{:#?}",
        refs
    );
}
