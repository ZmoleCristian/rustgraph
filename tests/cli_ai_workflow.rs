use serde_json::Value;
use std::collections::HashSet;
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

fn dead_function_names(json: &Value) -> HashSet<String> {
    json["dead_functions"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|f| f["info"]["name"].as_str().map(|s| s.to_string()))
        .collect()
}

#[test]
fn default_output_is_no_color_and_color_is_opt_in() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn hello() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();

    let plain = run_rustgraph(&["--path", &base, "--analyze", "functions"]);
    assert!(plain.status.success());
    let plain_stdout = String::from_utf8_lossy(&plain.stdout);
    assert!(
        !plain_stdout.contains("\u{1b}["),
        "default output should not contain ANSI escapes"
    );

    let colored = run_rustgraph(&["--path", &base, "--analyze", "functions", "--color"]);
    assert!(colored.status.success());
    let colored_stdout = String::from_utf8_lossy(&colored.stdout);
    assert!(
        colored_stdout.contains("\u{1b}["),
        "--color output should contain ANSI escapes"
    );
}

#[test]
fn json_output_has_no_trailing_summary_text() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn hello() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--analyze", "functions", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("Functions:"),
        "json output must not include text summary suffix"
    );

    let parsed: Value = serde_json::from_slice(&out.stdout).expect("valid pure json output");
    assert_eq!(
        parsed["functions"].as_array().map(|a| a.len()),
        Some(1),
        "expected exactly one function in json output"
    );
}

#[test]
fn ensemble_prefers_exact_name_over_fuzzy_partials() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn list() {}
pub fn test_list_shadow() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--ensemble", "list", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let matches = json["matches"].as_array().cloned().unwrap_or_default();
    assert_eq!(matches.len(), 1, "exact name should narrow candidate set");
    assert_eq!(
        matches[0]["info"]["name"].as_str(),
        Some("list"),
        "exact match should be preferred over fuzzy partials"
    );
}

#[test]
fn dead_code_heuristics_skip_cfg_test_runtime_and_reexports() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod a;
pub mod b;
pub mod reexp;

pub use reexp::api_fn;

pub fn called_live() {}
pub fn definitely_dead() {}

#[no_mangle]
pub extern "C" fn ffi_entry() {}

#[tokio::main]
pub async fn async_entry() {}

pub fn only_test_called() {}
pub fn root_driver() {
    called_live();
    a::duplicate();
}

#[cfg(test)]
pub fn cfg_test_only_public() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_test_only() {
        only_test_called();
    }
}
"#,
    );
    write_file(
        &fixture.path().join("src/a.rs"),
        r#"
pub fn duplicate() {}
"#,
    );
    write_file(
        &fixture.path().join("src/b.rs"),
        r#"
pub fn duplicate() {}
"#,
    );
    write_file(
        &fixture.path().join("src/reexp.rs"),
        r#"
pub fn api_fn() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);

    assert!(dead.contains("definitely_dead"));
    assert!(!dead.contains("api_fn"));
    assert!(!dead.contains("ffi_entry"));
    assert!(!dead.contains("async_entry"));
    assert!(!dead.contains("cfg_test_only_public"));
    assert_eq!(
        json["skipped"]["ambiguous_name_total"].as_u64(),
        Some(0),
        "qualified duplicate calls should resolve without ambiguity"
    );
}

#[test]
fn dead_code_marks_framework_handler_refs_as_live() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
use axum::{routing::{any, get}, Router};

pub fn list() {}
pub fn health_check() {}
pub fn definitely_dead() {}

pub fn routes() -> Router {
    Router::new()
        .route("/items", get(list))
        .route("/health", any(health_check))
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("definitely_dead"));
    assert!(
        !dead.contains("list"),
        "handlers referenced by framework routing should not be reported as dead"
    );
    assert!(
        !dead.contains("health_check"),
        "handlers referenced via any(handler) should not be reported as dead"
    );
    assert!(
        json["skipped"]["framework_handler_refs_total"]
            .as_u64()
            .map(|n| n >= 1)
            .unwrap_or(false),
        "expected framework handler skip counter to increment"
    );
}

