use clap::builder::PossibleValuesParser;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::api::{CallGraphDetail, EnsemblePreset, EnsembleView};

/// Allowed values for the `--kind` filter on the `refs` subcommand.
///
/// Passed to [`clap::builder::PossibleValuesParser`] so the shell can
/// tab-complete them and clap can validate them at parse time.
pub const REFS_KIND_VALUES: &[&str] = &[
    "field",
    "path",
    "assoc_call",
    "variant",
    "type",
    "struct",
    "pattern",
    "method",
    "call",
];


const GLOBALS_FOOTER_SHORT: &str =
    "Globals: -p / --also / -o / -j / --search / --exclude-tests / --color … (run `rustgraph --help` for the full Globals reference)";


const GLOBALS_FOOTER_LONG: &str = "\
Globals (run `rustgraph --help` for full descriptions):
  Resolution: -p, --path <PATH> | --no-auto-path | --absolute-paths | --also <PATH>
              --strict-resolution   drop call edges where type-strict matching returned 0 candidates
                                    instead of falling back to bare-name match; see stderr for count
  Output:     -o, --output <FILE> | -j, --json | --include-ignored | --color | --no-color
  Search:     --search <Q> | --search-threshold <F> | --match-signature | --exclude-tests";

/// Inventory filter mode for the top-level `--analyze` flag.
///
/// Selects which symbol classes are listed when running without a subcommand.
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum AnalyzeMode {
    /// List functions only.
    Functions,
    /// List structs only.
    Structs,
    /// List enums only.
    Enums,
    /// List both structs and enums (excludes functions).
    Both,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ModeCommand {
    #[command(
        about = "List callers of a target function. Tip: use `-p ../sibling-crate callers <fn>` to look up callers in another crate.",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    Callers(CallersCommand),
    #[command(
        about = "Build an ensemble for a target function. Tip: use `-p ../sibling-crate ensemble <fn>` for cross-crate ensembles.",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    Ensemble(EnsembleCommand),
    #[command(
        name = "dead-code",
        visible_alias = "orphans",
        about = "Find dead public functions",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG
    )]
    DeadCode(DeadCodeCommand),
    #[command(
        name = "call-graph",
        about = "Generate a call graph. Default: indented text tree (terminal-friendly). Pass --dot for graphviz output.",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG
    )]
    CallGraph(CallGraphCommand),
    #[command(
        about = "Print the module/file tree of the project, with symbol counts. Replaces `ls`/`tree` for code navigation. Tip: use `-p ../sibling-crate tree` for another crate.",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG
    )]
    Tree(TreeCommand),
    #[command(
        about = "Text/regex search across Rust source files (comments, strings, attribute args — everything `--search` doesn't see). Tip: use `-p ../sibling-crate grep <pat>` for cross-crate search.",
        long_about = "Text/regex search across Rust source files. Catches everything `--search` doesn't (comments, string literals, attribute args).\n\n\
Regex flavor: Rust `regex` crate syntax (NOT PCRE). Key differences:\n  \
  • No backreferences (\\1) or lookarounds ((?=...), (?!...)).\n  \
  • Use \\b for word boundaries, (?i) inline for case-insensitive (or pass -i).\n  \
  • Escape regex meta with \\ (or pass -F to treat the whole pattern as literal).\n  \
  • Char classes: [abc], [^abc], \\d \\w \\s and friends.\n  \
  • Full syntax: https://docs.rs/regex/latest/regex/#syntax\n\n\
Tips:\n  \
  • `grep '\\.unwrap\\(\\)' --by-function` — token-saving rollup; emits ONLY per-fn counts.\n  \
  • `grep <pat> --enclosing-function -j` — per-line matches with enclosing-fn metadata; pipe to jq.\n  \
  • `-p ../sibling-crate grep <pat>` — cross-crate search.\n\n\
Shell quoting trap (most-common cause of cryptic regex errors):\n  \
  • ALWAYS single-quote the pattern: `grep '\\.unwrap\\(\\)'` — double quotes let the shell expand `$`/`!` and unescape backslashes.\n  \
  • Escape regex parens with `\\(` and `\\)`. `grep '.expect(...)'` is a regex parser error; use `grep '\\.expect\\(...\\)'`.\n  \
  • Escape literal dots with `\\.` if you don't want them to mean any char.\n  \
  • For literal patterns with no escaping, pass `-F`: `grep -F '.unwrap()'`.",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    Grep(GrepCommand),
    #[command(
        about = "List every type that implements <Trait>, both via #[derive(Trait)] and hand-written `impl Trait for X`. Resolves the trait name by its last path segment (so `Serialize` matches `serde::Serialize`).",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    Impls(ImplsCommand),
    #[command(
        about = "Short alias for symbol lookup. `find <name>` ≈ `--search <name> --search-threshold 0.85`. Auto-shows fns + structs + enums; muscle-memory replacement for `rg <name>`. Query is name-only or `<a>|<b>` alternatives — for `path.rs:LINE` lookup use `callers path.rs:LINE`.",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    Find(FindCommand),
    #[command(
        about = "Find every reference to <IDENT> in the codebase: type uses, field reads, struct construction, pattern matches, method calls, plain paths. Complement to `callers` (function-call edges only).",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    Refs(RefsCommand),
    #[command(
        name = "paths-between",
        visible_alias = "why",
        about = "DFS through the call graph to enumerate paths from FROM to TO. Answers `does A actually reach B, and through what?`. Both args accept fn name, path.rs:LINE, or full symbol_id.",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    PathsBetween(PathsBetweenCommand),
    #[command(
        about = "Combined `callers <name>` + `refs <name>`. One stop for `what touches this name`. Useful when you don't yet know if NAME is a fn (use callers) or a type/field (use refs).",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    Usages(UsagesCommand),
    #[command(
        about = "Go-to-definition: print exact symbol location (path:line:name). Exit 1 if 0 or >1 matches. Use this when `find` is too fuzzy and you want a single deterministic answer.",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    Def(DefCommand),


    #[command(
        about = "List every access site for each field of <Type>. Closes the chronic gap where `refs <Type> --kind field` returns 0 (since --kind field filters on the field NAME, not the OWNER type). For each field, emits `instance.field` access sites grouped by field. Use `refs <Type>` for type uses, or `refs <field_name> --kind field` for one specific field — `members <Type>` is the per-Type rollup.",
        after_help = GLOBALS_FOOTER_SHORT,
        after_long_help = GLOBALS_FOOTER_LONG,
        arg_required_else_help = true
    )]
    Members(MembersCommand),
    #[command(
        about = "Dump every fn/struct/enum in the project (parse-only baseline). Same as bare `rustgraph` with no subcommand + --func/--struct/--enum. Useful for piping to wc -l for symbol counts."
    )]
    Inventory(InventoryCommand),
    #[command(
        about = "MCP (Model Context Protocol) server + self-registration. Default = serve stdio JSON-RPC. Subcommands: install / uninstall / status to manage Claude/Codex/Gemini config registration.",
        subcommand_negates_reqs = true
    )]
    Mcp(McpCommand),
    #[command(
        about = "Emit a shell completion script for the chosen shell to stdout. Pipe to the canonical path: `rustgraph completions zsh > ~/.zsh/completions/_rustgraph`."
    )]
    Completions(CompletionsCommand),
    #[command(
        name = "generate-man",
        about = "Emit a clap-derived man page (roff) to stdout. Useful for cargo-install users who don't get the AUR-shipped curated man page. Hidden subcommand.",
        hide = true
    )]
    GenerateMan,
}

