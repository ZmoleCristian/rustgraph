

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// Strongly-typed `kind` filter for `rustgraph_find`.
///
/// Used as the JSON-Schema enum so the MCP client gets validation errors
/// up front rather than the server silently dropping unknown values.
// NOTE: variants intentionally carry NO doc comments. schemars renders
// per-variant docs as `oneOf: [{const, description}, ...]`, which strict tool-
// schema validators (e.g. the Anthropic API) reject. Doc-free unit variants
// serialize to a flat `enum: [...]`. Describe the values on the *field* instead.
#[derive(Debug, Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum FindKind {
    Func,
    Struct,
    Enum,
    Const,
    Trait,
    Alias,
}

impl FindKind {
    fn flag(self) -> &'static str {
        match self {
            FindKind::Func => "--func",
            FindKind::Struct => "--struct",
            FindKind::Enum => "--enum",
            FindKind::Const => "--const",
            FindKind::Trait => "--trait",
            FindKind::Alias => "--alias",
        }
    }
}

/// Strongly-typed `kind` filter for `rustgraph_usages`.
///
/// Variants are doc-free on purpose — see the note on [`FindKind`]; value
/// meanings live on the `kind` field of [`UsagesArgs`].
#[derive(Debug, Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Call,
    Method,
    Variant,
    AssocCall,
    Struct,
    Pattern,
    Type,
    Field,
    Path,
    Macro,
}

impl RefKind {
    fn as_cli(self) -> &'static str {
        match self {
            RefKind::Call => "call",
            RefKind::Method => "method",
            RefKind::Variant => "variant",
            RefKind::AssocCall => "assoc_call",
            RefKind::Struct => "struct",
            RefKind::Pattern => "pattern",
            RefKind::Type => "type",
            RefKind::Field => "field",
            RefKind::Path => "path",
            RefKind::Macro => "macro",
        }
    }
}

/// Strongly-typed `view` selector for `rustgraph_ensemble`.
///
/// Variants are doc-free on purpose — see the note on [`FindKind`]; value
/// meanings live on the `view` field of [`EnsembleArgs`].
#[derive(Debug, Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum EnsembleView {
    Summary,
    Usage,
    Flow,
    Full,
}

impl EnsembleView {
    fn as_cli(self) -> &'static str {
        match self {
            EnsembleView::Summary => "summary",
            EnsembleView::Usage => "usage",
            EnsembleView::Flow => "flow",
            EnsembleView::Full => "full",
        }
    }
}

/// Strongly-typed `preset` selector for `rustgraph_ensemble`.
///
/// Variants are doc-free on purpose — see the note on [`FindKind`]; the cap
/// details live on the `preset` field of [`EnsembleArgs`].
#[derive(Debug, Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum EnsemblePreset {
    Quick,
    Balanced,
    Deep,
}

