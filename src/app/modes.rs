//! Request types and the top-level [`ExecutionMode`] enum that selects a subcommand pipeline.
//!
//! Each variant of [`ExecutionMode`] bundles the structured parameters required by the
//! corresponding runner in `app/run/`. [`ExecutionMode::from_args`] converts raw
//! [`crate::cli::Args`] into the appropriate variant.

use crate::api::{CallGraphDetail, EnsemblePreset, EnsembleView};
use crate::cli::{AnalyzeMode, Args, ModeCommand};

/// Which symbol kinds to include in an inventory or find operation.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AnalyzeSelection {
    Functions,
    Structs,
    Enums,
    FunctionsStructs,
    FunctionsEnums,
    StructsEnums,
    All,
}

impl AnalyzeSelection {
    /// Derives the selection from shortcut flags (`--func`, `--struct`, `--enum`) or
    /// the `--analyze` flag, giving precedence to shortcuts when any are set.
    pub fn from_args(args: &Args) -> Self {
        let any_shortcut = args.func || args.r#struct || args.r#enum;

        if any_shortcut {
            match (args.func, args.r#struct, args.r#enum) {
                (true, false, false) => Self::Functions,
                (false, true, false) => Self::Structs,
                (false, false, true) => Self::Enums,
                (true, true, false) => Self::FunctionsStructs,
                (true, false, true) => Self::FunctionsEnums,
                (false, true, true) => Self::StructsEnums,
                (true, true, true) => Self::All,
                (false, false, false) => Self::All,
            }
        } else {
            match args.analyze.unwrap_or(AnalyzeMode::Both) {
                AnalyzeMode::Functions => Self::Functions,
                AnalyzeMode::Structs => Self::Structs,
                AnalyzeMode::Enums => Self::Enums,
                AnalyzeMode::Both => Self::All,
            }
        }
    }

    /// Returns `true` if this selection includes functions.
    pub fn show_functions(self) -> bool {
        matches!(
            self,
            Self::Functions | Self::FunctionsStructs | Self::FunctionsEnums | Self::All
        )
    }

    /// Returns `true` if this selection includes structs.
    pub fn show_structs(self) -> bool {
        matches!(
            self,
            Self::Structs | Self::FunctionsStructs | Self::StructsEnums | Self::All
        )
    }

    /// Returns `true` if this selection includes enums.
    pub fn show_enums(self) -> bool {
        matches!(
            self,
            Self::Enums | Self::FunctionsEnums | Self::StructsEnums | Self::All
        )
    }
}

/// Parameters for the `ensemble` subcommand, which builds a rich multi-section context bundle
/// around a target symbol (code, call-sites, neighborhood, lifecycles, I/O boundaries).
#[derive(Clone, Debug)]
pub struct EnsembleRequest {
    /// Fuzzy-search query identifying the target symbol.
    pub query: String,
    /// Stable symbol ID used instead of `query` when provided.
    pub symbol_id: Option<String>,
    /// Named preset controlling numeric limits (max results, neighborhood depth, etc.).
    pub preset: Option<EnsemblePreset>,
    /// Output view selecting which sections are rendered (`summary`, `usage`, `flow`, `full`).
    pub view: Option<EnsembleView>,
    /// Pipe-separated (`|`) type names for lifecycle section filtering.
    pub lifecycle_types: Option<String>,
    /// Restrict target resolution to symbols under this path prefix.
    pub target_in: Option<String>,
    /// Explicit list of section names to include, overriding preset defaults.
    pub sections: Vec<String>,
    /// Minimum confidence score for I/O boundary signals.
    pub boundaries_min_score: f64,
    /// Suppress I/O boundary section entirely when `true`.
    pub no_boundaries: bool,
    /// Return all fuzzy matches rather than capping at `ambiguity_cap`.
    pub all: bool,
    /// Maximum number of ambiguous matches to surface before requiring disambiguation.
    pub ambiguity_cap: usize,
}