/// Arguments for the `inventory` subcommand (dump all symbols with optional
/// kind filter).
#[derive(clap::Args, Debug, Clone)]
pub struct InventoryCommand {
    #[arg(long, help = "Functions only")]
    pub func: bool,
    #[arg(long = "struct", help = "Structs only")]
    pub struct_only: bool,
    #[arg(long = "enum", help = "Enums only")]
    pub enum_only: bool,
}

/// Arguments for the `mcp` subcommand (MCP server + self-registration).
#[derive(clap::Args, Debug, Clone)]
pub struct McpCommand {
    #[command(subcommand)]
    pub action: Option<McpAction>,
}

/// Arguments for the `completions` subcommand (shell-completion generator).
#[derive(clap::Args, Debug, Clone)]
pub struct CompletionsCommand {
    #[arg(value_enum, help = "Target shell")]
    pub shell: clap_complete::Shell,
}

/// Sub-actions for the `mcp` subcommand.
#[derive(Subcommand, Debug, Clone)]
pub enum McpAction {
    #[command(
        about = "Detect installed AI clients (claude/codex/gemini) and register the rustgraph MCP server in each one's config. Idempotent. Atomic write with backup."
    )]
    Install {
        #[arg(long)]
        quiet: bool,
    },
    #[command(
        about = "Remove the rustgraph MCP entry from every detected AI client config."
    )]
    Uninstall {
        #[arg(long)]
        quiet: bool,
    },
    #[command(
        about = "List MCP registration state across detected AI clients (default action when `mcp` is invoked with no sub-action)."
    )]
    List,
    #[command(
        about = "Serve MCP over stdio JSON-RPC. Spawned by AI clients after registration; not for interactive human use."
    )]
    Serve,
}


/// Arguments for the `paths-between` subcommand (DFS call-graph reachability).
#[derive(clap::Args, Debug, Clone)]
pub struct PathsBetweenCommand {
    #[arg(help = "Source function (name, path.rs:LINE, or symbol_id)")]
    pub from: String,