#[test]
fn dead_code_marks_multiline_method_routed_handlers_as_live() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
use axum::{routing::{get, post}, Router};

pub fn get_user() {}
pub fn update_user() {}
pub fn delete_user() {}
pub fn create_user() {}
pub fn ws_handler() {}
pub fn definitely_dead() {}

pub fn routes() -> Router {
    Router::new()
        .route(
            "/v1/users/{id}",
            get(get_user)
                .put(update_user)
                .delete(delete_user),
        )
        .route(
            "/v1/users",
            post(create_user),
        )
        .route(
            "/v1/ws",
            axum::routing::get(ws_handler),
        )
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("definitely_dead"));
    assert!(
        !dead.contains("get_user"),
        "handler used via get(handler) should not be reported as dead"
    );
    assert!(
        !dead.contains("update_user"),
        "handler used via .put(handler) should not be reported as dead"
    );
    assert!(
        !dead.contains("delete_user"),
        "handler used via .delete(handler) should not be reported as dead"
    );
    assert!(
        !dead.contains("create_user"),
        "handler used via post(handler) should not be reported as dead"
    );
    assert!(
        !dead.contains("ws_handler"),
        "handler used via fully-qualified routing::get(handler) should not be reported as dead"
    );
}

#[test]
fn dead_code_keeps_transitively_called_model_functions_live() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod models;
pub mod routines;
"#,
    );
    write_file(
        &fixture.path().join("src/models.rs"),
        r#"
pub fn count_moderated_chats_for_session(_session_id: u64) -> usize {
    0
}

pub fn collect_monitor_metrics() {}

pub fn definitely_dead() {}
"#,
    );
    write_file(
        &fixture.path().join("src/routines.rs"),
        r#"
pub fn smoke_test() {
    let _ = crate::models::count_moderated_chats_for_session(42);
}

pub async fn metrics_broadcaster() {
    if true {
        crate::models::collect_monitor_metrics();
    }
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(
        !dead.contains("count_moderated_chats_for_session"),
        "model function used by routine smoke_test should not be reported as dead"
    );
    assert!(
        !dead.contains("collect_monitor_metrics"),
        "model function used by routine metrics broadcaster should not be reported as dead"
    );
    assert!(dead.contains("definitely_dead"));
}

#[test]
fn dead_code_keeps_macro_and_function_pointer_refs_live() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn count_a() -> usize { 1 }
pub fn count_b() -> usize { 2 }
pub fn build_ack(v: usize) -> usize { v + 1 }
pub fn flv_extend(a: usize, b: usize) -> usize { a + b }
pub fn definitely_dead() {}

pub async fn driver() {
    let _ = tokio::join!(count_a(), count_b());
    let values = vec![1usize, 2, 3];
    let _mapped: Vec<_> = values.into_iter().map(build_ack).collect();
    let _sum = [1usize, 2, 3].into_iter().fold(0usize, flv_extend);
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(!dead.contains("count_a"));
    assert!(!dead.contains("count_b"));
    assert!(!dead.contains("build_ack"));
    assert!(!dead.contains("flv_extend"));
    assert!(dead.contains("definitely_dead"));
}

#[test]
fn dead_code_does_not_treat_src_smoke_test_file_as_test_code() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod models;
pub mod smoke_test;
"#,
    );
    write_file(
        &fixture.path().join("src/models.rs"),
        r#"
pub fn live_from_smoke_test() {}
pub fn definitely_dead() {}
"#,
    );
    write_file(
        &fixture.path().join("src/smoke_test.rs"),
        r#"
pub fn run() {
    crate::models::live_from_smoke_test();
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(!dead.contains("live_from_smoke_test"));
    assert!(dead.contains("definitely_dead"));
}

#[test]
fn dead_code_keeps_calls_inside_macro_bodies_live() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn refresh_system_metrics() {}
pub fn collect_monitor_metrics() {}
pub fn definitely_dead() {}

pub async fn run() {
    tokio::select! {
        _ = async {} => {
            refresh_system_metrics();
            collect_monitor_metrics();
        }
    }
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(!dead.contains("refresh_system_metrics"));
    assert!(!dead.contains("collect_monitor_metrics"));
    assert!(dead.contains("definitely_dead"));
}

#[test]
fn dead_code_ignores_macro_rules_definitions_without_invocation() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn ghost_dead() {}
pub fn actually_live() {}

macro_rules! call_ghost_dead {
    () => {
        ghost_dead();
    };
}

pub fn driver() {
    actually_live();
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("ghost_dead"));
    assert!(!dead.contains("actually_live"));
}

