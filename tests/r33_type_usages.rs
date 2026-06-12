//! R33: type-usage queries — the rusty-merchant field-report gap.
//!
//! Two engine holes closed and one MCP exposure added:
//! 1. `refs`/`usages` were macro-blind: `vec![SellOrder { .. }]` and
//!    `assert_eq!(o.field, 1)` were invisible because syn doesn't descend into
//!    macro token streams. Bodies are now re-parsed (spans survive → exact
//!    lines), with a raw token-scan fallback tagged `macro`.
//! 2. `usages` used a duplicate lite visitor that missed pattern/path/variant/
//!    assoc_call kinds; it now shares the full `refs` pipeline and takes
//!    `--kind`.
//! 3. `rustgraph_usages` is exposed over MCP (was CLI-only, so agents fell
//!    back to text search for "where is this type used").

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

/// Mini rusty-merchant: a struct constructed via literals in plain code, inside
/// `vec![]`, read inside `assert_eq!`, used in type ascriptions, patterns, and
/// an impl block.
fn fixture(dir: &Path) {
    write_file(
        &dir.join("src/lib.rs"),
        "pub struct SellOrder {\n\
         \x20   pub item_id: i32,\n\
         \x20   pub cost_per_item: i32,\n\
         }\n\
         \n\
         pub struct VendingMachine {\n\
         \x20   pub sell_orders: Vec<SellOrder>,\n\
         }\n\
         \n\
         impl SellOrder {\n\
         \x20   pub fn total(&self, qty: i32) -> i32 {\n\
         \x20       self.cost_per_item * qty\n\
         \x20   }\n\
         }\n\
         \n\
         pub fn to_sell_order(item_id: i32) -> SellOrder {\n\
         \x20   SellOrder { item_id, cost_per_item: 1 }\n\
         }\n\
         \n\
         pub fn describe(order: &SellOrder) -> i32 {\n\
         \x20   match order {\n\
         \x20       SellOrder { item_id: 0, .. } => 0,\n\
         \x20       _ => order.cost_per_item,\n\
         \x20   }\n\
         }\n",
    );
    write_file(
        &dir.join("tests/store.rs"),
        "use fixture_crate::*;\n\
         \n\
         #[test]\n\
         fn roundtrip() {\n\
         \x20   let vm = VendingMachine {\n\
         \x20       sell_orders: vec![SellOrder { item_id: 7, cost_per_item: 75 }],\n\
         \x20   };\n\
         \x20   assert_eq!(vm.sell_orders[0].cost_per_item, 75);\n\
         }\n",
    );
}

/// The headline bug: a struct literal inside `vec![...]` (tests/store.rs:6)
/// was a silent miss. It must now surface with the `struct` tag and the exact
/// line number.
#[test]
fn refs_finds_struct_literal_inside_vec_macro() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "-j", "refs", "SellOrder"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).unwrap();
    let hits: Vec<(&str, u64, &str)> = v["refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["file_path"].as_str().unwrap(),
                r["line"].as_u64().unwrap(),
                r["kind"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(
        hits.iter()
            .any(|(f, l, k)| f.ends_with("tests/store.rs") && *l == 6 && *k == "struct"),
        "vec![SellOrder {{ .. }}] literal must be found at tests/store.rs:6 as `struct`: {hits:?}"
    );
}

/// Field reads inside `assert_eq!` (the `cost_per_item` shape from the field
/// report) must surface as `field` hits.
#[test]
fn refs_finds_field_access_inside_assert_macro() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "-j", "refs", "cost_per_item", "--kind", "field"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).unwrap();
    let lines: Vec<(String, u64)> = v["refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["file_path"].as_str().unwrap().to_string(),
                r["line"].as_u64().unwrap(),
            )
        })
        .collect();
    assert!(
        lines.iter().any(|(f, l)| f.ends_with("tests/store.rs") && *l == 8),
        "assert_eq!(vm.sell_orders[0].cost_per_item, 75) read must be found: {lines:?}"
    );
    // Literal field init inside vec![SellOrder { cost_per_item: 75 }] is a
    // FieldValue, not an ExprField — `field` kind covers `.field` access only.
    // The impl-block read (src/lib.rs) must be there too.
    assert!(
        lines.iter().any(|(f, _)| f.ends_with("src/lib.rs")),
        "self.cost_per_item read in impl must be found: {lines:?}"
    );
}