    #[arg(
        required_unless_present = "to_module",
        help = "Target function (name, path.rs:LINE, or symbol_id). Optional when --to-module is set."
    )]
    pub to: Option<String>,

    #[arg(
        long,
        default_value_t = 8,
        help = "Maximum path length in nodes (0 = unlimited). Higher values explode combinatorially in dense graphs."
    )]
    pub depth: usize,

    #[arg(
        short = 'n',
        long = "max-results",
        default_value_t = 10,
        help = "Cap on the number of paths to emit (0 = unlimited)."
    )]
    pub max_results: usize,

    #[arg(
        long = "target-in",
        value_name = "PATH_SUBSTR",
        help = "TARGET filter: scope both FROM and TO candidate resolution by file_path SUBSTR."
    )]
    pub in_path: Option<String>,

    #[arg(
        long,
        help = "Annotate each hop with the source line where the next call happens. Pulled from project.call_sites; first lookup wins when a fn calls the same callee from multiple sites."
    )]
    pub show_call_sites: bool,

    #[arg(
        long = "to-module",
        value_name = "PATH_SUBSTR",
        help = "Blast-radius mode: search to ANY fn whose file_path contains SUBSTR (not just one TO). When set, the TO positional is optional. If both are given, the destination set is the UNION (matches either)."
    )]
    pub to_module: Option<String>,
}

/// Arguments for the `def` subcommand (go-to-definition: exact single-symbol
/// location).
#[derive(clap::Args, Debug, Clone)]
pub struct DefCommand {
    #[arg(help = "Symbol name (exact, case-sensitive)")]
    pub name: String,

    #[arg(long, help = "Restrict to functions only")]
    pub func: bool,

    #[arg(long = "struct", help = "Restrict to structs only")]
    pub struct_only: bool,

    #[arg(long = "enum", help = "Restrict to enums only")]
    pub enum_only: bool,


    #[arg(
        long = "in",
        value_name = "PATH_SUBSTR",
        help = "Filter candidates by file path substring (applied before the ambiguity check)."
    )]
    pub in_path: Option<String>,
}

/// Arguments for the `members` subcommand (per-field access-site rollup for a
/// struct type).
#[derive(clap::Args, Debug, Clone)]
pub struct MembersCommand {
    #[arg(help = "Struct type name (case-sensitive). For enums use `refs <Enum> --kind variant`.")]
    pub type_name: String,

    #[arg(
        long = "in",
        value_name = "PATH_SUBSTR",
        help = "FILE filter: only show field-access sites in files whose path contains SUBSTR. Mirrors refs/grep/dead-code/impls --in."
    )]
    pub in_path: Option<String>,

    #[arg(
        short = 'n',
        long = "max-results",
        value_name = "N",
        default_value_t = 50,
        help = "Cap on access sites rendered PER FIELD (default 50; 0 = unlimited). Each field's site list is truncated independently with a footer; field counts in the header stay un-capped."
    )]
    pub max_results: usize,
}

/// Arguments for the `usages` subcommand (combined callers + refs for an
/// identifier).
#[derive(clap::Args, Debug, Clone)]
pub struct UsagesCommand {
    #[arg(help = "Identifier to look up (matches fn names, type names, field names by exact match)")]
    pub name: String,

    #[arg(
        long = "in",
        value_name = "PATH_SUBSTR",
        help = "Filter both halves by file path substring."
    )]
    pub in_path: Option<String>,

    #[arg(
        short = 'n',
        long = "max-results",
        value_name = "N",
        default_value_t = 100,
        help = "Cap on usages emitted (default 100; 0 = unlimited). The combined callers + refs total can be dense; truncated output adds a footer noting the original count."
    )]
    pub max_results: usize,
}

/// Arguments for the `refs` subcommand (all reference sites for an
/// identifier: type uses, field reads, call expressions, pattern matches,
/// etc.).
#[derive(clap::Args, Debug, Clone)]
pub struct RefsCommand {
    #[arg(help = "Identifier to look up (matched by exact last-segment name)")]
    pub ident: String,

    #[arg(
        long,
        value_name = "KIND",
        value_parser = PossibleValuesParser::new(REFS_KIND_VALUES),
        help = "Filter by classification tag (multi-value). Default: all. Allowed: field | path | assoc_call | variant | type | struct | pattern | method | call. Repeat (`--kind field --kind path`) to allow multiple. `variant` = enum-variant qualifier uses (e.g. `Color::Red`); `assoc_call` = struct/type associated functions (e.g. `SessionDocument::load`); `method` = `.method()` calls regardless of receiver type. NOTE: unknown values error with the allowed list (was: silently 0 hits). NOTE: `assoc_call` and `variant` filter on the QUALIFIER (Type/Enum), NOT the leaf method/variant name — so `refs load --kind assoc_call` returns 0 because `load` is matched as the leaf, not as a Type qualifier. Query the TYPE instead: `refs SessionDocument --kind assoc_call`."
    )]
    pub kind: Vec<String>,

    #[arg(
        long = "in",
        value_name = "PATH_SUBSTR",
        help = "FILE filter: search only files whose path contains SUBSTR."
    )]
    pub in_path: Option<String>,

    #[arg(
        long,
        help = "Group references by enclosing function (similar to `grep --by-function`)."
    )]
    pub by_function: bool,

    #[arg(
        short = 'n',
        long = "max-results",
        value_name = "N",
        default_value_t = 50,
        help = "Cap on references emitted (default 50; 0 = unlimited). Header still shows the full count; the rendered list is truncated with a footer."
    )]
    pub max_results: usize,
}