#[test]
fn dead_code_keeps_wrapper_function_argument_refs_live() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub async fn admin_layer_fn() {}
pub async fn handle_timeout_error() {}
pub fn parse_traceparent(_header: &str) -> Option<String> { None }
pub fn dispo_converted(_name: &str) -> String { String::new() }
pub fn is_amount(_value: &str) -> Result<(), String> { Ok(()) }
pub fn definitely_dead() {}

pub fn wire() {
    let _admin = axum::middleware::from_fn_with_state((), admin_layer_fn);
    let _timeout = tower::timeout::error::HandleErrorLayer::new(handle_timeout_error);
    let trace = Some("00-abc-def-01");
    let _ = trace
        .and_then(|v| Some(v))
        .and_then(parse_traceparent);
    let maybe_name = Some("sale");
    let _ = maybe_name.map_or_else(|| "fallback".to_string(), dispo_converted);
    let _ = clap::Arg::new("lamports").validator(is_amount);
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(!dead.contains("admin_layer_fn"));
    assert!(!dead.contains("handle_timeout_error"));
    assert!(!dead.contains("parse_traceparent"));
    assert!(!dead.contains("dispo_converted"));
    assert!(!dead.contains("is_amount"));
    assert!(dead.contains("definitely_dead"));
}

#[test]
fn dead_code_skips_generated_files() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod protocol_generated;

pub fn definitely_dead() {}
"#,
    );
    write_file(
        &fixture.path().join("src/protocol_generated.rs"),
        r#"
pub fn generated_public_api() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("definitely_dead"));
    assert!(!dead.contains("generated_public_api"));
}

#[test]
fn dead_code_skips_anchor_program_entrypoints() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
#[program]
pub mod adapter_demo {
    pub fn supply() {}
    pub fn unstake() {}
}

pub fn definitely_dead() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(!dead.contains("supply"));
    assert!(!dead.contains("unstake"));
    assert!(dead.contains("definitely_dead"));
}

#[test]
fn dead_code_skips_test_utils_files() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod test_utils;
pub fn definitely_dead() {}
"#,
    );
    write_file(
        &fixture.path().join("src/test_utils.rs"),
        r#"
pub fn helper_public() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("definitely_dead"));
    assert!(!dead.contains("helper_public"));
}

#[test]
fn dead_code_skips_test_helper_named_functions() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn test_helper_entrypoint() {}
pub fn setup_for_tests() {}
pub fn build_fixture_for_test() {}
pub fn seed_state_for_tests_only() {}
pub fn definitely_dead() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("definitely_dead"));
    assert!(!dead.contains("test_helper_entrypoint"));
    assert!(!dead.contains("setup_for_tests"));
    assert!(!dead.contains("build_fixture_for_test"));
    assert!(!dead.contains("seed_state_for_tests_only"));
}

#[test]
fn dead_code_skips_proc_macro_entrypoints() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
extern crate proc_macro;

#[proc_macro]
pub fn declarative_macro(_input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    proc_macro::TokenStream::new()
}

#[proc_macro_derive(DemoDerive)]
pub fn derive_macro(_input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    proc_macro::TokenStream::new()
}

#[proc_macro_attribute]
pub fn attribute_macro(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    item
}

pub fn definitely_dead() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("definitely_dead"));
    assert!(!dead.contains("declarative_macro"));
    assert!(!dead.contains("derive_macro"));
    assert!(!dead.contains("attribute_macro"));
}

