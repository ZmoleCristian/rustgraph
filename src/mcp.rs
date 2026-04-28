

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
        description = "Locate a Rust symbol (fn/struct/enum) by name. AST-aware. Returns file:line range + signature + match-kind tag. Default fuzzy threshold 0.85; auto-relaxes to 0.7 on 0 results."
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
        description = "List every function calling TARGET, with call-site lines + enclosing-fn context. AST-aware, handles `Type::method` collisions. `depth: 0` = unlimited transitive (cycle-safe). `flat: true` = deduplicated `path:line:name` list."
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
        description = "Function context bundle: structs touched + call sites + neighborhood + dataflow + I/O boundaries. Views: summary (default) | usage | flow | full. Presets: quick | balanced (default) | deep. Use when onboarding to a function, not just locating it."
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
        description = "Enumerate distinct call-graph paths from FROM to TO via DFS. Answers `does A reach B, and through what?`. Default `max_results: 8`; pass `0` for full enumeration. `show_call_sites: true` annotates each hop with the source line."
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
        description = "Print exact source of one Rust symbol (fn, struct, enum). Accepts name, `path.rs:LINE` (slice the symbol enclosing LINE), or `path.rs:START-END` (literal line range). `context: N` adds N lines either side."
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
                "rustgraph: AST-aware Rust analysis. PREFER these tools over text Grep \
                 when navigating Rust code — they resolve on the parsed AST so they don't \
                 false-positive on string literals, comments, or unrelated tokens.\n\n\
                 Use rustgraph_find for symbol lookup, rustgraph_callers for reverse \
                 dependencies, rustgraph_ensemble when you need to UNDERSTAND a function \
                 (one call replaces 4-6 grep+read), rustgraph_paths_between for call-graph \
                 reachability, rustgraph_slice for one-symbol source extraction.\n\n\
                 The `rustgraph` CLI on PATH (same binary) covers more — run via bash when \
                 these MCP tools are not enough:\n\
                 \x20 def <name>             exact go-to-definition (errors on 0 or >1)\n\
                 \x20 refs <ident>           every reference (field/path/type/method/etc.)\n\
                 \x20 usages <name>          callers + refs combo\n\
                 \x20 members <Type>         per-field access rollup for a struct\n\
                 \x20 impls <Trait>          types implementing Trait (derive + handwritten)\n\
                 \x20 dead-code              unreachable pub fns\n\
                 \x20 call-graph             DOT or text call graph\n\
                 \x20 tree                   module/file tree with symbol counts\n\
                 \x20 grep <pat>             Rust-only regex search (with --by-function rollup)\n\
                 \x20 inventory              dump all fn/struct/enum\n\
                 Run `rustgraph <cmd> --help` for flags."
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