/// Arguments for the `find` subcommand (fuzzy symbol lookup across
/// functions, structs, and enums).
#[derive(clap::Args, Debug, Clone)]
pub struct FindCommand {
    #[arg(
        help = "Symbol name only, or `<a>|<b>` alternatives (OR logic). Does NOT accept `path.rs:LINE` — for that, use `callers path.rs:LINE`, or open the path:line with the Read tool."
    )]
    pub query: String,

    #[arg(long, help = "Restrict to functions only")]
    pub func: bool,

    #[arg(long = "struct", help = "Restrict to structs only")]
    pub struct_only: bool,

    #[arg(long = "enum", help = "Restrict to enums only")]
    pub enum_only: bool,

    #[arg(
        short = 'n',
        long = "max-results",
        value_name = "N",
        default_value_t = 50,
        help = "Cap on results emitted (default 50; 0 = unlimited). When truncated, a footer notes the original total."
    )]
    pub max_results: usize,

    #[arg(
        long = "show-ids",
        help = "Append `[id: <symbol_id>]` to each text-mode result line. Off by default for readability; turn on when you need to round-trip through `callers --symbol-id` / `ensemble --symbol-id` (or just use `-j`)."
    )]
    pub show_ids: bool,

    #[arg(
        long,
        help = "Exact-name mode: return ALL exact (case-insensitive) name matches with no fuzz expansion. Does not error on 0 or many. Use this when `find` substring/prefix noise gets in the way."
    )]
    pub exact: bool,

    #[arg(
        long = "in",
        value_name = "PATH_SUBSTR",
        help = "FILE filter: only show symbols whose file_path contains SUBSTR (e.g. `--in src/daemon`). Mirrors grep/refs/dead-code --in."
    )]
    pub in_path: Option<String>,
}

/// Arguments for the `callers` subcommand (reverse call-graph lookup with
/// optional transitive depth).
#[derive(clap::Args, Debug, Clone)]
pub struct CallersCommand {
    #[arg(
        required_unless_present = "symbol_id",
        help = "Function name, path.rs:LINE, or symbol_id (use --symbol-id for the canonical form). Optional when --symbol-id is set."
    )]
    pub query: Option<String>,

    #[arg(
        long,
        help = "Resolve target by exact symbol_id (from `find -j` / `callers -j`). Bypasses fuzzy matching entirely. Format: `<path>:<line>:<name>` for fns, `struct:...` / `enum:...` for types. When set, positional QUERY is optional."
    )]
    pub symbol_id: Option<String>,

    #[arg(short = 'A', long = "after-context", default_value_t = 0)]
    #[arg(help = "Print N lines of trailing context around each call site")]
    pub after_context: usize,

    #[arg(short = 'B', long = "before-context", default_value_t = 0)]
    #[arg(help = "Print N lines of leading context around each call site")]
    pub before_context: usize,

    #[arg(short = 'C', long = "context")]
    #[arg(help = "Print N lines of leading and trailing context around each call site")]
    pub context: Option<usize>,

    #[arg(
        long = "target-in",
        value_name = "PATH_SUBSTR",
        help = "TARGET filter: only keep target fns whose file_path contains SUBSTR. For caller-side filtering use --callers-in."
    )]
    pub target_in: Option<String>,

    #[arg(
        long = "callers-in",
        value_name = "PATH_SUBSTR",
        help = "CALLER filter: only show callers whose file_path contains this substring. Filters at every depth (depth=1 direct callers AND transitive callers at depth ≥ 2)."
    )]
    pub callers_in: Option<String>,

    #[arg(
        long,
        default_value_t = 1,
        help = "Caller chain depth. Default 1 = direct callers (today's behavior). Set to 2+ for transitive caller chains; 0 = unlimited (cycle-safe). Output gains a depth field per node and a tree view in text mode."
    )]
    pub depth: usize,

    #[arg(
        long,
        help = "Process all matches when the query is ambiguous (>--ambiguity-cap fns). Without this flag, callers prints the candidate list and exits non-zero so you can disambiguate via --target-in <PATH>, --symbol-id, or `path.rs:LINE` query."
    )]
    pub all: bool,

    #[arg(
        long = "ambiguity-cap",
        value_name = "N",
        default_value_t = 7,
        help = "Max matches before requiring --all (default 7). Bump it when you legitimately want to process N> fns without --all firehose."
    )]
    pub ambiguity_cap: usize,

    #[arg(
        short = 'n',
        long = "max-results",
        value_name = "N",
        help = "Cap on callers emitted (default 50; 0 = unlimited). When truncated, a footer notes the original count."
    )]
    pub max_results: Option<usize>,

    #[arg(
        long,
        conflicts_with = "json",
        help = "Emit all transitive callers as a deduplicated `path:line:name` list (one per line, no tree). Pipe-friendly; works with --depth N. Mutually exclusive with -j."
    )]
    pub flat: bool,
}

