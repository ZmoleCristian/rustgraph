

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Result of one MCP registration or unregistration attempt for a detected AI
/// client.
#[derive(Debug)]
pub struct ClientReg {
    /// Short client name displayed in the report (e.g. `"claude"`, `"codex"`).
    pub name: &'static str,
    /// Absolute path to the client config file that was read or written.
    pub config_path: PathBuf,
    /// Outcome of the install / uninstall / status operation.
    pub status: RegStatus,
}

/// Outcome of a single MCP config operation on one AI client.
#[derive(Debug, PartialEq)]
pub enum RegStatus {
    /// Client config file not found; client likely not installed.
    NotFound,
    /// Entry already present with correct values; no write performed.
    AlreadyRegistered,
    /// Entry was written successfully.
    Registered,
    /// Entry was removed successfully.
    Removed,
    /// Uninstall was requested but the entry was not present.
    NotPresent,
    /// Operation failed; inner string contains the error message.
    Failed(String),
}

impl RegStatus {
    /// Returns a short human-readable status label for use in CLI reports.
    pub fn label(&self) -> &'static str {
        match self {
            RegStatus::NotFound => "skipped (config not found — client likely not installed)",
            RegStatus::AlreadyRegistered => "already registered (no change)",
            RegStatus::Registered => "registered",
            RegStatus::Removed => "removed",
            RegStatus::NotPresent => "skipped (entry not present)",
            RegStatus::Failed(_) => "FAILED",
        }
    }
}

/// Register the rustgraph MCP entry in every detected AI client config.
///
/// Returns one [`ClientReg`] per client. Idempotent: already-registered
/// entries are left unchanged. Writes are atomic (tmp-file rename) with an
/// auto-timestamped backup.
pub fn install_all() -> Vec<ClientReg> {
    vec![
        register_claude(),
        register_codex(),
        register_gemini(),
    ]
}

/// Remove the rustgraph MCP entry from every detected AI client config.
///
/// Returns one [`ClientReg`] per client. Clients whose config file is absent
/// or that do not currently contain the rustgraph entry are reported without
/// error.
pub fn uninstall_all() -> Vec<ClientReg> {
    vec![
        unregister_claude(),
        unregister_codex(),
        unregister_gemini(),
    ]
}

/// Query the current MCP registration state for every detected AI client.
///
/// Read-only; no config files are written.
pub fn list_all() -> Vec<ClientReg> {
    vec![status_claude(), status_codex(), status_gemini()]
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn claude_config_path() -> PathBuf {


    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        PathBuf::from(dir).join(".claude.json")
    } else {
        home().join(".claude.json")
    }
}

fn codex_config_path() -> PathBuf {
    home().join(".codex/config.toml")
}

fn gemini_config_path() -> PathBuf {
    home().join(".gemini/settings.json")
}

fn target_command() -> &'static str {
    "rustgraph"
}

fn target_args() -> Vec<String> {
    vec!["mcp".into(), "serve".into()]
}

fn backup(path: &Path) -> io::Result<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup_path = path.with_extension(format!("rustgraph-bak.{}", ts));
    fs::copy(path, &backup_path)?;
    Ok(backup_path)
}

fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let tmp = path.with_extension("rustgraph-tmp");
    fs::write(&tmp, contents)?;
    fs::rename(tmp, path)?;
    Ok(())
}


fn register_claude() -> ClientReg {
    let path = claude_config_path();
    if !path.exists() {
        return ClientReg { name: "claude", config_path: path, status: RegStatus::NotFound };
    }
    let status = match merge_claude(&path) {
        Ok(true) => RegStatus::Registered,
        Ok(false) => RegStatus::AlreadyRegistered,
        Err(e) => RegStatus::Failed(e.to_string()),
    };
    ClientReg { name: "claude", config_path: path, status }
}

fn unregister_claude() -> ClientReg {
    let path = claude_config_path();
    if !path.exists() {
        return ClientReg { name: "claude", config_path: path, status: RegStatus::NotFound };
    }
    let status = match remove_claude(&path) {
        Ok(true) => RegStatus::Removed,
        Ok(false) => RegStatus::NotPresent,
        Err(e) => RegStatus::Failed(e.to_string()),
    };
    ClientReg { name: "claude", config_path: path, status }
}

fn status_claude() -> ClientReg {
    let path = claude_config_path();
    if !path.exists() {
        return ClientReg { name: "claude", config_path: path, status: RegStatus::NotFound };
    }
    let status = match check_claude(&path) {
        Ok(true) => RegStatus::AlreadyRegistered,
        Ok(false) => RegStatus::NotPresent,
        Err(e) => RegStatus::Failed(e.to_string()),
    };
    ClientReg { name: "claude", config_path: path, status }
}

fn target_claude_value() -> serde_json::Value {
    serde_json::json!({
        "type": "stdio",
        "command": target_command(),
        "args": target_args(),
    })
}

