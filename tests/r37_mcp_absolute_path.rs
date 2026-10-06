use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use tempfile::tempdir;

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Session {
    fn start(cwd: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rustgraph"))
            .args(["mcp", "serve"])
            .current_dir(cwd)
            .env_remove("RUSTGRAPH_MISUSE_LOG")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn rustgraph mcp serve");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut session = Self {
            child,
            stdin,
            stdout,
            next_id: 0,
        };
        session.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "r37", "version": "0" }
            }),
        );
        session.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        session
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").expect("write request");
        self.stdin.flush().expect("flush request");
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read response");
            assert!(read > 0, "server closed stdout before answering {method}");
            let message: Value = serde_json::from_str(&line).expect("response JSON");
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.child.kill().expect("kill mcp server");
        self.child.wait().expect("reap mcp server");
    }
}

fn error_message(response: &Value) -> String {
    assert_eq!(
        response["error"]["code"],
        json!(-32602),
        "expected invalid_params: {response}"
    );
    assert_eq!(
        response["error"]["data"]["kind"],
        json!("rustgraph_invalid_path"),
        "{response}"
    );
    response["error"]["message"]
        .as_str()
        .expect("message")
        .to_string()
}

const TOOLS: [&str; 7] = [
    "rustgraph_find",
    "rustgraph_callers",
    "rustgraph_ensemble",
    "rustgraph_usages",
    "rustgraph_paths_between",
    "rustgraph_stringly",
    "rustgraph_tree",
];

fn minimal_args(tool: &str) -> Value {
    match tool {
        "rustgraph_find" => json!({ "query": "main" }),
        "rustgraph_paths_between" => json!({ "from": "main", "to": "run" }),
        "rustgraph_callers" | "rustgraph_ensemble" | "rustgraph_usages" => {
            json!({ "target": "main" })
        }
        "rustgraph_stringly" | "rustgraph_tree" => json!({}),
        other => panic!("unknown tool {other}"),
    }
}

#[test]
fn every_tool_schema_requires_path() {
    let cwd = tempdir().expect("tempdir");
    let mut session = Session::start(cwd.path());
    let listed = session.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), TOOLS.len(), "{listed}");
    for tool in tools {
        let required = tool["inputSchema"]["required"]
            .as_array()
            .expect("required array");
        assert!(
            required.contains(&json!("path")),
            "{} must require path: {tool}",
            tool["name"]
        );
        let description = tool["inputSchema"]["properties"]["path"]["description"]
            .as_str()
            .expect("path description");
        assert!(description.contains("Absolute"), "{description}");
    }
}

#[test]
fn every_tool_rejects_missing_path() {
    let cwd = tempdir().expect("tempdir");
    let mut session = Session::start(cwd.path());
    for tool in TOOLS {
        let message = error_message(&session.call(tool, minimal_args(tool)));
        assert!(
            message.contains("missing required `path`"),
            "{tool}: {message}"
        );
    }
}

#[test]
fn every_tool_rejects_relative_path() {
    let cwd = tempdir().expect("tempdir");
    let mut session = Session::start(cwd.path());
    for tool in TOOLS {
        let mut args = minimal_args(tool);
        args["path"] = json!("src");
        let message = error_message(&session.call(tool, args));
        assert!(message.contains("must be absolute"), "{tool}: {message}");
        assert!(message.contains("`src`"), "{tool}: {message}");
    }
}

#[test]
fn nonexistent_absolute_path_is_rejected() {
    let cwd = tempdir().expect("tempdir");
    let missing = cwd.path().join("no-such-crate");
    let mut session = Session::start(cwd.path());
    let message = error_message(&session.call(
        "rustgraph_find",
        json!({ "query": "main", "path": missing.to_str().expect("utf-8 path") }),
    ));
    assert!(message.contains("does not exist"), "{message}");
}

#[test]
fn absolute_path_works_from_unrelated_cwd() {
    let cwd = tempdir().expect("tempdir");
    let mut session = Session::start(cwd.path());
    let response = session.call(
        "rustgraph_find",
        json!({ "query": "serve_stdio", "path": env!("CARGO_MANIFEST_DIR"), "kind": "func" }),
    );
    let result = &response["result"];
    assert_ne!(result["isError"], json!(true), "{response}");
    let text = result["content"][0]["text"].as_str().expect("text content");
    assert!(text.contains("src/mcp.rs"), "{text}");

    let tree = session.call(
        "rustgraph_tree",
        json!({ "path": env!("CARGO_MANIFEST_DIR"), "prefix": "src/app/render", "files_only": true }),
    );
    let text = tree["result"]["content"][0]["text"]
        .as_str()
        .expect("tree text");
    assert!(text.contains("callers.rs"), "{text}");
}