impl CallersCommand {
    /// Returns `(before, after)` context line counts, normalising the
    /// combined `-C` flag over the separate `-B`/`-A` flags.
    ///
    /// When `-C N` is set, both values are `N`; otherwise the individual
    /// `--before-context` and `--after-context` values are returned.
    pub fn resolved_before_after(&self) -> (usize, usize) {
        if let Some(context) = self.context {
            (context, context)
        } else {
            (self.before_context, self.after_context)
        }
    }
}

/// Arguments for the `ensemble` subcommand (full context bundle for a
/// function: source, structs, call sites, neighborhood, dataflow).
#[derive(clap::Args, Debug, Clone)]
pub struct EnsembleCommand {
    #[arg(
        required_unless_present = "symbol_id",
        help = "Function name, path.rs:LINE, or use --symbol-id for canonical lookup (functions only — for structs/enums use `def <name>`). Optional when --symbol-id is set."
    )]
    pub query: Option<String>,

    #[arg(
        long,
        help = "Resolve target by exact symbol_id (from `find -j` / `callers -j`). Bypasses fuzzy matching. When set, positional QUERY is optional."
    )]
    pub symbol_id: Option<String>,

    #[arg(long, value_enum)]
    #[arg(
        help = "High-level ensemble depth profile: quick (fast), balanced (default), deep (maximum context)."
    )]
    pub preset: Option<EnsemblePreset>,

    #[arg(long, value_enum)]
    #[arg(help = "High-level ensemble output view: summary (DEFAULT — structs/call-sites/neighborhood/dataflow with abbreviated neighborhood), usage (call-sites + neighborhood), flow (neighborhood + lifecycle + dataflow + boundaries), or full (every section — often 300+ lines).")]
    pub view: Option<EnsembleView>,

    #[arg(long)]
    #[arg(
        help = "Lifecycle type names to analyze in ensemble mode (use '|' as separator). Defaults to structs used by matched function."
    )]
    pub lifecycle_types: Option<String>,

    #[arg(
        long = "target-in",
        value_name = "PATH_SUBSTR",
        help = "TARGET filter: only keep target fns whose file_path contains SUBSTR."
    )]
    pub target_in: Option<String>,

    #[arg(
        long = "section",
        value_name = "S",
        help = "Cherry-pick which sections render. Repeat for multiple. Valid: code, structs, call-sites, neighborhood, lifecycle, dataflow, boundaries, all. Overrides --view if set. Use this to suppress noisy sections (e.g. `--section code --section neighborhood` skips boundaries)."
    )]
    pub section: Vec<String>,

    #[arg(
        long,
        default_value_t = 0.7,
        help = "I/O boundaries minimum confidence score (0.0-1.0). Default 0.7 drops the noisy 0.5-0.65 entries that flooded `--view full` output. Set to 0.0 for the legacy unfiltered firehose."
    )]
    pub boundaries_min_score: f64,

    #[arg(long, help = "Suppress the I/O boundaries section entirely.")]
    pub no_boundaries: bool,

    #[arg(
        long,
        help = "Process all matches when the query is ambiguous (>--ambiguity-cap fns). Without this flag, ensemble prints the candidate list and exits non-zero so you can disambiguate via --target-in <PATH>, --symbol-id, or `path.rs:LINE` query."
    )]
    pub all: bool,

    #[arg(
        long = "ambiguity-cap",
        value_name = "N",
        default_value_t = 7,
        help = "Max matches before requiring --all (default 7)."
    )]
    pub ambiguity_cap: usize,
}

/// Arguments for the `dead-code` subcommand (find public functions with zero
/// non-test callers).
#[derive(clap::Args, Debug, Clone, Default)]
pub struct DeadCodeCommand {
    #[arg(
        long = "in",
        value_name = "PATH_SUBSTR",
        help = "FILE filter: only flag dead fns whose file_path contains this substring (e.g. `--in src/daemon`). Scope your dead-code audit to a specific layer."
    )]
    pub in_path: Option<String>,

    #[arg(
        long,
        help = "Show the skip-reason summary line (re-exported / inside cfg(test) / runtime entrypoints / ambiguous / framework-handler counts). Useful once for debugging a missed entry; default-off to keep daily output tight."
    )]
    pub verbose: bool,

    #[arg(
        short = 'n',
        long = "max-results",
        value_name = "N",
        default_value_t = 50,
        help = "Cap on candidates rendered (default 50; 0 = unlimited). Summary count stays unfiltered; the per-file list is truncated with a footer."
    )]
    pub max_results: usize,
}

/// Arguments for the `call-graph` subcommand (generate a rooted or full
/// project call graph as text tree or DOT).
#[derive(clap::Args, Debug, Clone)]
pub struct CallGraphCommand {