#[test]
fn dead_code_keeps_solana_entrypoint_macro_refs_live() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};

solana_program::entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    _instruction_data: &[u8],
) -> ProgramResult {
    Ok(())
}

pub fn definitely_dead() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("definitely_dead"));
    assert!(!dead.contains("process_instruction"));
}

#[test]
fn dead_code_requires_rust_analyzer_binary() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn maybe_dead() {}
"#,
    );

    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let base = fixture.path().to_string_lossy().to_string();
    let out = Command::new(bin)
        .env("PATH", "/definitely/missing")
        .args(["--path", &base, "--dead-code"])
        .output()
        .expect("run rustgraph with missing PATH");
    assert!(
        !out.status.success(),
        "dead-code mode should fail when rust-analyzer is unavailable"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("failed to start rust-analyzer"),
        "expected explicit rust-analyzer spawn failure, got: {}",
        stderr
    );
}

#[test]
fn ensemble_includes_framework_handler_refs_in_neighborhood_and_callsites() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
use axum::{routing::get, Router};

pub fn list() {}

pub fn routes() -> Router {
    Router::new().route("/items", get(list))
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "list",
        "--json",
        "--ensemble-view",
        "usage",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let m = &json["matches"][0];
    assert!(
        m["call_sites_total"].as_u64().unwrap_or(0) >= 1,
        "framework handler refs should be visible as call sites"
    );
    assert!(
        m["neighborhood"]["upstream_total"].as_u64().unwrap_or(0) >= 1,
        "framework handler refs should create upstream neighborhood edges"
    );
}

#[test]
fn framework_handler_resolution_uses_caller_module_for_duplicate_names() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod disposition;
pub mod pitch;
"#,
    );
    write_file(
        &fixture.path().join("src/disposition/mod.rs"),
        r#"
use axum::routing::get;
pub mod handler;

pub fn routes() {
    let _ = get(handler::list);
}
"#,
    );
    write_file(
        &fixture.path().join("src/disposition/handler.rs"),
        r#"
pub fn list() {}
"#,
    );
    write_file(
        &fixture.path().join("src/pitch/mod.rs"),
        r#"
use axum::routing::get;
pub mod handler;

pub fn routes() {
    let _ = get(handler::list);
}
"#,
    );
    write_file(
        &fixture.path().join("src/pitch/handler.rs"),
        r#"
pub fn list() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let disposition_handler = fixture
        .path()
        .join("src/disposition/handler.rs")
        .to_string_lossy()
        .to_string();
    let query = format!("{}:2", disposition_handler);
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        &query,
        "--json",
        "--ensemble-view",
        "usage",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let m = &json["matches"][0];
    let call_sites = m["call_sites"].as_array().cloned().unwrap_or_default();
    assert_eq!(call_sites.len(), 1, "expected one resolved call site");
    let file = call_sites[0]["file_path"].as_str().unwrap_or("");
    assert!(
        file.ends_with("/src/disposition/mod.rs"),
        "call site should resolve to disposition module route, got {}",
        file
    );
}

