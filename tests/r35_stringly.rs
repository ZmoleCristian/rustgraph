//! R35: evidence-ranked stringly-typing detector and dependency-origin taxonomy.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::tempdir;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, contents).expect("write fixture");
}

fn fixture(root: &Path) {
    write_file(
        &root.join("Cargo.toml"),
        r#"[package]
name = "stringly-fixture"
version = "0.1.0"
edition = "2024"

[dependencies]
web-url = { package = "url", version = "2" }
"#,
    );
    write_file(
        &root.join("src/lib.rs"),
        r#"use std::net::IpAddr;
use web_url::Url;

pub struct Config {
    pub cache_path: String,
    pub callback_url: String,
    pub user_id: String,
}

pub fn inspect_host(host: String) {
    let _: IpAddr = host.parse().unwrap();
}

pub fn inspect_callback(raw_callback: String) {
    let _: Url = raw_callback.parse().unwrap();
}

pub fn inspect_token(token: String) {
    let _ = token.parse::<missing_types::Token>();
}

pub fn validate_status(status: String) -> Result<(), ()> {
    match status.as_str() {
        "pending" | "running" | "done" => Ok(()),
        _ => return Err(()),
    }
}

pub fn open_kind(kind: String) -> usize {
    match kind.as_str() {
        "alpha" => 1,
        "beta" => 2,
        _ => 3,
    }
}

pub fn parse_year(raw_year: &str) -> Option<i32> {
    raw_year.parse::<i32>().ok()
}

pub fn read_path(path: &str) { let _ = path; }

pub fn set_mode(mode: String) { let _ = mode; }
pub fn call_mode() { set_mode("fast".to_string()); }
"#,
    );
    write_file(
        &root.join("tests/api.rs"),
        "use stringly_fixture::set_mode;\n#[test]\nfn helper(test_url: String) { let _ = test_url.parse::<url::Url>(); set_mode(\"safe\".to_string()); }\n",
    );
}

fn run(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rustgraph"));
    command
        .arg("--no-auto-path")
        .arg("--path")
        .arg(root)
        .args(args)
        .output()
        .expect("run rustgraph")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn default_report_distinguishes_all_four_type_origins() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let output = run(dir.path(), &["--json", "stringly"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: Value = serde_json::from_str(&stdout(&output)).unwrap();

    assert!(json["origin_counts"]["standard-library"].as_u64().unwrap() >= 1);
    assert!(
        json["origin_counts"]["existing-dependency"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(json["origin_counts"]["external-crate"].as_u64().unwrap() >= 1);
    assert!(json["origin_counts"]["local-type"].as_u64().unwrap() >= 1);
}

#[test]
fn json_reports_canonical_path_alias_and_dependency_cost() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let output = run(dir.path(), &["--json", "stringly"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let findings = json["findings"].as_array().unwrap();

    let existing = findings
        .iter()
        .find(|finding| finding["name"] == "raw_callback")
        .expect("existing dependency finding");
    assert_eq!(existing["suggestion"]["display"], "web_url::Url");
    assert_eq!(existing["suggestion"]["canonical_path"], "url::Url");
    assert_eq!(existing["suggestion"]["origin"], "existing-dependency");
    assert_eq!(existing["suggestion"]["dependency_alias"], "web_url");
    assert_eq!(existing["suggestion"]["new_dependency"], false);

    let external = findings
        .iter()
        .find(|finding| finding["name"] == "token")
        .expect("external dependency finding");
    assert_eq!(external["suggestion"]["origin"], "external-crate");
    assert_eq!(external["suggestion"]["crate_name"], "missing_types");
    assert_eq!(external["suggestion"]["new_dependency"], true);
}

#[test]
fn rejecting_match_scores_above_open_fallback_and_explains_both() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let default_output = run(dir.path(), &["--json", "stringly"]);
    let default_json: Value = serde_json::from_str(&stdout(&default_output)).unwrap();
    assert!(
        !default_json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["name"] == "kind")
    );

    let output = run(
        dir.path(),
        &["--json", "stringly", "--min-confidence", "0.80"],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let findings = json["findings"].as_array().unwrap();
    let status = findings
        .iter()
        .find(|finding| finding["name"] == "status")
        .unwrap();
    let kind = findings
        .iter()
        .find(|finding| finding["name"] == "kind")
        .unwrap();

    assert_eq!(status["confidence"], 0.95);
    assert_eq!(kind["confidence"], 0.80);
    assert!(status["evidence"].to_string().contains("rejected"));
    assert!(kind["evidence"].to_string().contains("Unknown(String)"));
}

#[test]
fn name_only_candidates_are_opt_in_via_lower_threshold() {
    let dir = tempdir().unwrap();
    fixture(dir.path());

    let default_output = run(dir.path(), &["--json", "stringly"]);
    let default_json: Value = serde_json::from_str(&stdout(&default_output)).unwrap();
    assert!(
        !default_json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["name"] == "cache_path")
    );

    let lower_output = run(
        dir.path(),
        &["--json", "stringly", "--min-confidence", "0.70"],
    );
    assert!(
        lower_output.status.success(),
        "stderr: {}",
        stderr(&lower_output)
    );
    let lower_json: Value = serde_json::from_str(&stdout(&lower_output)).unwrap();
    let findings = lower_json["findings"].as_array().unwrap();
    let path = findings
        .iter()
        .find(|finding| finding["name"] == "cache_path")
        .unwrap();
    assert_eq!(path["suggestion"]["canonical_path"], "std::path::PathBuf");
    let borrowed = findings
        .iter()
        .find(|finding| finding["name"] == "path")
        .unwrap();
    assert_eq!(borrowed["declared_type"], "&str");
    assert_eq!(borrowed["suggestion"]["canonical_path"], "std::path::Path");
    assert_eq!(borrowed["suggestion"]["display"], "&std::path::Path");
}

#[test]
fn origin_filter_and_exclude_tests_are_applied_before_summary_counts() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let output = run(
        dir.path(),
        &[
            "--exclude-tests",
            "--json",
            "stringly",
            "--origin",
            "external-crate",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(json["origin_counts"]["external-crate"], 1);
    assert_eq!(json["total_findings"], 1);
    assert!(
        json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["suggestion"]["origin"] == "external-crate")
    );
}

#[test]
fn max_results_truncates_payload_but_not_summary() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let output = run(dir.path(), &["--json", "stringly", "--max-results", "1"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(json["findings"].as_array().unwrap().len(), 1);
    assert_eq!(json["truncated"], true);
    assert!(json["total_findings"].as_u64().unwrap() > 1);
}

#[test]
fn cross_file_callsite_evidence_keeps_its_own_source_path() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let output = run(
        dir.path(),
        &["--json", "stringly", "--min-confidence", "0.75"],
    );
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let json: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let mode = json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["name"] == "mode")
        .expect("literal callsites should propose Mode");
    let paths: Vec<&str> = mode["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|evidence| evidence["file_path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"src/lib.rs"), "{paths:?}");
    assert!(paths.contains(&"tests/api.rs"), "{paths:?}");

    let excluded = run(
        dir.path(),
        &[
            "--exclude-tests",
            "--json",
            "stringly",
            "--min-confidence",
            "0.75",
        ],
    );
    assert!(excluded.status.success(), "stderr: {}", stderr(&excluded));
    let excluded_json: Value = serde_json::from_str(&stdout(&excluded)).unwrap();
    assert!(
        excluded_json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["name"] != "mode"),
        "test-only literal evidence must not leak into production findings"
    );
}