    #[arg(help = "Anchor function (positional shortcut for --root). Same as --root NAME.")]
    pub root_positional: Option<String>,

    #[arg(long, value_enum, default_value_t = CallGraphDetail::Internal)]
    #[arg(help = "Call graph detail level: internal, external, or all")]
    pub detail: CallGraphDetail,

    #[arg(
        long,
        help = "Anchor the graph at this function (name or path.rs:LINE). Walks outgoing call edges and emits a subgraph instead of the whole project. Default: emit full graph (often 500+ nodes, unusable)."
    )]
    pub root: Option<String>,

    #[arg(
        long,
        default_value_t = 3,
        help = "When --root is set, cap the BFS depth (0 = unlimited). Ignored without --root."
    )]
    pub depth: usize,

    #[arg(
        long,
        help = "When --root is set, also include callers of root (up to --depth hops up). Default: downstream-only."
    )]
    pub include_callers: bool,

    #[arg(
        long,
        help = "Emit DOT (graphviz) instead of the default indented text tree. Default flipped in R13 — DOT is unrenderable without graphviz, so text is the happy path."
    )]
    pub dot: bool,

    #[arg(
        long,
        help = "Show the `<external>` rollup line listing unresolved callees (std iterator chains, method calls, cross-crate fns). Hidden by default in text mode because the noise floor buries actual project edges. No effect in --dot mode."
    )]
    pub show_external: bool,

    #[arg(
        long,
        help = "Render the WHOLE project graph (no scope filter). Required when no --root/--search is set on a project with >50 functions — without this flag the unscoped path errors out. Small projects (≤50 fns) auto-render unscoped without --all."
    )]
    pub all: bool,
}

/// Arguments for the `tree` subcommand (module/file tree with symbol counts).
#[derive(clap::Args, Debug, Clone)]
pub struct TreeCommand {
    #[arg(help = "Optional path prefix (relative to project root) to limit the tree to a subtree")]
    pub prefix: Option<String>,

    #[arg(long, help = "Hide symbol counts; show files only")]
    pub files_only: bool,
}

/// Arguments for the `grep` subcommand (regex/literal text search across
/// Rust source files, including comments and string literals).
#[derive(clap::Args, Debug, Clone)]
pub struct GrepCommand {
    #[arg(help = "Regex pattern (Rust regex syntax)")]
    pub pattern: String,

    #[arg(short = 'i', long, help = "Case-insensitive match")]
    pub ignore_case: bool,

    #[arg(short = 'F', long, help = "Treat pattern as a literal string (not regex)")]
    pub fixed_string: bool,


    #[arg(
        short = 'C',
        long = "context",
        help = "Show N lines of context around each match (shorthand for -B N -A N; overrides -B/-A when set)."
    )]
    pub context: Option<usize>,

    #[arg(
        short = 'B',
        long = "before-context",
        value_name = "N",
        default_value_t = 0,
        help = "Show N lines of leading context before each match. Mirrors callers --before-context. Overridden by -C if both are set."
    )]
    pub before_context: usize,

    #[arg(
        short = 'A',
        long = "after-context",
        value_name = "N",
        default_value_t = 0,
        help = "Show N lines of trailing context after each match. Mirrors callers --after-context. Overridden by -C if both are set."
    )]
    pub after_context: usize,

    #[arg(
        short = 'n',
        long = "max-results",
        value_name = "N",
        default_value_t = 200,
        help = "Cap on total matches printed (default 200; 0 = unlimited). Unified across all listing subcommands."
    )]
    pub max_results: usize,

    #[arg(
        long = "enclosing-function",
        visible_alias = "enclosing-fn",
        help = "For each match, also report the enclosing fn name and its file:start-end range. Folds the grep→manual-fn-mapping pipeline into one call."
    )]
    pub enclosing_function: bool,

    #[arg(
        long = "by-function",
        visible_alias = "rollup",
        conflicts_with = "by_file",
        help = "Token-saver: skip the per-line match dump, emit ONLY the summary-by-function table (count desc, with [test] tags). Implies --enclosing-function. Use this for big audits where you only need the per-fn counts. Mutually exclusive with --by-file."
    )]
    pub by_function: bool,


    #[arg(
        long = "by-file",
        conflicts_with = "by_function",
        help = "Token-saver: skip per-line dump, emit ONLY a per-file rollup (`<path>: N matches`). Mutually exclusive with --by-function. JSON envelope: `by_file: [{path, count}]`."
    )]
    pub by_file: bool,

    #[arg(
        long = "in",
        value_name = "PATH_SUBSTR",
        help = "FILE filter: search only files whose path contains SUBSTR (e.g. `--in src/daemon`)."
    )]
    pub in_path: Option<String>,
}

impl GrepCommand {
    /// Returns `(before, after)` context line counts, normalising `-C` over
    /// separate `-B`/`-A` flags.
    ///
    /// Mirrors [`CallersCommand::resolved_before_after`]: `-C N` wins over
    /// individual values when set.
    pub fn resolved_before_after(&self) -> (usize, usize) {
        if let Some(c) = self.context {
            (c, c)
        } else {
            (self.before_context, self.after_context)
        }
    }
}