#[test]
fn ensemble_filters_low_signal_unresolved_calls() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn wrap(v: Option<u32>) -> Result<Option<u32>, ()> {
    if v.is_some() {
        Ok(v)
    } else {
        Err(())
    }
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "wrap",
        "--json",
        "--ensemble-view",
        "flow",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let unresolved = json["matches"][0]["neighborhood"]["unresolved_outgoing_calls"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect::<Vec<_>>();
    assert!(
        !unresolved
            .iter()
            .any(|c| c == "Ok" || c == "Err" || c == "Some" || c == "None"),
        "constructor-like unresolved calls should be filtered"
    );
}

#[test]
fn dead_code_reports_ambiguous_name_details() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod a;
pub mod b;

pub fn root() {
    duplicate();
}
"#,
    );
    write_file(
        &fixture.path().join("src/a.rs"),
        r#"
pub fn duplicate() {}
"#,
    );
    write_file(
        &fixture.path().join("src/b.rs"),
        r#"
pub fn duplicate() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let details = json["ambiguous_name_details"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let duplicate = details
        .iter()
        .find(|d| d["symbol"].as_str() == Some("duplicate"))
        .expect("expected duplicate ambiguous detail");
    assert_eq!(
        duplicate["unresolved_non_test_call_sites_total"].as_u64(),
        Some(1),
        "expected one unresolved call site for duplicate"
    );
    assert_eq!(
        duplicate["candidates_total"].as_u64(),
        Some(2),
        "expected two candidate duplicate functions"
    );
}

#[test]
fn dead_code_same_crate_fallback_resolves_duplicate_name_calls() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("bins/a/src/m1.rs"),
        r#"
use crate::m2::get;

pub fn caller() {
    get();
}
"#,
    );
    write_file(
        &fixture.path().join("bins/a/src/m2.rs"),
        r#"
pub fn get() {}
"#,
    );
    write_file(
        &fixture.path().join("bins/b/src/m2.rs"),
        r#"
pub fn get() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let details = json["ambiguous_name_details"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        details.iter().all(|d| d["name"].as_str() != Some("get")),
        "same-crate fallback should resolve get without ambiguous detail"
    );
}

#[test]
fn dead_code_ignores_associated_function_calls_for_duplicate_free_names() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod a;
pub mod b;

pub struct Item;

impl From<u8> for Item {
    fn from(_value: u8) -> Self {
        Item
    }
}

pub fn root() {
    let _ = Item::from(1u8);
}
"#,
    );
    write_file(
        &fixture.path().join("src/a.rs"),
        r#"
pub fn from() {}
"#,
    );
    write_file(
        &fixture.path().join("src/b.rs"),
        r#"
pub fn from() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("from"));
    assert_eq!(
        json["skipped"]["ambiguous_name_total"].as_u64(),
        Some(0),
        "associated-function calls like Item::from should not create ambiguous skips for free fn from"
    );
    let details = json["ambiguous_name_details"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        details.iter().all(|d| d["symbol"].as_str() != Some("from")),
        "associated-function calls should not add unresolved ambiguity for free fn from"
    );
}

#[test]
fn dead_code_ignores_external_qualified_calls_for_duplicate_free_names() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod a;
pub mod b;

pub fn root() {
    let _ = serde_json::from_str::<serde_json::Value>("{}");
}
"#,
    );
    write_file(
        &fixture.path().join("src/a.rs"),
        r#"
pub fn from_str() {}
"#,
    );
    write_file(
        &fixture.path().join("src/b.rs"),
        r#"
pub fn from_str() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(dead.contains("from_str"));
    assert_eq!(
        json["skipped"]["ambiguous_name_total"].as_u64(),
        Some(0),
        "external qualified calls like serde_json::from_str should not create ambiguous skips"
    );
    let details = json["ambiguous_name_details"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        details
            .iter()
            .all(|d| d["symbol"].as_str() != Some("from_str")),
        "external qualified calls should not add unresolved ambiguity for free fn from_str"
    );
}

#[test]
fn duplicate_name_resolution_uses_module_paths() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub mod a;
pub mod b;

pub fn root() {
    a::duplicate();
    b::duplicate();
}
"#,
    );
    write_file(
        &fixture.path().join("src/a.rs"),
        r#"
pub fn duplicate() {}
"#,
    );
    write_file(
        &fixture.path().join("src/b.rs"),
        r#"
pub fn duplicate() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--dead-code", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let dead = dead_function_names(&json);
    assert!(
        !dead.contains("duplicate"),
        "module-qualified duplicate calls should keep both functions alive"
    );
    assert_eq!(
        json["skipped"]["ambiguous_name_total"].as_u64(),
        Some(0),
        "no ambiguity should remain for a::duplicate and b::duplicate"
    );
}

#[test]
fn ensemble_includes_neighborhood_and_type_lifecycle() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Event {
    pub id: u32,
}

pub fn parse(raw: &str) -> Event {
    Event { id: raw.len() as u32 }
}

pub fn transform(ev: Event) -> Event {
    ev
}