fn merge_claude(path: &Path) -> io::Result<bool> {
    let contents = fs::read_to_string(path)?;
    let mut json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let target = target_claude_value();

    let obj = json
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "config root not a JSON object"))?;
    let mcps = obj
        .entry("mcpServers".to_string())
        .or_insert_with(|| serde_json::json!({}));
    let mcps_obj = mcps
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "mcpServers not a JSON object"))?;

    if mcps_obj.get("rustgraph") == Some(&target) {
        return Ok(false);
    }
    mcps_obj.insert("rustgraph".to_string(), target);

    let _ = backup(path);
    let new_contents = serde_json::to_string_pretty(&json)?;
    write_atomic(path, &new_contents)?;
    Ok(true)
}

fn remove_claude(path: &Path) -> io::Result<bool> {
    let contents = fs::read_to_string(path)?;
    let mut json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let removed = json
        .as_object_mut()
        .and_then(|o| o.get_mut("mcpServers"))
        .and_then(|m| m.as_object_mut())
        .map(|mcps| mcps.remove("rustgraph").is_some())
        .unwrap_or(false);
    if removed {
        let _ = backup(path);
        let new_contents = serde_json::to_string_pretty(&json)?;
        write_atomic(path, &new_contents)?;
    }
    Ok(removed)
}

fn check_claude(path: &Path) -> io::Result<bool> {
    let contents = fs::read_to_string(path)?;
    let json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let registered = json
        .get("mcpServers")
        .and_then(|m| m.get("rustgraph"))
        .is_some();
    Ok(registered)
}


fn register_codex() -> ClientReg {
    let path = codex_config_path();
    if !path.exists() {
        return ClientReg { name: "codex", config_path: path, status: RegStatus::NotFound };
    }
    let status = match merge_codex(&path) {
        Ok(true) => RegStatus::Registered,
        Ok(false) => RegStatus::AlreadyRegistered,
        Err(e) => RegStatus::Failed(e.to_string()),
    };
    ClientReg { name: "codex", config_path: path, status }
}

fn unregister_codex() -> ClientReg {
    let path = codex_config_path();
    if !path.exists() {
        return ClientReg { name: "codex", config_path: path, status: RegStatus::NotFound };
    }
    let status = match remove_codex(&path) {
        Ok(true) => RegStatus::Removed,
        Ok(false) => RegStatus::NotPresent,
        Err(e) => RegStatus::Failed(e.to_string()),
    };
    ClientReg { name: "codex", config_path: path, status }
}

fn status_codex() -> ClientReg {
    let path = codex_config_path();
    if !path.exists() {
        return ClientReg { name: "codex", config_path: path, status: RegStatus::NotFound };
    }
    let status = match check_codex(&path) {
        Ok(true) => RegStatus::AlreadyRegistered,
        Ok(false) => RegStatus::NotPresent,
        Err(e) => RegStatus::Failed(e.to_string()),
    };
    ClientReg { name: "codex", config_path: path, status }
}

fn merge_codex(path: &Path) -> io::Result<bool> {
    let contents = fs::read_to_string(path)?;
    let mut doc: toml_edit::DocumentMut = contents
        .parse()
        .map_err(|e: toml_edit::TomlError| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

    let already = doc
        .get("mcp_servers")
        .and_then(|t| t.get("rustgraph"))
        .map(|t| {
            let cmd_ok = t.get("command").and_then(|v| v.as_str()) == Some(target_command());
            let args_ok = t
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()
                        == target_args().iter().map(|s| s.as_str()).collect::<Vec<_>>()
                })
                .unwrap_or(false);
            cmd_ok && args_ok
        })
        .unwrap_or(false);
    if already {
        return Ok(false);
    }

    let mcp_servers = doc
        .entry("mcp_servers")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let mcp_table = mcp_servers
        .as_table_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "mcp_servers not a TOML table"))?;

    let mut entry = toml_edit::Table::new();
    entry["command"] = toml_edit::value(target_command());
    let mut args_arr = toml_edit::Array::new();
    for a in target_args() {
        args_arr.push(a);
    }
    entry["args"] = toml_edit::Item::Value(toml_edit::Value::Array(args_arr));
    mcp_table["rustgraph"] = toml_edit::Item::Table(entry);

    let _ = backup(path);
    write_atomic(path, &doc.to_string())?;
    Ok(true)
}