/// Arguments for the `impls` subcommand (list every type that implements a
/// given trait, via derive or hand-written impl).
#[derive(clap::Args, Debug, Clone)]
pub struct ImplsCommand {
    #[arg(
        help = "Trait name to look up. Matched by last path segment, so `Serialize` matches `serde::Serialize`. Pass the fully qualified name to disambiguate."
    )]
    pub trait_name: String,

    #[arg(
        long,
        help = "Only show derive-generated impls (skip hand-written `impl Trait for X`)"
    )]
    pub derived_only: bool,

    #[arg(
        long,
        help = "Only show hand-written impls (skip `#[derive(...)]`)"
    )]
    pub handwritten_only: bool,

    #[arg(
        long = "in",
        value_name = "PATH_SUBSTR",
        help = "FILE filter: only show impls whose file_path contains SUBSTR (e.g. `--in src/api`). Mirrors grep/refs/dead-code."
    )]
    pub in_path: Option<String>,

    #[arg(
        short = 'n',
        long = "max-results",
        value_name = "N",
        default_value_t = 50,
        help = "Cap on implementors rendered (default 50; 0 = unlimited). Summary counts stay unfiltered; the rendered list is truncated with a footer."
    )]
    pub max_results: usize,
}

#[derive(Parser)]
#[command(name = "rustgraph")]
#[command(version)]
#[command(after_help = "See `man rustgraph` for the full reference. Run `rustgraph <subcommand> --help` for per-command flags.")]
#[command(about = "AST-aware Rust codebase navigation.\n\n\
MCP (Claude / Codex / Gemini integration):\n  \
  rustgraph mcp install\n  \
  rustgraph mcp list\n  \
  rustgraph mcp uninstall\n  \
  rustgraph mcp                      # serve stdio (spawned by client)")]
pub struct Args {