impl EnsemblePreset {
    fn as_cli(self) -> &'static str {
        match self {
            EnsemblePreset::Quick => "quick",
            EnsemblePreset::Balanced => "balanced",
            EnsemblePreset::Deep => "deep",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindArgs {
    /// Symbol name; `a|b` for OR. Single identifiers only — no phrases.
    pub query: String,
    /// Crate root (defaults to cwd).
    #[serde(default)]
    pub path: Option<String>,
    /// Kind filter: `func`, `struct`, `enum`, `const` (consts + statics), `trait`, or `alias` (type aliases).
    #[serde(default)]
    pub kind: Option<FindKind>,
    /// Fuzzy threshold in [0,1] (default 0.85; lower = looser matching).
    #[serde(default)]
    pub threshold: Option<f64>,
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
    /// View: `summary` (default, concise), `usage` (caller/use-site), `flow` (dataflow), `full` (all three).
    #[serde(default)]
    pub view: Option<EnsembleView>,
    /// Preset caps: `quick` (tight — 80 call sites, depth 1, structs + small sample, cheapest),
    /// `balanced` (default — 200 sites, depth 2, full callers+callees+structs+dataflow bundle),
    /// `deep` (generous — unlimited sites, depth 3, wide transitive coverage for lifecycle audits).
    #[serde(default)]
    pub preset: Option<EnsemblePreset>,
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
    /// Maximum path length in nodes (default 8, 0 = unlimited).
    #[serde(default)]
    pub depth: Option<u32>,
    /// Max paths (default 10, 0 = unlimited).
    #[serde(default)]
    pub max_results: Option<u32>,
    /// Maximum path states examined (default 100000, 0 = unlimited).
    #[serde(default)]
    pub max_expansions: Option<u32>,
    /// Annotate each hop with the call-site line.
    #[serde(default)]
    pub show_call_sites: Option<bool>,
    /// Drop test functions from endpoints and traversal.
    #[serde(default)]
    pub exclude_tests: Option<bool>,
    /// Request conservative resolution explicitly (paths-between is conservative by default).
    #[serde(default)]
    pub strict_resolution: Option<bool>,
    /// Subprocess deadline in milliseconds (default 30000; clamped to 100..300000).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UsagesArgs {
    /// Exact identifier: type, field, or fn name (single ident — not a path or phrase).
    pub target: String,
    /// Crate root (defaults to cwd).
    #[serde(default)]
    pub path: Option<String>,
    /// Restrict results to files whose path contains this substring.
    #[serde(default)]
    pub in_path: Option<String>,
    /// Filter the refs section by classification: `struct` (literals/construction), `type`
    /// (ascriptions: `Vec<X>`, params, returns, impl blocks), `pattern` (match arms,
    /// destructuring), `field` (`.field` reads/writes — pass the FIELD name as target),
    /// `variant`, `assoc_call`, `method`, `call`, `path`, `macro` (unparseable macro body,
    /// best-effort). Omit for all kinds. Callers section is unaffected.
    #[serde(default)]
    pub kind: Option<Vec<RefKind>>,
    /// Cap on usages emitted (default 100; 0 = unlimited).
    #[serde(default)]
    pub max_results: Option<u32>,
    /// Drop hits whose enclosing fn is #[test]. Default false — when adding a struct field
    /// you WANT the test literals too.
    #[serde(default)]
    pub exclude_tests: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TreeArgs {
    /// Crate root (defaults to cwd).
    #[serde(default)]
    pub path: Option<String>,
    /// Optional path prefix (relative to project root) to limit the tree to a subtree, e.g. `src/governor`.
    #[serde(default)]
    pub prefix: Option<String>,
    /// Hide symbol counts; show files only.
    #[serde(default)]
    pub files_only: Option<bool>,
}

/// MCP server that exposes the six core rustgraph tools over stdio JSON-RPC.
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
        description = "Use INSTEAD OF Grep for 'where is X' / 'find fn|struct|enum|const|trait|alias X'. Returns file:line + signature. Doesn't match comments or strings."
    )]
    async fn find(&self, p: Parameters<FindArgs>) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut argv: Vec<String> = Vec::new();
        if let Some(path) = &p.0.path {
            argv.push("-p".into());
            argv.push(path.clone());
        }
        if let Some(t) = p.0.threshold {
            argv.push("--search-threshold".into());
            argv.push(t.to_string());
        }
        argv.push("find".into());
        argv.push(p.0.query.clone());
        if let Some(kind) = p.0.kind {
            argv.push(kind.flag().into());
        }
        Ok(run_rustgraph(&self.binary, &argv).await)
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
        Ok(run_rustgraph(&self.binary, &argv).await)
    }


    #[tool(
        name = "rustgraph_ensemble",
        description = "Use INSTEAD OF 5+ Read calls to UNDERSTAND a function. With the default `balanced` preset ONE call returns callers + callees + structs touched + dataflow (~10× fewer tool calls than reading manually). The `quick` preset trades coverage for tokens — pass it only when you already know you just want the structs and a tiny callee sample. Triggers: 'explain X' / 'how does X work'."
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
        if let Some(v) = p.0.view {
            argv.push("--view".into());
            argv.push(v.as_cli().into());
        }
        if let Some(pre) = p.0.preset {
            argv.push("--preset".into());
            argv.push(pre.as_cli().into());
        }
        Ok(run_rustgraph(&self.binary, &argv).await)
    }


    #[tool(
        name = "rustgraph_usages",
        description = "Use INSTEAD OF Grep for 'where is TYPE X used' / 'who constructs X' / 'what breaks if I add a field'. Returns every AST-resolved reference site: struct literals, type ascriptions (Vec<X>, fn params/returns, impl blocks), pattern matches, field accesses, assoc/method calls — INCLUDING inside macro bodies (vec!, assert_eq!). Complement to rustgraph_callers (fn-call edges only): callers can't target a struct/enum/field, this can. For a fn target it also bundles the caller list. Pass the FIELD name as target for field read/write sites."
    )]
    async fn usages(
        &self,
        p: Parameters<UsagesArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut argv: Vec<String> = Vec::new();
        if let Some(path) = &p.0.path {
            argv.push("-p".into());
            argv.push(path.clone());
        }
        if p.0.exclude_tests == Some(true) {
            argv.push("--exclude-tests".into());
        }
        argv.push("usages".into());
        argv.push(p.0.target.clone());
        if let Some(needle) = &p.0.in_path {
            argv.push("--in".into());
            argv.push(needle.clone());
        }
        for kind in p.0.kind.iter().flatten() {
            argv.push("--kind".into());
            argv.push(kind.as_cli().into());
        }
        if let Some(n) = p.0.max_results {
            argv.push("--max-results".into());
            argv.push(n.to_string());
        }
        Ok(run_rustgraph(&self.binary, &argv).await)
    }


    #[tool(
        name = "rustgraph_paths_between",
        description = "Use INSTEAD OF manual tracing for 'walk me through' / 'trace flow' / 'does A reach B'. Enumerates shortest-first call-graph paths with exact call-site lines, reverse-reachability pruning, and hard exploration/deadline bounds."
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
        if p.0.exclude_tests == Some(true) {
            argv.push("--exclude-tests".into());
        }
        if p.0.strict_resolution == Some(true) {
            argv.push("--strict-resolution".into());
        }
        argv.push("paths-between".into());
        argv.push(p.0.from.clone());
        argv.push(p.0.to.clone());
        if let Some(depth) = p.0.depth {
            argv.push("--depth".into());
            argv.push(depth.to_string());
        }
        if let Some(n) = p.0.max_results {
            argv.push("--max-results".into());
            argv.push(n.to_string());
        }
        if let Some(n) = p.0.max_expansions {
            argv.push("--max-expansions".into());
            argv.push(n.to_string());
        }
        if p.0.show_call_sites == Some(true) {
            argv.push("--show-call-sites".into());
        }
        let timeout_ms = p
            .0
            .timeout_ms
            .unwrap_or(PATHS_BETWEEN_TIMEOUT_MS)
            .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS);
        Ok(run_rustgraph_with_timeout(&self.binary, &argv, timeout_ms).await)
    }


    #[tool(
        name = "rustgraph_tree",
        description = "Use INSTEAD OF ls/find/tree for 'what's in this module' / 'show project layout' / 'what files are under src/X'. Returns the module/file tree with per-file fn/struct/enum counts. Pass `prefix` (e.g. `src/governor`) to scope to a subtree. Topology view, not symbol search — use this when you DON'T yet know the symbol name."
    )]
    async fn tree(&self, p: Parameters<TreeArgs>) -> Result<CallToolResult, rmcp::ErrorData> {
        let mut argv: Vec<String> = Vec::new();
        if let Some(path) = &p.0.path {
            argv.push("-p".into());
            argv.push(path.clone());
        }
        argv.push("tree".into());
        if let Some(prefix) = &p.0.prefix {
            argv.push(prefix.clone());
        }
        if p.0.files_only == Some(true) {
            argv.push("--files-only".into());
        }
        Ok(run_rustgraph(&self.binary, &argv).await)
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
                 \x20 'who calls X' (X = fn)                → rustgraph_callers\n\
                 \x20 'where is TYPE/field X used' / 'who constructs X' / 'what breaks if I add a field' → rustgraph_usages\n\
                 \x20 'understand X' / 'how does X work'    → rustgraph_ensemble  (one call > 5 Reads)\n\
                 \x20 'walk me through' / 'trace flow'      → rustgraph_paths_between\n\
                 \x20 'show me X' / 'read source of X'      → use the Read tool (rustgraph_find gives you the path:line to Read)\n\
                 \x20 'what's in module X' / 'project layout' / unknown symbol name → rustgraph_tree (replaces ls/find/tree)"
                    .into(),
            ),
        }
    }
}