fn remove_codex(path: &Path) -> io::Result<bool> {
    let contents = fs::read_to_string(path)?;
    let mut doc: toml_edit::DocumentMut = contents
        .parse()
        .map_err(|e: toml_edit::TomlError| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let removed = doc
        .get_mut("mcp_servers")
        .and_then(|t| t.as_table_mut())
        .map(|t| t.remove("rustgraph").is_some())
        .unwrap_or(false);
    if removed {
        let _ = backup(path);
        write_atomic(path, &doc.to_string())?;
    }
    Ok(removed)
}

fn check_codex(path: &Path) -> io::Result<bool> {
    let contents = fs::read_to_string(path)?;
    let doc: toml_edit::DocumentMut = contents
        .parse()
        .map_err(|e: toml_edit::TomlError| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(doc.get("mcp_servers").and_then(|t| t.get("rustgraph")).is_some())
}


fn register_gemini() -> ClientReg {
    let path = gemini_config_path();
    if !path.exists() {
        return ClientReg { name: "gemini", config_path: path, status: RegStatus::NotFound };
    }
    let status = match merge_gemini(&path) {
        Ok(true) => RegStatus::Registered,
        Ok(false) => RegStatus::AlreadyRegistered,
        Err(e) => RegStatus::Failed(e.to_string()),
    };
    ClientReg { name: "gemini", config_path: path, status }
}

fn unregister_gemini() -> ClientReg {
    let path = gemini_config_path();
    if !path.exists() {
        return ClientReg { name: "gemini", config_path: path, status: RegStatus::NotFound };
    }
    let status = match remove_gemini(&path) {
        Ok(true) => RegStatus::Removed,
        Ok(false) => RegStatus::NotPresent,
        Err(e) => RegStatus::Failed(e.to_string()),
    };
    ClientReg { name: "gemini", config_path: path, status }
}

fn status_gemini() -> ClientReg {
    let path = gemini_config_path();
    if !path.exists() {
        return ClientReg { name: "gemini", config_path: path, status: RegStatus::NotFound };
    }
    let status = match check_gemini(&path) {
        Ok(true) => RegStatus::AlreadyRegistered,
        Ok(false) => RegStatus::NotPresent,
        Err(e) => RegStatus::Failed(e.to_string()),
    };
    ClientReg { name: "gemini", config_path: path, status }
}

fn target_gemini_value() -> serde_json::Value {
    serde_json::json!({
        "command": target_command(),
        "args": target_args(),
        "trust": true,
    })
}

fn merge_gemini(path: &Path) -> io::Result<bool> {
    let contents = fs::read_to_string(path)?;
    let mut json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let target = target_gemini_value();

    let obj = json
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "config root not a JSON object"))?;
    let mcps = obj
        .entry("mcpServers".to_string())
        .or_insert_with(|| serde_json::json!({}));
    let mcps_obj = mcps
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "mcpServers not a JSON object"))?;

    if mcps_obj.get("rustgraph") == Some(&target) {
        return Ok(false);
    }
    mcps_obj.insert("rustgraph".to_string(), target);

    let _ = backup(path);
    let new_contents = serde_json::to_string_pretty(&json)?;
    write_atomic(path, &new_contents)?;
    Ok(true)
}

fn remove_gemini(path: &Path) -> io::Result<bool> {
    let contents = fs::read_to_string(path)?;
    let mut json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let removed = json
        .as_object_mut()
        .and_then(|o| o.get_mut("mcpServers"))
        .and_then(|m| m.as_object_mut())
        .map(|mcps| mcps.remove("rustgraph").is_some())
        .unwrap_or(false);
    if removed {
        let _ = backup(path);
        let new_contents = serde_json::to_string_pretty(&json)?;
        write_atomic(path, &new_contents)?;
    }
    Ok(removed)
}

fn check_gemini(path: &Path) -> io::Result<bool> {
    let contents = fs::read_to_string(path)?;
    let json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(json.get("mcpServers").and_then(|m| m.get("rustgraph")).is_some())
}


/// Print the MCP-not-registered nudge banner to stderr if [`nudge_needed`] is true.
pub fn print_nudge_if_needed() {
    if nudge_needed() {
        eprintln!("[!] MCP server not registered with any detected AI client.");
        eprintln!("    Run `rustgraph mcp install` to enable agent integration.");
        eprintln!("    (silence: touch ~/.config/rustgraph/no-nudge)");
        eprintln!();
    }
}

/// Returns true if the user should be nudged to run `rustgraph mcp install`:
/// at least one AI client config exists, none of them register rustgraph yet,
/// and the user has not opted out via `~/.config/rustgraph/no-nudge`.
pub fn nudge_needed() -> bool {
    if dirs::config_dir().map(|d| d.join("rustgraph/no-nudge").exists()).unwrap_or(false) {
        return false;
    }
    let regs = list_all();
    let any_client = regs.iter().any(|r| r.status != RegStatus::NotFound);
    if !any_client {
        return false;
    }
    let any_registered = regs.iter().any(|r| r.status == RegStatus::AlreadyRegistered);
    !any_registered
}

/// Print a formatted status report for a slice of [`ClientReg`] results.
///
/// When `quiet` is `true`, no output is produced (suitable for scripts that
/// only need the process exit code).
pub fn print_report(regs: &[ClientReg], quiet: bool) {
    if quiet {
        return;
    }
    for r in regs {
        let extra = match &r.status {
            RegStatus::Failed(e) => format!(" — {}", e),
            _ => String::new(),
        };
        println!(
            "  {:<8} [{:<60}] {}{}",
            r.name,
            r.config_path.display().to_string(),
            r.status.label(),
            extra,
        );
    }
}