#[test]
fn non_actionable_contexts_are_counted_and_explicitly_auditable() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    write_file(
        &dir.path().join("src/generated/model.rs"),
        r#"// @generated — do not edit
pub fn validate_generated(status: &str) -> Result<(), ()> {
    match status { "on" | "off" => Ok(()), _ => Err(()) }
}
"#,
    );
    write_file(
        &dir.path().join("vendor/foreign.rs"),
        r#"pub fn validate_vendor(mode: &str) -> Result<(), ()> {
    match mode { "fast" | "safe" => Ok(()), _ => Err(()) }
}
"#,
    );

    let default_output = run(dir.path(), &["--json", "stringly"]);
    assert!(
        default_output.status.success(),
        "stderr: {}",
        stderr(&default_output)
    );
    let default_json: Value = serde_json::from_str(&stdout(&default_output)).unwrap();
    assert!(default_json["suppressed_findings"].as_u64().unwrap() >= 3);
    assert!(
        default_json["suppression_counts"]["text-boundary"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(
        default_json["suppression_counts"]["generated-code"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(
        default_json["suppression_counts"]["vendored-code"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(
        !default_json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| matches!(
                finding["owner"].as_str(),
                Some("parse_year" | "validate_generated" | "validate_vendor")
            ))
    );

    let audit_output = run(dir.path(), &["--json", "stringly", "--include-suppressed"]);
    assert!(
        audit_output.status.success(),
        "stderr: {}",
        stderr(&audit_output)
    );
    let audit_json: Value = serde_json::from_str(&stdout(&audit_output)).unwrap();
    let findings = audit_json["findings"].as_array().unwrap();
    for (owner, name) in [
        ("parse_year", "raw_year"),
        ("validate_generated", "status"),
        ("validate_vendor", "mode"),
    ] {
        let finding = findings
            .iter()
            .find(|finding| finding["owner"] == owner && finding["name"] == name)
            .unwrap_or_else(|| panic!("missing audited finding {owner}.{name}"));
        assert!(finding["caveats"].as_array().is_some_and(|caveats| {
            caveats
                .iter()
                .any(|caveat| caveat["suppresses_by_default"] == true)
        }));
    }
}

#[test]
fn cli_rejects_invalid_confidence_and_origin_values() {
    let dir = tempdir().unwrap();
    fixture(dir.path());
    let confidence = run(dir.path(), &["stringly", "--min-confidence", "1.2"]);
    assert!(!confidence.status.success());
    assert!(stderr(&confidence).contains("finite number in [0,1]"));

    let origin = run(dir.path(), &["stringly", "--origin", "mystery"]);
    assert!(!origin.status.success());
    assert!(stderr(&origin).contains("standard-library"));
}
