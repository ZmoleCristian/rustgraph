//! LSP JSON-RPC message builders and response parsers for the rust-analyzer client.

use super::super::model::{ReferenceLocation, path_to_file_uri, uri_to_file_path};
use std::path::Path;

/// Build the `initialize` request params for an LSP session rooted at `workspace_root`.
pub(super) fn initialize_params(
    workspace_root: &Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let root_uri = path_to_file_uri(workspace_root)?;
    Ok(serde_json::json!({
        "processId": serde_json::Value::Null,
        "rootUri": root_uri,
        "capabilities": {},
        "workspaceFolders": [
            {
                "uri": root_uri,
                "name": workspace_root
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("workspace")
            }
        ]
    }))
}

/// Wrap `params` in a JSON-RPC 2.0 request envelope with the given `id` and `method`.
pub(super) fn request_payload(
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
}

/// Wrap `params` in a JSON-RPC 2.0 notification envelope (no `id` field).
pub(super) fn notification_payload(method: &str, params: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params
    })
}

/// Return `true` if `message` is a JSON-RPC response whose `id` matches the given `id`.
pub(super) fn is_matching_response(message: &serde_json::Value, id: u64) -> bool {
    message.get("id").and_then(|value| value.as_u64()) == Some(id)
}

/// Return `true` if `error` is a retryable LSP `ContentModified` error (code `-32801`).
pub(super) fn is_retryable_content_modified(error: &serde_json::Value) -> bool {
    let code = error
        .get("code")
        .and_then(|value| value.as_i64())
        .unwrap_or_default();
    let msg = error
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_lowercase();
    code == -32801 && msg.contains("content modified")
}

fn range_start(item: &serde_json::Value, field: &str) -> Option<usize> {
    item.get("range")
        .and_then(|value| value.get("start"))
        .and_then(|value| value.get(field))
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .or_else(|| {
            item.get("targetSelectionRange")
                .and_then(|value| value.get("start"))
                .and_then(|value| value.get(field))
                .and_then(|value| value.as_u64())
                .map(|value| value as usize)
        })
}

/// Parse a single LSP `Location` or `LocationLink` JSON object into a `ReferenceLocation`.
///
/// Supports both `uri`/`range` (standard `Location`) and `targetUri`/`targetSelectionRange`
/// (`LocationLink`) formats.  Returns `None` if required fields are absent or the URI
/// cannot be converted to a filesystem path.  Line numbers are converted from 0-based LSP
/// values to 1-based source-line numbers.
pub(super) fn parse_reference_location(item: &serde_json::Value) -> Option<ReferenceLocation> {
    let uri = item
        .get("uri")
        .and_then(|value| value.as_str())
        .or_else(|| item.get("targetUri").and_then(|value| value.as_str()))?;
    let file_path = uri_to_file_path(uri)?;
    let line = range_start(item, "line")?;
    let character = range_start(item, "character")?;
    Some((file_path, line + 1, character))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn initialize_params_returns_root_uri_for_existing_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let params = initialize_params(dir.path()).expect("params");
        assert!(params["rootUri"].as_str().unwrap().starts_with("file://"));
        assert!(params["workspaceFolders"][0]["name"].is_string());
    }

    #[test]
    fn request_payload_includes_id_and_method() {
        let p = request_payload(42, "textDocument/definition", json!({"x": 1}));
        assert_eq!(p["id"], 42);
        assert_eq!(p["method"], "textDocument/definition");
        assert_eq!(p["params"]["x"], 1);
        assert_eq!(p["jsonrpc"], "2.0");
    }

    #[test]
    fn notification_payload_omits_id() {
        let p = notification_payload("$/cancelRequest", json!({"id": 3}));
        assert!(p.get("id").is_none());
        assert_eq!(p["method"], "$/cancelRequest");
    }

    #[test]
    fn is_matching_response_compares_ids() {
        let msg = json!({"id": 5, "result": {}});
        assert!(is_matching_response(&msg, 5));
        assert!(!is_matching_response(&msg, 6));
    }

    #[test]
    fn is_matching_response_returns_false_when_no_id() {
        let msg = json!({"result": {}});
        assert!(!is_matching_response(&msg, 1));
    }

    #[test]
    fn is_retryable_content_modified_detects_known_code_and_msg() {
        let err = json!({"code": -32801, "message": "content modified"});
        assert!(is_retryable_content_modified(&err));
    }

    #[test]
    fn is_retryable_content_modified_returns_false_for_other_codes() {
        let err = json!({"code": -32601, "message": "method not found"});
        assert!(!is_retryable_content_modified(&err));
    }

    #[test]
    fn is_retryable_content_modified_handles_missing_fields() {
        assert!(!is_retryable_content_modified(&json!({})));
    }

    #[test]
    fn parse_reference_location_uses_uri_and_range() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("a.rs");
        std::fs::write(&target, "").expect("write");
        let uri = path_to_file_uri(&target).expect("uri");
        let item = json!({
            "uri": uri,
            "range": {
                "start": {"line": 5, "character": 3},
                "end": {"line": 5, "character": 10}
            }
        });
        let loc = parse_reference_location(&item).expect("loc");
        assert!(loc.0.ends_with("a.rs"));
        assert_eq!(loc.1, 6);
        assert_eq!(loc.2, 3);
    }

    #[test]
    fn parse_reference_location_supports_target_uri() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("b.rs");
        std::fs::write(&target, "").expect("write");
        let uri = path_to_file_uri(&target).expect("uri");
        let item = json!({
            "targetUri": uri,
            "targetSelectionRange": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 5}
            }
        });
        let loc = parse_reference_location(&item).expect("loc");
        assert!(loc.0.ends_with("b.rs"));
        assert_eq!(loc.1, 1);
    }

    #[test]
    fn parse_reference_location_returns_none_for_missing_uri() {
        let item = json!({"range": {"start": {"line": 0, "character": 0}}});
        assert!(parse_reference_location(&item).is_none());
    }

    #[test]
    fn parse_reference_location_returns_none_for_invalid_uri() {
        let item = json!({"uri": "not a uri", "range": {"start": {"line": 0, "character": 0}}});
        assert!(parse_reference_location(&item).is_none());
    }
}
