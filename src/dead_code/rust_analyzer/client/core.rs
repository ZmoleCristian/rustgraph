use std::collections::HashSet;
use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout};

use crate::dead_code::model::ReferenceLocation;
use crate::dead_code::rust_analyzer::protocol::{
    initialize_params, is_matching_response, is_retryable_content_modified, notification_payload,
    parse_reference_location, request_payload,
};

use super::process;
use super::transport;

type DynError = Box<dyn std::error::Error>;

pub(in crate::dead_code::rust_analyzer) struct RustAnalyzerLspClient {
    pub(in crate::dead_code::rust_analyzer) child: Child,
    pub(in crate::dead_code::rust_analyzer) stdin: ChildStdin,
    pub(in crate::dead_code::rust_analyzer) stdout: BufReader<ChildStdout>,
    pub(in crate::dead_code::rust_analyzer) next_id: u64,
}

impl RustAnalyzerLspClient {
    pub(in crate::dead_code::rust_analyzer) fn start(
        binary: &str,
        workspace_root: &Path,
    ) -> Result<Self, DynError> {
        let resolved_binary = process::resolved_binary(binary);
        process::probe_binary(&resolved_binary)?;

        let mut child = process::spawn_process(&resolved_binary, workspace_root)?;

        let stdin = child
            .stdin
            .take()
            .ok_or("failed to capture rust-analyzer stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("failed to capture rust-analyzer stdout")?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        };

        let init_params = initialize_params(workspace_root)?;
        let _ = client
            .request("initialize", init_params)
            .map_err(|e| format!("rust-analyzer LSP initialization failed: {}", e))?;
        client.notify("initialized", serde_json::json!({}))?;
        Ok(client)
    }

    pub(in crate::dead_code::rust_analyzer) fn request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, DynError> {
        for attempt in 0..3 {
            let id = self.next_id;
            self.next_id += 1;
            let request = request_payload(id, method, params.clone());
            transport::write_message(&mut self.stdin, &request)?;

            loop {
                let message = transport::read_message(&mut self.stdout)?;
                if !is_matching_response(&message, id) {
                    continue;
                }
                if let Some(err) = message.get("error") {
                    if is_retryable_content_modified(err) && attempt < 2 {
                        break;
                    }
                    return Err(
                        format!("rust-analyzer '{}' request failed: {}", method, err).into(),
                    );
                }
                return Ok(message
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
            }
        }
        Err(format!(
            "rust-analyzer '{}' request failed after retries due to content changes",
            method
        )
        .into())
    }

    pub(in crate::dead_code::rust_analyzer) fn notify(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), DynError> {
        let notif = notification_payload(method, params);
        transport::write_message(&mut self.stdin, &notif)
    }

    pub(in crate::dead_code::rust_analyzer) fn references(
        &mut self,
        uri: &str,
        line_zero_based: usize,
        character_utf16: usize,
    ) -> Result<Vec<ReferenceLocation>, DynError> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line_zero_based, "character": character_utf16 },
            "context": { "includeDeclaration": false }
        });
        let result = self.request("textDocument/references", params)?;
        let Some(items) = result.as_array() else {
            return Ok(Vec::new());
        };

        let mut out = Vec::new();
        for item in items {
            if let Some(location) = parse_reference_location(item) {
                out.push(location);
            }
        }
        Ok(out)
    }

    pub(in crate::dead_code::rust_analyzer) fn open_document(
        &mut self,
        file_path: &str,
        file_uri: &str,
        opened_files: &mut HashSet<String>,
    ) -> Result<(), DynError> {
        if !opened_files.insert(file_path.to_string()) {
            return Ok(());
        }

        let source = fs::read_to_string(file_path).unwrap_or_default();
        self.notify(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": file_uri,
                    "languageId": "rust",
                    "version": 1,
                    "text": source
                }
            }),
        )
    }

    pub(in crate::dead_code::rust_analyzer) fn shutdown(mut self) {
        let _ = self.request("shutdown", serde_json::json!(null));
        let _ = self.notify("exit", serde_json::json!(null));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