/// Run the `rustgraph` CLI as a subprocess and translate its output into a
/// structured `CallToolResult`.
///
/// On success: return stdout as a text content block (no marker, no stderr).
/// On failure: return a structured error whose `structured_content` carries
/// stdout, stderr, exit code, and (on Unix) the terminating signal so MCP
/// clients can introspect the failure programmatically. The text content
/// block remains human-readable for clients that ignore structured fields.
const DEFAULT_MCP_TIMEOUT_MS: u64 = 60_000;
const PATHS_BETWEEN_TIMEOUT_MS: u64 = 30_000;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 300_000;

async fn run_rustgraph(binary: &str, args: &[String]) -> CallToolResult {
    run_rustgraph_with_timeout(binary, args, DEFAULT_MCP_TIMEOUT_MS).await
}

/// Spawn a cancellation-safe child with a wall-clock deadline.
///
/// `kill_on_drop(true)` is important here: cancelling an MCP request drops
/// this future, which in turn terminates the CLI child instead of leaving a
/// detached CPU-bound process behind.
async fn run_rustgraph_with_timeout(
    binary: &str,
    args: &[String],
    timeout_ms: u64,
) -> CallToolResult {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return spawn_failure(binary, args, &error),
    };

    let output = match tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let payload = json!({
                "kind": "rustgraph_wait_failure",
                "summary": format!("failed while waiting for rustgraph subprocess: {error}"),
                "binary": binary,
                "argv": args,
                "io_error_kind": format!("{:?}", error.kind()),
            });
            log_misuse(args, "wait_failure", &payload);
            return CallToolResult::structured_error(payload);
        }
        Err(_) => {
            let payload = json!({
                "kind": "rustgraph_subprocess_timeout",
                "summary": format!("rustgraph exceeded the {timeout_ms} ms MCP deadline and was terminated"),
                "binary": binary,
                "argv": args,
                "timeout_ms": timeout_ms,
            });
            log_misuse(args, "subprocess_timeout", &payload);
            return CallToolResult::structured_error(payload);
        }
    };
    translate_output(args, output)
}