/// Parameters for the `callers` subcommand, which finds all callers of a target function
/// and attaches per-call-site source context.
#[derive(Clone, Debug)]
pub struct CallersRequest {
    /// Fuzzy-search query identifying the callee.
    pub query: String,
    /// Stable symbol ID used instead of `query` when provided.
    pub symbol_id: Option<String>,
    /// Source lines to show before each matched call site.
    pub before_context: usize,
    /// Source lines to show after each matched call site.
    pub after_context: usize,
    /// Restrict callee resolution to symbols whose file path contains this substring.
    pub target_in: Option<String>,
    /// Restrict caller results to functions whose file path contains this substring.
    pub callers_in: Option<String>,
    /// How many call-graph hops to follow (1 = direct callers only; 0 = unlimited).
    pub depth: usize,
    /// Return all fuzzy matches rather than capping at `ambiguity_cap`.
    pub all: bool,
    /// Maximum number of ambiguous matches to surface before requiring disambiguation.
    pub ambiguity_cap: usize,
    /// Cap on total caller results returned; `None` means unlimited.
    pub max_results: Option<usize>,
    /// Emit a flat list of call sites instead of a grouped tree.
    pub flat: bool,
}

/// Parameters for the `tree` subcommand, which renders the module/file hierarchy.
#[derive(Clone, Debug)]
pub struct TreeRequest {
    /// Optional path prefix to scope the tree output.
    pub prefix: Option<String>,
    /// When `true`, omit module nodes and list only source files.
    pub files_only: bool,
}

/// Parameters for the `grep` subcommand, which searches source text within the project.
#[derive(Clone, Debug)]
pub struct GrepRequest {
    /// Regular expression or literal pattern to search for.
    pub pattern: String,
    /// Case-insensitive matching when `true`.
    pub ignore_case: bool,
    /// Treat `pattern` as a fixed string rather than a regex.
    pub fixed_string: bool,
    /// Symmetric context lines; overrides `before_context`/`after_context` when set.
    pub context: Option<usize>,
    /// Source lines to show before each matched line.
    pub before_context: usize,
    /// Source lines to show after each matched line.
    pub after_context: usize,
    /// Maximum number of match lines to return.
    pub max_results: usize,
    /// Annotate each match with its enclosing function name and line range.
    pub enclosing_function: bool,
    /// Group and label results by enclosing function.
    pub by_function: bool,
    /// Group and label results by source file.
    pub by_file: bool,
    /// Restrict the search to files under this path prefix.
    pub in_path: Option<String>,
}

/// Parameters for the `impls` subcommand, which lists trait implementations.
#[derive(Clone, Debug)]
pub struct ImplsRequest {
    /// Name of the trait whose implementations to find.
    pub trait_name: String,
    /// Restrict to `#[derive]`-generated impls only.
    pub derived_only: bool,
    /// Restrict to hand-written (non-derived) impls only.
    pub handwritten_only: bool,
    /// Restrict results to types defined under this path prefix.
    pub in_path: Option<String>,
    /// Maximum number of impls to return.
    pub max_results: usize,
}

/// Parameters for the `find` subcommand, which fuzzy-searches for named symbols.
#[derive(Clone, Debug)]
pub struct FindRequest {
    /// Fuzzy query string matched against symbol names.
    pub query: String,
    /// Minimum fuzzy-match score in `[0.0, 1.0]` to include a result.
    pub threshold: f64,
    /// Symbol kinds to include in the search.
    pub selection: AnalyzeSelection,
    /// Maximum number of results to return.
    pub max_results: usize,
    /// Print the stable symbol ID alongside each match.
    pub show_ids: bool,
    /// Require an exact name match rather than fuzzy scoring.
    pub exact: bool,
    /// Restrict the search to symbols defined under this path prefix.
    pub in_path: Option<String>,
}

/// Parameters for the `refs` subcommand, which finds all use-sites of an identifier.
#[derive(Clone, Debug)]
pub struct RefsRequest {
    /// Identifier whose references to locate.
    pub ident: String,
    /// Optional list of reference kinds to include (`field`, `path`, `assoc_call`, `variant`, `type`, `struct`, `pattern`, `method`, `call`).
    pub kind: Vec<String>,
    /// Restrict results to files under this path prefix.
    pub in_path: Option<String>,
    /// Group results by enclosing function.
    pub by_function: bool,
    /// Maximum number of references to return.
    pub max_results: usize,
}