pub fn persist(ev: Event) {
    let _ = ev.id;
}

pub fn pipeline(raw: &str) {
    let e: Event = parse(raw);
    let e2: Event = transform(e);
    persist(e2);
}

pub fn entry() {
    pipeline("x");
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "pipeline",
        "--json",
        "--ensemble-view",
        "full",
        "--ensemble-lifecycle-types",
        "Event",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let m = &json["matches"][0];

    assert!(m["neighborhood"]["upstream_total"].as_u64().unwrap_or(0) >= 1);
    assert!(m["neighborhood"]["downstream_total"].as_u64().unwrap_or(0) >= 3);

    let lifecycles = m["lifecycles"].as_array().cloned().unwrap_or_default();
    assert!(!lifecycles.is_empty(), "expected lifecycle output");

    let event = lifecycles
        .iter()
        .find(|l| l["type_name"].as_str() == Some("Event"))
        .expect("Event lifecycle should exist");

    assert!(event["functions_total"].as_u64().unwrap_or(0) >= 3);
    assert!(
        event["sample_paths"]
            .as_array()
            .map(|p| !p.is_empty())
            .unwrap_or(false),
        "expected at least one lifecycle sample path"
    );
}

#[test]
fn ensemble_view_flow_can_request_flow_chunks() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Event {
    pub id: u32,
}

pub fn parse(raw: &str) -> Event {
    Event { id: raw.len() as u32 }
}

pub fn persist(ev: Event) {
    let _ = ev.id;
}

pub fn pipeline(raw: &str) {
    let e: Event = parse(raw);
    persist(e);
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "pipeline",
        "--json",
        "--ensemble-view",
        "flow",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(
        json["sections"].as_array().map(|a| a.len()).unwrap_or(0),
        4,
        "flow view should report neighborhood|lifecycle|dataflow|boundaries"
    );

    let m = &json["matches"][0];
    assert_eq!(
        m["code"]["lines"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
        "code section should be omitted when not requested"
    );
    assert_eq!(
        m["structs_used"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
        "structs section should be omitted when not requested"
    );
    assert_eq!(
        m["call_sites"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
        "call-sites section should be omitted when not requested"
    );
    assert!(
        m["dataflow_hints"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "dataflow section should be populated"
    );
}

#[test]
fn ensemble_preset_changes_neighborhood_depth() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn leaf() {}

pub fn level2() {
    leaf();
}

pub fn level1() {
    level2();
}

pub fn root() {
    level1();
}

pub fn entry() {
    root();
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let quick = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "root",
        "--json",
        "--ensemble-preset",
        "quick",
        "--ensemble-view",
        "usage",
    ]);
    assert!(
        quick.status.success(),
        "{}",
        String::from_utf8_lossy(&quick.stderr)
    );

    let deep = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "root",
        "--json",
        "--ensemble-preset",
        "deep",
        "--ensemble-view",
        "usage",
    ]);
    assert!(
        deep.status.success(),
        "{}",
        String::from_utf8_lossy(&deep.stderr)
    );

    let quick_json: Value = serde_json::from_slice(&quick.stdout).expect("valid quick json");
    let deep_json: Value = serde_json::from_slice(&deep.stdout).expect("valid deep json");
    let quick_downstream = quick_json["matches"][0]["neighborhood"]["downstream_total"]
        .as_u64()
        .unwrap_or(0);
    let deep_downstream = deep_json["matches"][0]["neighborhood"]["downstream_total"]
        .as_u64()
        .unwrap_or(0);

    assert!(
        deep_downstream > quick_downstream,
        "deep preset should traverse further neighborhood depth than quick preset"
    );
}

#[test]
fn lifecycle_resolves_ambiguous_duplicate_names_across_files() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Item {
    pub id: u32,
}

pub mod a;
pub mod b;