/// A macro body that parses as neither an expr list nor a stmt list must fall
/// back to the raw token scan — tagged `macro`, never silently dropped.
#[test]
fn refs_token_scan_fallback_for_unparseable_macro_body() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub struct SellOrder;\n\
         macro_rules! weird { ($($t:tt)*) => {}; }\n\
         pub fn f() {\n\
         \x20   weird!(=> SellOrder <=);\n\
         }\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "-j", "refs", "SellOrder"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).unwrap();
    let hits: Vec<(u64, &str)> = v["refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["line"].as_u64().unwrap(), r["kind"].as_str().unwrap()))
        .collect();
    assert!(
        hits.iter().any(|(l, k)| *l == 4 && *k == "macro"),
        "ident inside unparseable macro body must surface as `macro` at line 4: {hits:?}"
    );
}

/// `usages` now runs the FULL refs classification (the old lite visitor missed
/// patterns): the match-arm `SellOrder {{ .. }}` pattern must appear.
#[test]
fn usages_covers_pattern_matches() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "-j", "usages", "SellOrder"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).unwrap();
    let kinds: Vec<String> = v["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["entries"].as_array().unwrap().iter())
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect();
    assert!(kinds.iter().any(|k| k == "pattern"), "match-arm pattern: {kinds:?}");
    assert!(kinds.iter().any(|k| k == "struct"), "literals: {kinds:?}");
    assert!(kinds.iter().any(|k| k == "type"), "ascriptions: {kinds:?}");
}

/// The acceptance shape from the field report: every construction +
/// ascription site for the type, including the vec![] literal in tests.
#[test]
fn usages_answers_where_is_this_type_used() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "usages", "SellOrder"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    for needle in [
        "src/lib.rs:7",   // Vec<SellOrder> field in VendingMachine
        "src/lib.rs:10",  // impl SellOrder
        "src/lib.rs:16",  // fn ... -> SellOrder
        "src/lib.rs:17",  // literal
        "tests/store.rs:6", // literal inside vec![]
    ] {
        assert!(stdout.contains(needle), "missing {needle} in:\n{stdout}");
    }
}

/// `--kind` filters the refs half of usages.
#[test]
fn usages_kind_filter_narrows_refs_section() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path", &base, "-j", "usages", "SellOrder", "--kind", "struct",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).unwrap();
    let kinds: Vec<String> = v["sections"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["entries"].as_array().unwrap().iter())
        .map(|e| e["kind"].as_str().unwrap().to_string())
        .collect();
    assert!(!kinds.is_empty(), "struct literals must remain");
    assert!(
        kinds.iter().all(|k| k == "struct"),
        "--kind struct must drop every other kind: {kinds:?}"
    );
}

/// `usages` on a fn still produces a callers section (the combined contract),
/// and macro-parsed bodies don't break call classification.
#[test]
fn usages_on_fn_still_reports_callers() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    write_file(
        &dir.path().join("src/extra.rs"),
        "pub fn caller() { let _ = crate::to_sell_order(1); }\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "usages", "to_sell_order"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("=== via callers"), "callers section: {stdout}");
    assert!(!stdout.contains("skipped — not a function"), "{stdout}");
}

/// Unknown --kind on usages errors with the allowed list (parse-time guard,
/// same contract as refs).
#[test]
fn usages_rejects_unknown_kind() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "usages", "SellOrder", "--kind", "banana"]);
    assert!(!out.status.success(), "unknown kind must fail");
    let stderr = stderr_of(&out);
    assert!(stderr.contains("macro"), "error must list allowed values: {stderr}");
}

/// Exact line numbers survive the macro-body re-parse (spans are preserved) —
/// a multi-line vec![] literal must report the literal's own line, not the
/// macro invocation line.
#[test]
fn macro_reparse_preserves_exact_lines() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("src/lib.rs"),
        "pub struct SellOrder { pub id: i32 }\n\
         pub fn build() -> Vec<SellOrder> {\n\
         \x20   vec![\n\
         \x20       SellOrder { id: 1 },\n\
         \x20       SellOrder { id: 2 },\n\
         \x20   ]\n\
         }\n",
    );
    let base = dir.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "-j", "refs", "SellOrder", "--kind", "struct"]);
    assert!(out.status.success(), "stderr: {}", stderr_of(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).unwrap();
    let lines: Vec<u64> = v["refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["line"].as_u64().unwrap())
        .collect();
    assert_eq!(lines, vec![4, 5], "literal lines inside vec![] must be exact: {lines:?}");
}