/// Parameters for the `paths-between` subcommand, which traces call-graph paths
/// between two named symbols.
#[derive(Clone, Debug)]
pub struct PathsBetweenRequest {
    /// Starting symbol name (source of the path search).
    pub from: String,
    /// Ending symbol name (destination of the path search).
    pub to: String,
    /// Maximum call-graph depth to explore.
    pub depth: usize,
    /// Maximum number of distinct paths to return.
    pub max_results: usize,
    /// Restrict the graph traversal to functions under this path prefix.
    pub in_path: Option<String>,
    /// Annotate each hop with its concrete call-site location.
    pub show_call_sites: bool,
    /// Blast-radius mode: treat any function whose `file_path` contains this substring as a destination.
    pub to_module: Option<String>,
}

/// Parameters for the `usages` subcommand, which locates type-use sites.
#[derive(Clone, Debug)]
pub struct UsagesRequest {
    /// Type or identifier name whose usages to find.
    pub name: String,
    /// Restrict results to files under this path prefix.
    pub in_path: Option<String>,
    /// Maximum number of usages to return.
    pub max_results: usize,
}

/// Parameters for the `def` subcommand, which jumps to a symbol's definition site.
#[derive(Clone, Debug)]
pub struct DefRequest {
    /// Name of the symbol whose definition to locate.
    pub name: String,
    /// When `true`, restrict to function definitions only.
    pub func_only: bool,
    /// When `true`, restrict to struct definitions only.
    pub struct_only: bool,
    /// When `true`, restrict to enum definitions only.
    pub enum_only: bool,
    /// Restrict the search to symbols under this path prefix.
    pub in_path: Option<String>,
}


/// Parameters for the `members` subcommand, which lists fields or variants of a type.
#[derive(Clone, Debug)]
pub struct MembersRequest {
    /// Name of the struct or enum whose members to list.
    pub type_name: String,
    /// Restrict the search to types defined under this path prefix.
    pub in_path: Option<String>,
    /// Maximum number of types to match.
    pub max_results: usize,
}

/// Selects which analysis pipeline to execute and carries its typed request parameters.
///
/// Constructed by [`ExecutionMode::from_args`] from the parsed CLI args. Each variant maps
/// one-to-one to a runner in `app/run/`.
#[derive(Clone, Debug)]
pub enum ExecutionMode {
    Ensemble(EnsembleRequest),
    Callers(CallersRequest),
    CallGraph {
        detail: CallGraphDetail,
        config: CallGraphConfig,
    },
    DeadCode {
        in_path: Option<String>,
        verbose: bool,
        max_results: usize,
    },
    Inventory {
        selection: AnalyzeSelection,
    },
    Tree(TreeRequest),
    Grep(GrepRequest),
    Impls(ImplsRequest),
    Find(FindRequest),
    Refs(RefsRequest),
    PathsBetween(PathsBetweenRequest),
    Usages(UsagesRequest),
    Def(DefRequest),
    Members(MembersRequest),
}

/// Rendering and traversal configuration for the `call-graph` subcommand.
#[derive(Clone, Debug)]
pub struct CallGraphConfig {
    /// Root function name to start the call-graph traversal from.
    pub root: Option<String>,
    /// Maximum depth to traverse from the root.
    pub depth: usize,
    /// When `true`, include callers (upstream) in addition to callees.
    pub include_callers: bool,
    /// Emit plain-text output rather than Graphviz DOT format.
    pub text: bool,
    /// Include calls to functions outside the project root.
    pub show_external: bool,
    /// Show all reachable nodes rather than capping at the default limit.
    pub all: bool,
}

impl Default for CallGraphConfig {
    fn default() -> Self {
        Self {
            root: None,
            depth: 0,
            include_callers: false,
            text: true,
            show_external: false,
            all: false,
        }
    }
}