pub fn pipeline(i: Item) -> Item {
    let x = a::step(i);
    b::step(x)
}
"#,
    );
    write_file(
        &fixture.path().join("src/a.rs"),
        r#"
use crate::Item;

pub fn step(i: Item) -> Item {
    i
}
"#,
    );
    write_file(
        &fixture.path().join("src/b.rs"),
        r#"
use crate::Item;

pub fn step(i: Item) -> Item {
    i
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();


    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "pipeline",
        "--json",
        "--ensemble-view",
        "full",
        "--ensemble-lifecycle-types",
        "Item",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let m = &json["matches"][0];
    let lifecycle_arr = m["lifecycles"].as_array().expect("lifecycles array");
    let item_lifecycle = lifecycle_arr
        .iter()
        .find(|l| l["type_name"].as_str() == Some("Item"))
        .expect("expected Item lifecycle");

    let mut saw_a = false;
    let mut saw_b = false;
    for path in item_lifecycle["sample_paths"]
        .as_array()
        .unwrap_or(&Vec::new())
    {
        for hop in path["hops"].as_array().unwrap_or(&Vec::new()) {
            if let Some(fp) = hop["file_path"].as_str() {
                if fp.ends_with("/a.rs") {
                    saw_a = true;
                }
                if fp.ends_with("/b.rs") {
                    saw_b = true;
                }
            }
        }
    }

    assert!(saw_a, "expected lifecycle paths to include module a::step");
    assert!(saw_b, "expected lifecycle paths to include module b::step");
}

#[test]
fn ensemble_view_usage_returns_usage_sections_only() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub struct Request {
    pub id: u64,
}

pub fn handle(req: Request) {
    log_it(req.id);
}

pub fn log_it(id: u64) {
    println!("{}", id);
}

pub fn run() {
    handle(Request { id: 7 });
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "--ensemble",
        "handle",
        "--ensemble-view",
        "usage",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let sections = json["sections"]
        .as_array()
        .expect("sections array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    assert_eq!(sections, vec!["call-sites", "neighborhood"]);

    let m = &json["matches"][0];
    assert_eq!(
        m["code"]["lines"].as_array().map(|arr| arr.len()),
        Some(0),
        "usage view should omit code section"
    );
    assert_eq!(
        m["structs_used"].as_array().map(|arr| arr.len()),
        Some(0),
        "usage view should omit structs section"
    );
    assert!(
        m["call_sites_total"].as_u64().unwrap_or(0) >= 1,
        "usage view should include call-sites"
    );
}

#[test]
fn callers_returns_calling_functions_with_call_lines() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn target() {}

pub fn alpha() {
    target();
    target();
}

pub fn beta() {
    target();
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "--callers", "target", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let matches = json["matches"].as_array().expect("matches array");
    assert_eq!(matches.len(), 1, "expected one target match");
    assert_eq!(matches[0]["info"]["name"].as_str(), Some("target"));
    assert_eq!(matches[0]["call_sites_total"].as_u64(), Some(3));

    let callers = matches[0]["callers"].as_array().expect("callers array");
    assert_eq!(callers.len(), 2, "expected two caller functions");

    let alpha = callers
        .iter()
        .find(|entry| entry["info"]["name"].as_str() == Some("alpha"))
        .expect("alpha caller present");
    let beta = callers
        .iter()
        .find(|entry| entry["info"]["name"].as_str() == Some("beta"))
        .expect("beta caller present");

    assert_eq!(alpha["call_sites_total"].as_u64(), Some(2));
    assert_eq!(beta["call_sites_total"].as_u64(), Some(1));
    assert_eq!(
        alpha["call_sites"].as_array().map(|arr| arr.len()),
        Some(2),
        "alpha should list two call lines"
    );
    assert_eq!(
        beta["call_sites"].as_array().map(|arr| arr.len()),
        Some(1),
        "beta should list one call line"
    );
}

#[test]
fn callers_subcommand_help_lists_context_flags() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = Command::new(bin)
        .args(["callers", "--help"])
        .output()
        .expect("run callers --help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("-A") && stdout.contains("-B") && stdout.contains("-C"),
        "expected callers --help to show grep-style context flags, got: {}",
        stdout
    );
}

#[test]
fn callers_without_query_prints_callers_help() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = Command::new(bin)
        .args(["callers"])
        .output()
        .expect("run callers");
    assert!(
        !out.status.success(),
        "callers without query should exit non-zero"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{}\n{}", stdout, stderr);
    assert!(
        combined.contains("Usage: rustgraph callers"),
        "expected callers help usage, got: {}",
        combined
    );
}

#[test]
fn ensemble_without_query_prints_ensemble_help() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let out = Command::new(bin)
        .args(["ensemble"])
        .output()
        .expect("run ensemble");
    assert!(
        !out.status.success(),
        "ensemble without query should exit non-zero"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{}\n{}", stdout, stderr);
    assert!(
        combined.contains("Usage: rustgraph ensemble"),
        "expected ensemble help usage, got: {}",
        combined
    );
}

#[test]
fn slice_subcommand_returns_symbol_source_as_json() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn hello(name: &str) -> String {
    format!("hello {name}")
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "slice", "hello", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(json["symbol"]["name"].as_str(), Some("hello"));
    assert_eq!(json["slice"]["start_line"].as_u64(), Some(2));
    assert!(
        json["slice"]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("format!(\"hello {name}\")")
    );
}

#[test]
fn slice_subcommand_reports_ambiguous_queries() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn dup() {}

mod nested {
    pub fn dup() {}
}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "slice", "dup"]);
    assert!(!out.status.success(), "ambiguous slice query should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("query matched multiple symbols"));
    assert!(stderr.contains("use --symbol-id"));
}