    #[arg(
        short, long,
        default_value = ".",
        global = true, hide_short_help = true, hide_long_help = true,
        help_heading = "Resolution",
        help = "Crate root path"
    )]
    pub path: PathBuf,

    #[arg(
        long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Resolution",
        help = "Disable Cargo.toml auto-discovery; use -p verbatim"
    )]
    pub no_auto_path: bool,

    #[arg(
        long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Resolution",
        help = "Emit absolute file paths (default: relative to -p)"
    )]
    pub absolute_paths: bool,

    #[arg(
        long = "also", value_name = "PATH",
        global = true, hide_short_help = true, hide_long_help = true, help_heading = "Resolution",
        help = "Extra crate root(s) to merge (cross-crate; repeat for multiple)"
    )]
    pub also: Vec<PathBuf>,

    #[arg(
        short, long,
        global = true, hide_short_help = true, hide_long_help = true, help_heading = "Output",
        help = "Write to FILE instead of stdout"
    )]
    pub output: Option<PathBuf>,

    #[arg(
        short, long,
        global = true, hide_short_help = true, hide_long_help = true, help_heading = "Output",
        help = "Emit JSON (stable schema, pipes to jq)"
    )]
    pub json: bool,

    #[arg(
        long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Output",
        help = "Include .gitignore'd files (target/, build/)"
    )]
    pub include_ignored: bool,

    #[arg(long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Output", help = "Enable colored output")]
    pub color: bool,

    #[arg(long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Output", conflicts_with = "color", help = "Disable colored output (default)")]
    pub no_color: bool,

    #[arg(long, value_enum, help_heading = "Inventory mode",
        help = "Inventory filter [default: both]. Inventory mode only — for lookup use `find`"
    )]
    pub analyze: Option<AnalyzeMode>,

    #[arg(long, help_heading = "Inventory mode", help = "Inventory shortcut: functions only")]
    pub func: bool,

    #[arg(long, help_heading = "Inventory mode", help = "Inventory shortcut: structs only")]
    pub r#struct: bool,

    #[arg(long, help_heading = "Inventory mode", help = "Inventory shortcut: enums only")]
    pub r#enum: bool,

    #[arg(long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Search",
        help = "Fuzzy search across fns/structs/enums (use '|' for OR)"
    )]
    pub search: Option<String>,

    #[arg(long, default_value = "0.85", global = true, hide_short_help = true, hide_long_help = true, help_heading = "Search",
        help = "Fuzzy match threshold (default 0.85; raise toward 1.0 for strict)"
    )]
    pub search_threshold: f64,

    #[arg(long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Search",
        help = "ADDITIVE — union name-tier hits with hits whose signature (params/return) or file_path fuzzy-matches the query (deduped; name wins on collision). `find State --match-signature` returns every fn named *State* PLUS every fn whose params/return mention `State`. Stderr summary: `note: X name + Y sig/path matches`."
    )]
    pub match_signature: bool,

    #[arg(long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Search",
        help = "Drop matches whose enclosing fn is_test (grep/refs/find/dead-code/callers/ensemble/usages)"
    )]
    pub exclude_tests: bool,

    #[arg(long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Search",
        help = "Filter to public items only (skip pub(crate)/private). Applies to inventory + find."
    )]
    pub public_only: bool,


    #[arg(long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Resolution",
        help = "Drop call edges where type-strict matching returned 0 candidates instead of falling back to bare-name match (callers/call-graph/paths-between).",
        long_help = "When set, drop call edges that the strict resolver filters out instead of falling back to bare-name match. \
Reduces false-positive edges at the cost of hiding some legit ones; see stderr for dropped count."
    )]
    pub strict_resolution: bool,


    #[arg(
        long, global = true, hide_short_help = true, hide_long_help = true, help_heading = "Diff",
        help = "Filter results to changes since --since (default HEAD~1). Granularity varies: find applies span-level filtering to fn/struct/enum; callers/refs/dead-code apply span-level filtering to fns; grep/impls/tree filter at file-level. Requires git."
    )]
    pub changed: bool,

    #[arg(
        long, value_name = "GIT_REF",
        global = true, hide_short_help = true, hide_long_help = true, help_heading = "Diff",
        help = "Diff base for --changed (default HEAD~1 when --changed is set without --since). Any git ref accepted (branch, tag, sha)."
    )]
    pub since: Option<String>,

    #[command(subcommand)]
    pub command: Option<ModeCommand>,

    #[arg(long, hide = true)]
    pub call_graph: bool,

    #[arg(long, value_enum, default_value_t = CallGraphDetail::Internal, hide = true)]
    pub call_graph_detail: CallGraphDetail,

    #[arg(long, visible_alias = "orphans", hide = true)]
    pub dead_code: bool,

    #[arg(long, hide = true)]
    pub ensemble: Option<String>,

    #[arg(long, hide = true)]
    pub callers: Option<String>,

    #[arg(long, value_enum, hide = true)]
    pub ensemble_preset: Option<EnsemblePreset>,

    #[arg(long, value_enum, hide = true)]
    pub ensemble_view: Option<EnsembleView>,

    #[arg(long, hide = true)]
    pub ensemble_lifecycle_types: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callers_command_resolved_before_after_uses_explicit_context() {
        let cmd = CallersCommand {
            query: Some("foo".to_string()),
            after_context: 5,
            before_context: 3,
            context: Some(7),
            symbol_id: None,
            target_in: None,
            callers_in: None,
            depth: 1,
            all: false,
            ambiguity_cap: 7,
            max_results: None,
            flat: false,
        };
        assert_eq!(cmd.resolved_before_after(), (7, 7));
    }

    #[test]
    fn callers_command_resolved_before_after_uses_separate_when_no_combined() {
        let cmd = CallersCommand {
            query: Some("foo".to_string()),
            after_context: 5,
            before_context: 3,
            context: None,
            symbol_id: None,
            target_in: None,
            callers_in: None,
            depth: 1,
            all: false,
            ambiguity_cap: 7,
            max_results: None,
            flat: false,
        };
        assert_eq!(cmd.resolved_before_after(), (3, 5));
    }

    #[test]
    fn callers_command_resolved_before_after_defaults_to_zero() {
        let cmd = CallersCommand {
            query: Some("foo".to_string()),
            after_context: 0,
            before_context: 0,
            context: None,
            symbol_id: None,
            target_in: None,
            callers_in: None,
            depth: 1,
            all: false,
            ambiguity_cap: 7,
            max_results: None,
            flat: false,
        };
        assert_eq!(cmd.resolved_before_after(), (0, 0));
    }

    #[test]
    fn args_can_parse_default_invocation() {
        let parsed = Args::try_parse_from(["rustgraph"]).expect("parse");
        assert_eq!(parsed.path, std::path::PathBuf::from("."));
        assert!(!parsed.json);
        assert!(!parsed.color);
        assert_eq!(parsed.search_threshold, 0.85);
        assert!(parsed.command.is_none());
    }

    #[test]
    fn args_color_and_no_color_are_exclusive() {
        let result = Args::try_parse_from(["rustgraph", "--color", "--no-color"]);
        assert!(result.is_err());
    }

    #[test]
    fn args_recognises_callers_subcommand_with_context_flags() {
        let parsed = Args::try_parse_from(["rustgraph", "callers", "-C", "3", "target"])
            .expect("parse");
        match parsed.command.unwrap() {
            ModeCommand::Callers(c) => {
                assert_eq!(c.query.as_deref(), Some("target"));
                assert_eq!(c.context, Some(3));
                assert_eq!(c.resolved_before_after(), (3, 3));
                assert_eq!(c.max_results, None);
            }
            _ => panic!("expected callers subcommand"),
        }
    }

    #[test]
    fn args_dead_code_alias_orphans_works() {
        let parsed = Args::try_parse_from(["rustgraph", "orphans"]).expect("parse");
        assert!(matches!(parsed.command, Some(ModeCommand::DeadCode(_))));
    }
}