fn spawn_failure(binary: &str, args: &[String], error: &std::io::Error) -> CallToolResult {
    let payload = json!({
        "kind": "rustgraph_spawn_failure",
        "summary": format!("failed to spawn rustgraph subprocess: {error}"),
        "binary": binary,
        "argv": args,
        "io_error_kind": format!("{:?}", error.kind()),
    });
    log_misuse(args, "spawn_failure", &payload);
    CallToolResult::structured_error(payload)
}

fn translate_output(args: &[String], output: std::process::Output) -> CallToolResult {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if output.status.success() {
        let body = if stdout.is_empty() {
            log_misuse(args, "empty", &json!({}));
            "(no output)".to_string()
        } else {
            stdout
        };
        CallToolResult::success(vec![Content::text(body)])
    } else {
        let exit_code = output.status.code();
        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt;
            output.status.signal()
        };
        #[cfg(not(unix))]
        let signal: Option<i32> = None;

        let summary = match (exit_code, signal) {
            (Some(code), _) => format!("rustgraph exited with status {code}"),
            (None, Some(signal)) => format!("rustgraph terminated by signal {signal}"),
            (None, None) => "rustgraph terminated abnormally".to_string(),
        };
        let payload = json!({
            "kind": "rustgraph_subprocess_failure",
            "summary": summary,
            "argv": args,
            "exit_code": exit_code,
            "signal": signal,
            "stdout": stdout,
            "stderr": stderr,
        });
        log_misuse(args, "subprocess_failure", &payload);
        CallToolResult::structured_error(payload)
    }
}