impl ExecutionMode {
    /// Constructs the appropriate [`ExecutionMode`] variant from parsed CLI args.
    ///
    /// Subcommands take precedence over legacy top-level flags (e.g. `--dead-code`,
    /// `--callers`, `--ensemble`). Falls back to `Inventory` when no action is specified.
    pub fn from_args(args: &Args) -> Self {
        if let Some(command) = args.command.clone() {
            return match command {
                ModeCommand::Callers(callers) => {
                    let (before_context, after_context) = callers.resolved_before_after();


                    let query = callers
                        .query
                        .or_else(|| callers.symbol_id.clone())
                        .unwrap_or_default();
                    Self::Callers(CallersRequest {
                        query,
                        symbol_id: callers.symbol_id,
                        before_context,
                        after_context,
                        target_in: callers.target_in,
                        callers_in: callers.callers_in,
                        depth: callers.depth,
                        all: callers.all,
                        ambiguity_cap: callers.ambiguity_cap,
                        max_results: callers.max_results,
                        flat: callers.flat,
                    })
                }
                ModeCommand::Ensemble(ensemble) => {
                    let query = ensemble
                        .query
                        .or_else(|| ensemble.symbol_id.clone())
                        .unwrap_or_default();
                    Self::Ensemble(EnsembleRequest {
                        query,
                        symbol_id: ensemble.symbol_id,
                        preset: ensemble.preset,
                        view: ensemble.view,
                        lifecycle_types: ensemble.lifecycle_types,
                        target_in: ensemble.target_in,
                        sections: ensemble.section,
                        boundaries_min_score: ensemble.boundaries_min_score,
                        no_boundaries: ensemble.no_boundaries,
                        all: ensemble.all,
                        ambiguity_cap: ensemble.ambiguity_cap,
                    })
                }
                ModeCommand::DeadCode(dc) => Self::DeadCode {
                    in_path: dc.in_path,
                    verbose: dc.verbose,
                    max_results: dc.max_results,
                },
                ModeCommand::CallGraph(call_graph) => Self::CallGraph {
                    detail: call_graph.detail,
                    config: CallGraphConfig {

                        root: call_graph.root.or(call_graph.root_positional),
                        depth: call_graph.depth,
                        include_callers: call_graph.include_callers,
                        text: !call_graph.dot,
                        show_external: call_graph.show_external,
                        all: call_graph.all,
                    },
                },
                ModeCommand::Tree(tree) => Self::Tree(TreeRequest {
                    prefix: tree.prefix,
                    files_only: tree.files_only,
                }),
                ModeCommand::Grep(grep) => {


                    let (before_context, after_context) = grep.resolved_before_after();
                    Self::Grep(GrepRequest {
                        pattern: grep.pattern,
                        ignore_case: grep.ignore_case,
                        fixed_string: grep.fixed_string,
                        context: grep.context,
                        before_context,
                        after_context,
                        max_results: grep.max_results,

                        enclosing_function: grep.enclosing_function || grep.by_function,
                        by_function: grep.by_function,
                        by_file: grep.by_file,
                        in_path: grep.in_path,
                    })
                }
                ModeCommand::Impls(impls) => Self::Impls(ImplsRequest {
                    trait_name: impls.trait_name,
                    derived_only: impls.derived_only,
                    handwritten_only: impls.handwritten_only,
                    in_path: impls.in_path,
                    max_results: impls.max_results,
                }),
                ModeCommand::Refs(refs) => Self::Refs(RefsRequest {
                    ident: refs.ident,
                    kind: refs.kind,
                    in_path: refs.in_path,
                    by_function: refs.by_function,
                    max_results: refs.max_results,
                }),
                ModeCommand::PathsBetween(pb) => Self::PathsBetween(PathsBetweenRequest {
                    from: pb.from,


                    to: pb.to.unwrap_or_default(),
                    depth: pb.depth,
                    max_results: pb.max_results,
                    in_path: pb.in_path,
                    show_call_sites: pb.show_call_sites,
                    to_module: pb.to_module,
                }),
                ModeCommand::Usages(u) => Self::Usages(UsagesRequest {
                    name: u.name,
                    in_path: u.in_path,
                    max_results: u.max_results,
                }),
                ModeCommand::Def(d) => Self::Def(DefRequest {
                    name: d.name,
                    func_only: d.func,
                    struct_only: d.struct_only,
                    enum_only: d.enum_only,
                    in_path: d.in_path,
                }),
                ModeCommand::Members(m) => Self::Members(MembersRequest {
                    type_name: m.type_name,
                    in_path: m.in_path,
                    max_results: m.max_results,
                }),
                ModeCommand::Inventory(inv) => {
                    let selection = match (inv.func, inv.struct_only, inv.enum_only) {
                        (false, false, false) => AnalyzeSelection::All,
                        (true, false, false) => AnalyzeSelection::Functions,
                        (false, true, false) => AnalyzeSelection::Structs,
                        (false, false, true) => AnalyzeSelection::Enums,
                        (true, true, false) => AnalyzeSelection::FunctionsStructs,
                        (true, false, true) => AnalyzeSelection::FunctionsEnums,
                        (false, true, true) => AnalyzeSelection::StructsEnums,
                        _ => AnalyzeSelection::All,
                    };
                    Self::Inventory { selection }
                }
                ModeCommand::Mcp(_) => unreachable!("Mcp variant intercepted in main.rs before app::run"),
                ModeCommand::Completions(_) => unreachable!("Completions variant intercepted in main.rs before app::run"),
                ModeCommand::GenerateMan => unreachable!("GenerateMan variant intercepted in main.rs before app::run"),
                ModeCommand::Find(find) => {
                    let any = find.func || find.struct_only || find.enum_only;
                    let selection = if !any {
                        AnalyzeSelection::All
                    } else {
                        match (find.func, find.struct_only, find.enum_only) {
                            (true, false, false) => AnalyzeSelection::Functions,
                            (false, true, false) => AnalyzeSelection::Structs,
                            (false, false, true) => AnalyzeSelection::Enums,
                            (true, true, false) => AnalyzeSelection::FunctionsStructs,
                            (true, false, true) => AnalyzeSelection::FunctionsEnums,
                            (false, true, true) => AnalyzeSelection::StructsEnums,
                            _ => AnalyzeSelection::All,
                        }
                    };
                    Self::Find(FindRequest {
                        query: find.query,
                        threshold: args.search_threshold,
                        selection,
                        max_results: find.max_results,
                        show_ids: find.show_ids,
                        exact: find.exact,
                        in_path: find.in_path,
                    })
                }
            };
        }

        if let Some(query) = args.ensemble.clone() {
            Self::Ensemble(EnsembleRequest {
                query,
                symbol_id: None,
                preset: args.ensemble_preset,
                view: args.ensemble_view,
                lifecycle_types: args.ensemble_lifecycle_types.clone(),
                target_in: None,
                sections: Vec::new(),
                boundaries_min_score: 0.7,
                no_boundaries: false,
                all: false,
                ambiguity_cap: 7,
            })
        } else if let Some(query) = args.callers.clone() {
            Self::Callers(CallersRequest {
                query,
                symbol_id: None,
                before_context: 0,
                after_context: 0,
                target_in: None,
                callers_in: None,
                depth: 1,
                all: false,
                ambiguity_cap: 7,
                max_results: None,
                flat: false,
            })
        } else if args.call_graph {
            Self::CallGraph {
                detail: args.call_graph_detail,
                config: CallGraphConfig::default(),
            }
        } else if args.dead_code {
            Self::DeadCode {
                in_path: None,
                verbose: false,
                max_results: 50,
            }
        } else {
            Self::Inventory {
                selection: AnalyzeSelection::from_args(args),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(args).expect("parse")
    }

    #[test]
    fn analyze_selection_default_is_all() {
        let args = parse(&["rustgraph"]);
        let sel = AnalyzeSelection::from_args(&args);
        assert_eq!(sel, AnalyzeSelection::All);
        assert!(sel.show_functions());
        assert!(sel.show_structs());
        assert!(sel.show_enums());
    }

    #[test]
    fn analyze_selection_func_shortcut_only_functions() {
        let args = parse(&["rustgraph", "--func"]);
        let sel = AnalyzeSelection::from_args(&args);
        assert_eq!(sel, AnalyzeSelection::Functions);
        assert!(sel.show_functions());
        assert!(!sel.show_structs());
        assert!(!sel.show_enums());
    }

    #[test]
    fn analyze_selection_struct_and_enum_shortcuts_combine() {
        let args = parse(&["rustgraph", "--struct", "--enum"]);
        let sel = AnalyzeSelection::from_args(&args);
        assert_eq!(sel, AnalyzeSelection::StructsEnums);
        assert!(!sel.show_functions());
        assert!(sel.show_structs());
        assert!(sel.show_enums());
    }

    #[test]
    fn analyze_selection_func_struct_combines() {
        let args = parse(&["rustgraph", "--func", "--struct"]);
        let sel = AnalyzeSelection::from_args(&args);
        assert_eq!(sel, AnalyzeSelection::FunctionsStructs);
    }

    #[test]
    fn analyze_selection_func_enum_combines() {
        let args = parse(&["rustgraph", "--func", "--enum"]);
        let sel = AnalyzeSelection::from_args(&args);
        assert_eq!(sel, AnalyzeSelection::FunctionsEnums);
    }

    #[test]
    fn analyze_selection_all_shortcuts_yield_all() {
        let args = parse(&["rustgraph", "--func", "--struct", "--enum"]);
        let sel = AnalyzeSelection::from_args(&args);
        assert_eq!(sel, AnalyzeSelection::All);
    }

    #[test]
    fn analyze_selection_via_analyze_flag() {
        let args = parse(&["rustgraph", "--analyze", "structs"]);
        let sel = AnalyzeSelection::from_args(&args);
        assert_eq!(sel, AnalyzeSelection::Structs);
    }

    #[test]
    fn execution_mode_defaults_to_inventory() {
        let args = parse(&["rustgraph"]);
        match ExecutionMode::from_args(&args) {
            ExecutionMode::Inventory { selection } => {
                assert_eq!(selection, AnalyzeSelection::All);
            }
            _ => panic!("expected inventory mode"),
        }
    }

    #[test]
    fn execution_mode_dead_code_via_legacy_flag() {
        let args = parse(&["rustgraph", "--dead-code"]);
        assert!(matches!(ExecutionMode::from_args(&args), ExecutionMode::DeadCode { .. }));
    }

    #[test]
    fn execution_mode_call_graph_via_legacy_flag() {
        let args = parse(&["rustgraph", "--call-graph"]);
        match ExecutionMode::from_args(&args) {
            ExecutionMode::CallGraph { .. } => {}
            _ => panic!("expected call graph mode"),
        }
    }

    #[test]
    fn execution_mode_ensemble_via_legacy_flag_carries_preset_and_view() {
        let args = parse(&[
            "rustgraph",
            "--ensemble",
            "foo",
            "--ensemble-preset",
            "deep",
            "--ensemble-view",
            "flow",
        ]);
        match ExecutionMode::from_args(&args) {
            ExecutionMode::Ensemble(req) => {
                assert_eq!(req.query, "foo");
                assert!(req.preset.is_some());
                assert!(req.view.is_some());
            }
            _ => panic!("expected ensemble mode"),
        }
    }

    #[test]
    fn execution_mode_callers_via_legacy_flag_has_zero_context() {
        let args = parse(&["rustgraph", "--callers", "foo"]);
        match ExecutionMode::from_args(&args) {
            ExecutionMode::Callers(req) => {
                assert_eq!(req.query, "foo");
                assert_eq!(req.before_context, 0);
                assert_eq!(req.after_context, 0);
            }
            _ => panic!("expected callers mode"),
        }
    }

    #[test]
    fn execution_mode_subcommand_takes_precedence_over_legacy_flag() {
        let args = parse(&["rustgraph", "--dead-code", "callers", "foo"]);
        match ExecutionMode::from_args(&args) {
            ExecutionMode::Callers(req) => {
                assert_eq!(req.query, "foo");
            }
            _ => panic!("expected callers subcommand to win"),
        }
    }

}