#[test]
fn slice_subcommand_supports_explicit_file_ranges() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        r#"
pub fn alpha() {}
pub fn beta() {}
pub fn gamma() {}
"#,
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&[
        "--path",
        &base,
        "slice",
        "--file",
        "src/lib.rs",
        "--start-line",
        "2",
        "--end-line",
        "3",
        "--json",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json: Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(json["symbol"], Value::Null);
    assert!(
        json["slice"]["file_path"]
            .as_str()
            .unwrap_or_default()
            .ends_with("/src/lib.rs")
    );
    let content = json["slice"]["content"].as_str().unwrap_or_default();
    assert!(content.contains("pub fn alpha() {}"));
    assert!(content.contains("pub fn beta() {}"));
}

#[test]
fn call_graph_resolves_ambiguous_local_function_calls() {
    let fixture = tempdir().expect("tempdir");
    write_file(
        &fixture.path().join("src/lib.rs"),
        "mod a;\nmod b;\npub use a::run;\n",
    );
    write_file(
        &fixture.path().join("src/a.rs"),
        "pub fn run() { let _ = value_to_string(\"x\"); }\nfn value_to_string(v: &str) -> String { v.to_string() }\n",
    );
    write_file(
        &fixture.path().join("src/b.rs"),
        "pub fn value_to_string(v: &str) -> String { v.to_string() }\n",
    );

    let base = fixture.path().to_string_lossy().to_string();
    let out = run_rustgraph(&["--path", &base, "call-graph", "--dot", "--detail", "internal"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let expected_edge = format!(
        "\"{}/src/a.rs:1:run\" -> \"{}/src/a.rs:2:value_to_string\";",
        base, base
    );
    assert!(
        stdout.contains(&expected_edge),
        "expected local target resolution edge, got:\n{}",
        stdout
    );

    let wrong_edge = format!(
        "\"{}/src/a.rs:1:run\" -> \"{}/src/b.rs:1:value_to_string\";",
        base, base
    );
    assert!(
        !stdout.contains(&wrong_edge),
        "call graph should not resolve to unrelated module target"
    );
}

#[cfg(unix)]
#[test]
fn broken_pipe_pipeline_does_not_panic() {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    let cmd = format!(
        "\"{}\" --path . --analyze functions | head -n 1 >/dev/null",
        bin.replace('"', "\\\"")
    );
    let out = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run pipeline");

    assert!(
        out.status.success(),
        "pipeline failed: {:?}",
        out.status.code()
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.to_lowercase().contains("panicked"),
        "stderr should not contain panic output: {stderr}"
    );
}