/// Opt-in misuse logger. No-op unless `RUSTGRAPH_MISUSE_LOG` points at a file.
/// Appends one JSON line per failed/zero-hit MCP call so the operator can mine
/// where agents misfire across projects. The env var path *is* the on switch;
/// unset = dead path, every install logs nothing. Fully best-effort: any IO or
/// clock error is swallowed so logging can never break a tool call.
fn log_misuse(args: &[String], outcome: &str, detail: &serde_json::Value) {
    let Ok(path) = std::env::var("RUSTGRAPH_MISUSE_LOG") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(String::from));
    let record = json!({
        "ts": ts,
        "version": env!("CARGO_PKG_VERSION"),
        "cwd": cwd,
        "outcome": outcome,
        "args": args,
        "detail": detail,
    });
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{record}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_text(content: &[Content]) -> String {
        content
            .iter()
            .filter_map(|c| c.as_text().map(|t| t.text.clone()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn arg_schema_json<T: JsonSchema>() -> String {
        let schema = schemars::SchemaGenerator::default().into_root_schema_for::<T>();
        serde_json::to_string(&schema).expect("serialize schema")
    }

    /// Regression guard: the Anthropic tool-schema validator rejects `oneOf`
    /// ("'oneOf' is not permitted"). schemars emits `oneOf` for enum variants
    /// that carry doc comments, so every tool-arg enum must keep its variants
    /// doc-free (describe values on the field). This test fails the moment a
    /// `oneOf` reappears in any tool's input schema.
    #[test]
    fn no_tool_arg_schema_uses_oneof() {
        let schemas = [
            ("FindArgs", arg_schema_json::<FindArgs>()),
            ("CallersArgs", arg_schema_json::<CallersArgs>()),
            ("EnsembleArgs", arg_schema_json::<EnsembleArgs>()),
            ("PathsBetweenArgs", arg_schema_json::<PathsBetweenArgs>()),
            ("TreeArgs", arg_schema_json::<TreeArgs>()),
            ("UsagesArgs", arg_schema_json::<UsagesArgs>()),
        ];
        for (name, json) in schemas {
            assert!(
                !json.contains("oneOf"),
                "{name} input schema must not contain `oneOf` (strict validators reject it): {json}"
            );
        }
    }

    #[test]
    fn paths_between_args_accept_all_search_guards() {
        let args: PathsBetweenArgs = serde_json::from_value(json!({
            "from": "main",
            "to": "splice",
            "depth": 8,
            "max_results": 3,
            "max_expansions": 5000,
            "show_call_sites": true,
            "exclude_tests": true,
            "strict_resolution": true,
            "timeout_ms": 2500
        }))
        .expect("valid paths-between MCP arguments");
        assert_eq!(args.depth, Some(8));
        assert_eq!(args.max_results, Some(3));
        assert_eq!(args.max_expansions, Some(5000));
        assert_eq!(args.show_call_sites, Some(true));
        assert_eq!(args.exclude_tests, Some(true));
        assert_eq!(args.strict_resolution, Some(true));
        assert_eq!(args.timeout_ms, Some(2500));
    }

    #[tokio::test]
    async fn run_rustgraph_spawn_failure_is_structured_error() {
        // Use a path that cannot exist as an executable.
        let result = run_rustgraph(
            "/nonexistent/no-such-binary-rustgraph-test",
            &["find".to_string(), "x".to_string()],
        )
        .await;
        assert_eq!(result.is_error, Some(true), "must be flagged as error");
        let payload = result
            .structured_content
            .as_ref()
            .expect("structured payload required");
        assert_eq!(
            payload.get("kind").and_then(|v| v.as_str()),
            Some("rustgraph_spawn_failure")
        );
        let argv = payload
            .get("argv")
            .and_then(|v| v.as_array())
            .expect("argv array");
        assert_eq!(argv.len(), 2);
    }

    #[test]
    fn find_kind_serde_accepts_lowercase_strings() {
        let v: FindKind = serde_json::from_str("\"func\"").unwrap();
        assert!(matches!(v, FindKind::Func));
        let v: FindKind = serde_json::from_str("\"struct\"").unwrap();
        assert!(matches!(v, FindKind::Struct));
        let v: FindKind = serde_json::from_str("\"enum\"").unwrap();
        assert!(matches!(v, FindKind::Enum));
        let v: FindKind = serde_json::from_str("\"const\"").unwrap();
        assert!(matches!(v, FindKind::Const));
        assert_eq!(FindKind::Const.flag(), "--const");
        let v: FindKind = serde_json::from_str("\"trait\"").unwrap();
        assert!(matches!(v, FindKind::Trait));
        assert_eq!(FindKind::Trait.flag(), "--trait");
        let v: FindKind = serde_json::from_str("\"alias\"").unwrap();
        assert!(matches!(v, FindKind::Alias));
        assert_eq!(FindKind::Alias.flag(), "--alias");
    }

    #[test]
    fn ref_kind_serde_accepts_cli_value_set() {
        // Every CLI --kind value must deserialize, and round-trip back to the same flag.
        for s in crate::cli::REFS_KIND_VALUES {
            let raw = format!("\"{s}\"");
            let v: RefKind = serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("MCP RefKind must accept CLI kind '{s}': {e}"));
            assert_eq!(&v.as_cli(), s, "as_cli must round-trip '{s}'");
        }
    }

    #[test]
    fn ref_kind_serde_rejects_unknown_values() {
        let err = serde_json::from_str::<RefKind>("\"banana\"").err();
        assert!(err.is_some(), "unknown ref kinds must be rejected");
    }

    #[test]
    fn find_kind_serde_rejects_unknown_values() {
        let err = serde_json::from_str::<FindKind>("\"banana\"").err();
        assert!(
            err.is_some(),
            "unknown variants must be rejected at deserialize time"
        );
    }

    #[test]
    fn ensemble_view_serde_accepts_documented_set() {
        for s in ["summary", "usage", "flow", "full"] {
            let raw = format!("\"{s}\"");
            let _v: EnsembleView =
                serde_json::from_str(&raw).expect("should accept documented view");
        }
    }

    #[test]
    fn ensemble_preset_serde_accepts_documented_set() {
        for s in ["quick", "balanced", "deep"] {
            let raw = format!("\"{s}\"");
            let _v: EnsemblePreset =
                serde_json::from_str(&raw).expect("should accept documented preset");
        }
    }

    #[test]
    fn ensemble_view_rejects_unknown() {
        let err = serde_json::from_str::<EnsembleView>("\"deep-dive\"").err();
        assert!(err.is_some(), "unknown view must fail deserialization");
    }

    #[test]
    fn ensemble_preset_rejects_unknown() {
        let err = serde_json::from_str::<EnsemblePreset>("\"slow\"").err();
        assert!(err.is_some(), "unknown preset must fail deserialization");
    }

    #[tokio::test]
    async fn run_rustgraph_subprocess_nonzero_is_structured_error() {
        // `false` exits 1 with no output on every Unix-like system. On Windows
        // this test is skipped because we don't ship `false`.
        if cfg!(not(unix)) {
            return;
        }
        let result = run_rustgraph("false", &[]).await;
        assert_eq!(result.is_error, Some(true));
        let payload = result
            .structured_content
            .as_ref()
            .expect("structured payload required");
        assert_eq!(
            payload.get("kind").and_then(|v| v.as_str()),
            Some("rustgraph_subprocess_failure")
        );
        assert!(payload.get("exit_code").is_some());
        assert!(payload.get("stderr").is_some());
        assert!(payload.get("stdout").is_some());
        // Ensure no machine-unfriendly stderr marker leaks back into the human text.
        let text = extract_text(&result.content);
        assert!(
            !text.contains("--- stderr ---"),
            "old marker should not appear in restructured output: {text}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_rustgraph_timeout_returns_structured_error_promptly() {
        let started = std::time::Instant::now();
        let result = run_rustgraph_with_timeout("sleep", &["5".to_string()], 100).await;
        assert_eq!(result.is_error, Some(true));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timeout must not wait for the child to finish"
        );
        let payload = result
            .structured_content
            .as_ref()
            .expect("structured payload required");
        assert_eq!(
            payload.get("kind").and_then(|value| value.as_str()),
            Some("rustgraph_subprocess_timeout")
        );
        assert_eq!(
            payload.get("timeout_ms").and_then(|value| value.as_u64()),
            Some(100)
        );
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
