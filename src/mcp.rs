

use std::process::Command;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindArgs {
    /// Symbol name; `a|b` for OR.
    pub query: String,
    /// Crate root (defaults to cwd).
    #[serde(default)]
    pub path: Option<String>,
    /// Kind filter: `func`, `struct`, or `enum`.
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CallersArgs {
    /// Function name, `path.rs:LINE`, or symbol id.
    pub target: String,
    /// Crate root (defaults to cwd).
    #[serde(default)]
    pub path: Option<String>,
    /// Transitive depth: 1 = direct, 0 = unlimited.
    #[serde(default)]
    pub depth: Option<u32>,
    /// Restrict callers to files containing this substring.
    #[serde(default)]
    pub callers_in: Option<String>,
    /// Flat `path:line:name` list (vs tree).
    #[serde(default)]
    pub flat: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EnsembleArgs {
    /// Function name, `path.rs:LINE`, or symbol id.
    pub target: String,
    /// Crate root (defaults to cwd).
    #[serde(default)]
    pub path: Option<String>,
    /// View: `summary` (default), `usage`, `flow`, `full`.
    #[serde(default)]
    pub view: Option<String>,
    /// Preset: `quick`, `balanced` (default), `deep`.
    #[serde(default)]
    pub preset: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PathsBetweenArgs {
    /// Source fn (name, `path.rs:LINE`, or symbol id).
    pub from: String,
    /// Target fn (name, `path.rs:LINE`, or symbol id).
    pub to: String,
    /// Crate root (defaults to cwd).
    #[serde(default)]
    pub path: Option<String>,
    /// Max paths (default 8, 0 = unlimited).
    #[serde(default)]
    pub max_results: Option<u32>,
    /// Annotate each hop with the call-site line.
    #[serde(default)]
    pub show_call_sites: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SliceArgs {
    /// Name, `path.rs:LINE`, or `path.rs:START-END`.
    pub query: String,
    /// Crate root (defaults to cwd).
    #[serde(default)]
    pub path: Option<String>,
    /// Lines of context above/below the slice.
    #[serde(default)]
    pub context: Option<u32>,
}

/// MCP server that exposes the five core rustgraph tools over stdio JSON-RPC.
///
/// Each tool is implemented by building a `rustgraph` CLI argv and spawning
/// the current executable as a subprocess, so the server is always in sync
/// with the installed binary version.
#[derive(Debug, Clone)]
pub struct RustgraphServer {
    tool_router: ToolRouter<Self>,
    binary: Arc<String>,
}

impl Default for RustgraphServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router(router = tool_router)]
impl RustgraphServer {
    /// Create a new server instance, resolving the binary path from
    /// `std::env::current_exe` so the spawned subprocess is the same binary
    /// that is serving.
    pub fn new() -> Self {
        let binary = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "rustgraph".to_string());
        Self {
            tool_router: Self::tool_router(),
            binary: Arc::new(binary),
        }
    }


    #[tool(
        name = "rustgraph_find",
        description = "Use INSTEAD OF Grep for 'where is X' / 'find fn|struct X'. Returns file:line + signature. Doesn't match comments or strings."
    )]
    async fn find(&self, p: Parameters<FindArgs>) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut argv: Vec<String> = Vec::new();
        if let Some(path) = &p.0.path {
            argv.push("-p".into());
            argv.push(path.clone());
        }
        argv.push("find".into());
        argv.push(p.0.query.clone());
        if let Some(kind) = &p.0.kind {
            match kind.as_str() {
                "func" => argv.push("--func".into()),
                "struct" => argv.push("--struct".into()),
                "enum" => argv.push("--enum".into()),
                _ => {}
            }
        }
        Ok(run_rustgraph(&self.binary, &argv))
    }


    #[tool(
        name = "rustgraph_callers",
        description = "Use INSTEAD OF Grep for 'who calls X' / 'what depends on X'. Returns caller tree with call-site lines. depth:0 = full transitive. Handles Type::method overload collisions."
    )]
    async fn callers(
        &self,
        p: Parameters<CallersArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut argv: Vec<String> = Vec::new();
        if let Some(path) = &p.0.path {
            argv.push("-p".into());
            argv.push(path.clone());
        }
        argv.push("callers".into());
        argv.push(p.0.target.clone());
        if let Some(d) = p.0.depth {
            argv.push("--depth".into());
            argv.push(d.to_string());
        }
        if let Some(needle) = &p.0.callers_in {
            argv.push("--callers-in".into());
            argv.push(needle.clone());
        }
        if p.0.flat == Some(true) {
            argv.push("--flat".into());
        }
        Ok(run_rustgraph(&self.binary, &argv))
    }


    #[tool(
        name = "rustgraph_ensemble",
        description = "Use INSTEAD OF 5+ Read or rustgraph_slice calls to UNDERSTAND a function. ONE call returns callers + callees + structs touched + dataflow. ~10× fewer tool calls than reading manually. Triggers: 'explain X' / 'how does X work'."
    )]
    async fn ensemble(
        &self,
        p: Parameters<EnsembleArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut argv: Vec<String> = Vec::new();
        if let Some(path) = &p.0.path {
            argv.push("-p".into());
            argv.push(path.clone());
        }
        argv.push("ensemble".into());
        argv.push(p.0.target.clone());
        if let Some(v) = &p.0.view {
            argv.push("--view".into());
            argv.push(v.clone());
        }
        if let Some(pre) = &p.0.preset {
            argv.push("--preset".into());
            argv.push(pre.clone());
        }
        Ok(run_rustgraph(&self.binary, &argv))
    }


    #[tool(
        name = "rustgraph_paths_between",
        description = "Use INSTEAD OF manual tracing for 'walk me through' / 'trace flow' / 'does A reach B'. Enumerates call-graph paths with file:line per hop. Deterministic; better than guessing."
    )]
    async fn paths_between(
        &self,
        p: Parameters<PathsBetweenArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut argv: Vec<String> = Vec::new();
        if let Some(path) = &p.0.path {
            argv.push("-p".into());
            argv.push(path.clone());
        }
        argv.push("paths-between".into());
        argv.push(p.0.from.clone());
        argv.push(p.0.to.clone());
        if let Some(n) = p.0.max_results {
            argv.push("--max-results".into());
            argv.push(n.to_string());
        }
        if p.0.show_call_sites == Some(true) {
            argv.push("--show-call-sites".into());
        }
        Ok(run_rustgraph(&self.binary, &argv))
    }


    #[tool(
        name = "rustgraph_slice",
        description = "Use INSTEAD OF Read for 'show me X' / 'source of X'. Accepts name, path:LINE, or path:START-END. NOTE: if you also need callers/callees/structs, use rustgraph_ensemble — saves multiple round-trips."
    )]
    async fn slice(&self, p: Parameters<SliceArgs>) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut argv: Vec<String> = Vec::new();
        if let Some(path) = &p.0.path {
            argv.push("-p".into());
            argv.push(path.clone());
        }
        argv.push("slice".into());
        argv.push(p.0.query.clone());
        if let Some(c) = p.0.context {
            argv.push("-C".into());
            argv.push(c.to_string());
        }
        Ok(run_rustgraph(&self.binary, &argv))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RustgraphServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "rustgraph".into(),
                title: Some("rustgraph — AST-aware Rust codebase navigation".into()),
                version: env!("CARGO_PKG_VERSION").into(),
                website_url: Some("https://github.com/ZmoleCristian/rustgraph".into()),
                icons: None,
            },
            instructions: Some(
                "Rust nav for this codebase. PREFER these over Grep/Read for any Rust \
                 symbol / function / call-chain question. AST-resolved (no false-match on \
                 comments/strings) and one call here saves 4-10 Grep+Read cycles.\n\n\
                 When you see:\n\
                 \x20 'where is X'                          → rustgraph_find\n\
                 \x20 'who calls X'                         → rustgraph_callers\n\
                 \x20 'understand X' / 'how does X work'    → rustgraph_ensemble  (one call > 5 Reads)\n\
                 \x20 'walk me through' / 'trace flow'      → rustgraph_paths_between\n\
                 \x20 'show me X' / 'source of X'           → rustgraph_slice"
                    .into(),
            ),
        }
    }
}

fn run_rustgraph(binary: &str, args: &[String]) -> CallToolResult {
    let output = Command::new(binary).args(args).output();
    match output {
        Ok(out) => {
            let mut combined = String::new();
            if !out.stdout.is_empty() {
                combined.push_str(&String::from_utf8_lossy(&out.stdout));
            }
            if !out.stderr.is_empty() {
                if !combined.is_empty() {
                    combined.push_str("\n--- stderr ---\n");
                }
                combined.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            if combined.is_empty() {
                combined.push_str("(no output)");
            }
            if out.status.success() {
                CallToolResult::success(vec![Content::text(combined)])
            } else {
                CallToolResult::error(vec![Content::text(combined)])
            }
        }
        Err(e) => CallToolResult::error(vec![Content::text(format!(
            "failed to spawn rustgraph subprocess: {}",
            e
        ))]),
    }
}

/// Start the MCP server on stdin/stdout and wait until the client disconnects.
///
/// Called by `main` when `rustgraph mcp` is invoked with no sub-action.
pub async fn serve_stdio() -> anyhow::Result<()> {
    let server = RustgraphServer::new();
    let service = server
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await?;
    service.waiting().await?;
    Ok(())
}
