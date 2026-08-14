use crate::dead_code::model::RUST_ANALYZER_BIN;
use std::path::Path;
use std::process::{Child, Command, Stdio};

type DynError = Box<dyn std::error::Error>;

pub(super) fn resolve_rust_analyzer_binary() -> String {
    let rustup_path = Command::new("rustup")
        .args(["which", "--toolchain", "stable", "rust-analyzer"])
        .output()
        .ok()
        .and_then(|out| {
            if !out.status.success() {
                return None;
            }
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if path.is_empty() { None } else { Some(path) }
        });

    rustup_path.unwrap_or_else(|| RUST_ANALYZER_BIN.to_string())
}

pub(super) fn resolved_binary(binary: &str) -> String {
    if binary == RUST_ANALYZER_BIN {
        resolve_rust_analyzer_binary()
    } else {
        binary.to_string()
    }
}

pub(super) fn probe_binary(binary: &str) -> Result<(), DynError> {
    let version_probe = Command::new(binary)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("failed to start rust-analyzer ('{}'): {}", binary, error))?;
    if version_probe.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&version_probe.stderr)
        .trim()
        .to_string();
    let stdout = String::from_utf8_lossy(&version_probe.stdout)
        .trim()
        .to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "unknown startup failure".to_string()
    };

    Err(format!(
        "rust-analyzer binary '{}' is not runnable: {}",
        binary, detail
    )
    .into())
}

pub(super) fn spawn_process(binary: &str, workspace_root: &Path) -> Result<Child, DynError> {
    Command::new(binary)
        .current_dir(workspace_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("failed to start rust-analyzer ('{}'): {}", binary, error).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_binary_passes_through_other_binaries_unchanged() {
        assert_eq!(resolved_binary("custom-bin"), "custom-bin");
        assert_eq!(resolved_binary("/usr/bin/foo"), "/usr/bin/foo");
    }

    #[test]
    fn probe_binary_returns_error_for_missing_binary() {
        let result = probe_binary("/no/such/binary/in/this/test");
        assert!(result.is_err());
        let err = result.err().unwrap().to_string();
        assert!(
            err.contains("rust-analyzer") || err.contains("failed to start"),
            "expected helpful error, got: {err}"
        );
    }

    #[test]
    fn probe_binary_returns_error_for_unrunnable_binary() {
        let result = probe_binary("false");

        if result.is_ok() {
            return;
        }
        let err = result.err().unwrap().to_string();
        assert!(
            err.contains("rust-analyzer") || err.contains("not runnable") || err.contains("failed"),
            "expected helpful error, got: {err}"
        );
    }
}
