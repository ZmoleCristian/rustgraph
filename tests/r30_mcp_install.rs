

use std::fs;
use std::process::{Command, Output};
use tempfile::tempdir;

fn run_with_home(home: &std::path::Path, args: &[&str]) -> Output {
    let bin = env!("CARGO_BIN_EXE_rustgraph");
    Command::new(bin)
        .env("HOME", home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .args(args)
        .output()
        .expect("run rustgraph")
}

#[test]
fn install_no_clients_present_succeeds_with_skipped_status() {
    let home = tempdir().unwrap();
    let out = run_with_home(home.path(), &["mcp", "install"]);
    assert!(out.status.success(), "install should exit 0 when no clients found");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("claude"), "report should list claude: {}", stdout);
    assert!(stdout.contains("codex"), "report should list codex: {}", stdout);
    assert!(stdout.contains("gemini"), "report should list gemini: {}", stdout);
    assert!(stdout.contains("skipped"), "all should be skipped: {}", stdout);
}

#[test]
fn install_claude_creates_mcpservers_entry() {
    let home = tempdir().unwrap();
    let claude_path = home.path().join(".claude.json");
    fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
    fs::write(&claude_path, r#"{"someKey":"v","numStartups":42}"#).unwrap();

    let out = run_with_home(home.path(), &["mcp", "install"]);
    assert!(out.status.success(), "install failed: {:?}", out);

    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&claude_path).unwrap()).unwrap();
    assert_eq!(after["someKey"], "v", "preserved unrelated keys");
    assert_eq!(after["numStartups"], 42);
    assert_eq!(
        after["mcpServers"]["rustgraph"]["command"]
            .as_str()
            .unwrap(),
        "rustgraph"
    );
    assert_eq!(
        after["mcpServers"]["rustgraph"]["args"][0]
            .as_str()
            .unwrap(),
        "mcp"
    );
    assert_eq!(
        after["mcpServers"]["rustgraph"]["type"].as_str().unwrap(),
        "stdio"
    );
}

#[test]
fn install_is_idempotent_re_run_no_change() {
    let home = tempdir().unwrap();
    let claude_path = home.path().join(".claude.json");
    fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
    fs::write(&claude_path, r#"{}"#).unwrap();

    let _ = run_with_home(home.path(), &["mcp", "install"]);
    let first = fs::read_to_string(&claude_path).unwrap();

    let out = run_with_home(home.path(), &["mcp", "install"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("already registered"),
        "second run must report already-registered, got: {}",
        stdout
    );
    let second = fs::read_to_string(&claude_path).unwrap();
    assert_eq!(first, second, "config must be byte-identical on idempotent re-run");
}

#[test]
fn install_codex_writes_toml_entry() {
    let home = tempdir().unwrap();
    let codex_path = home.path().join(".codex/config.toml");
    fs::create_dir_all(codex_path.parent().unwrap()).unwrap();
    fs::write(&codex_path, "model = \"gpt-5.5\"\n").unwrap();

    let out = run_with_home(home.path(), &["mcp", "install"]);
    assert!(out.status.success());

    let after = fs::read_to_string(&codex_path).unwrap();
    assert!(after.contains("model = \"gpt-5.5\""), "preserved existing keys: {}", after);
    assert!(after.contains("[mcp_servers.rustgraph]"), "added section: {}", after);
    assert!(after.contains("command = \"rustgraph\""), "command line: {}", after);
    assert!(after.contains("args = [\"mcp\", \"serve\"]"), "args line: {}", after);
}

#[test]
fn install_gemini_writes_trust_true() {
    let home = tempdir().unwrap();
    let gemini_path = home.path().join(".gemini/settings.json");
    fs::create_dir_all(gemini_path.parent().unwrap()).unwrap();
    fs::write(&gemini_path, r#"{"theme":"dark"}"#).unwrap();

    let out = run_with_home(home.path(), &["mcp", "install"]);
    assert!(out.status.success());

    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&gemini_path).unwrap()).unwrap();
    assert_eq!(after["theme"], "dark", "preserved unrelated keys");
    assert_eq!(after["mcpServers"]["rustgraph"]["trust"], true);
    assert_eq!(after["mcpServers"]["rustgraph"]["command"], "rustgraph");
}

#[test]
fn uninstall_removes_only_rustgraph_entry() {
    let home = tempdir().unwrap();
    let claude_path = home.path().join(".claude.json");
    fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
    let initial = serde_json::json!({
        "mcpServers": {
            "rustgraph": {"command": "rustgraph", "args": ["mcp"], "type": "stdio"},
            "other-server": {"command": "other", "args": []}
        }
    });
    fs::write(&claude_path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

    let out = run_with_home(home.path(), &["mcp", "uninstall"]);
    assert!(out.status.success());

    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&claude_path).unwrap()).unwrap();
    assert!(after["mcpServers"]["rustgraph"].is_null());
    assert_eq!(
        after["mcpServers"]["other-server"]["command"]
            .as_str()
            .unwrap(),
        "other"
    );
}

#[test]
fn install_creates_backup_file() {
    let home = tempdir().unwrap();
    let claude_path = home.path().join(".claude.json");
    fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
    fs::write(&claude_path, r#"{}"#).unwrap();

    let _ = run_with_home(home.path(), &["mcp", "install"]);

    let backups: Vec<_> = fs::read_dir(claude_path.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .contains("rustgraph-bak.")
        })
        .collect();
    assert!(
        !backups.is_empty(),
        "expected at least one backup file in {:?}",
        claude_path.parent().unwrap()
    );
}

#[test]
fn status_shows_per_client_state() {
    let home = tempdir().unwrap();
    let claude_path = home.path().join(".claude.json");
    fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
    fs::write(&claude_path, r#"{}"#).unwrap();


    let out = run_with_home(home.path(), &["mcp", "list"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("entry not present"), "should report claude unregistered: {}", stdout);
    assert!(stdout.contains("config not found"), "should report codex/gemini missing: {}", stdout);


    let _ = run_with_home(home.path(), &["mcp", "install"]);
    let out2 = run_with_home(home.path(), &["mcp", "list"]);
    let stdout2 = String::from_utf8_lossy(&out2.stdout);
    assert!(stdout2.contains("already registered"), "post-install status: {}", stdout2);
}

#[test]
fn quiet_flag_suppresses_per_client_lines() {
    let home = tempdir().unwrap();
    let claude_path = home.path().join(".claude.json");
    fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
    fs::write(&claude_path, r#"{}"#).unwrap();

    let out = run_with_home(home.path(), &["mcp", "install", "--quiet"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("registered"),
        "quiet should suppress per-client lines: {}",
        stdout
    );
}

#[test]
fn malformed_claude_config_does_not_crash_other_clients() {
    let home = tempdir().unwrap();
    let claude_path = home.path().join(".claude.json");
    fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
    fs::write(&claude_path, r#"{this is not valid json"#).unwrap();

    let gemini_path = home.path().join(".gemini/settings.json");
    fs::create_dir_all(gemini_path.parent().unwrap()).unwrap();
    fs::write(&gemini_path, r#"{}"#).unwrap();

    let out = run_with_home(home.path(), &["mcp", "install"]);

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("FAILED"), "claude should report FAILED: {}", stdout);
    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&gemini_path).unwrap()).unwrap();
    assert_eq!(after["mcpServers"]["rustgraph"]["command"], "rustgraph");
}
