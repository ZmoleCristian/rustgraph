//! Static test mapping with optional LLVM execution evidence.

mod llvm;
pub use llvm::verify_coverage_sources;
pub use llvm::{ExecutionState, FunctionExecution, RuntimeCoverage, import_llvm_coverage};

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt::Write;

use serde::Serialize;
use syn::{spanned::Spanned, visit::Visit};

use crate::{function_id, project::ProjectData, resolve_call_targets};

/// Root selection. Helpers in test modules are not roots.
#[derive(Debug, Default, Clone)]
pub struct TestCoverageOptions {
    pub test_filter: Option<String>,
    pub include_ignored_tests: bool,
}

/// A syntactic control-flow site whose alternatives have not been exercised or solved.
#[derive(Debug, Clone, Serialize)]
pub struct UnknownFlow {
    pub kind: String,
    pub line: usize,
    pub column: usize,
    /// A syntactic alternative/iteration, not merely multiple sequential calls.
    pub splits_paths: bool,
}

#[derive(Debug, Serialize)]
pub struct CoverageNode {
    pub id: String,
    pub name: String,
    pub file: String,
    pub line: usize,
    pub end_line: usize,
    pub test_attribute: Option<String>,
    pub ignored: bool,
    pub selected_root: bool,
    pub test_code: bool,
    pub cfg: Vec<String>,
    /// Selected test IDs with a possible syntactic call path to this function.
    pub possible_from_tests: BTreeSet<String>,
    pub unknown_flow: Vec<UnknownFlow>,
    /// None for static-only analysis. Missing instrumentation is distinct from zero counts.
    pub execution: Option<FunctionExecution>,
}

impl CoverageNode {
    pub fn has_path_splits(&self) -> bool {
        self.unknown_flow.iter().any(|flow| flow.splits_paths)
    }

    fn highlighted(&self) -> bool {
        match self.execution.as_ref().map(|e| e.state) {
            Some(ExecutionState::Observed) => true,
            Some(ExecutionState::NotObserved) => false,
            _ => self.selected_root || !self.possible_from_tests.is_empty(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CoverageEdge {
    pub from: String,
    pub to: String,
    pub line: usize,
    pub column: usize,
    /// The existing source resolver is heuristic, not compiler type checking.
    pub evidence: &'static str,
}

#[derive(Debug, Serialize)]
pub struct CoverageBlindSpot {
    pub caller: Option<String>,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub expression: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct TestCoverageReport {
    pub analysis: &'static str,
    /// No combined percentage: source reachability, execution and branch coverage differ.
    pub execution_coverage_percent: Option<f64>,
    pub runtime: Option<RuntimeCoverage>,
    pub limitations: Vec<String>,
    pub nodes: Vec<CoverageNode>,
    pub edges: Vec<CoverageEdge>,
    pub blind_spots: Vec<CoverageBlindSpot>,
    pub parse_errors: Vec<String>,
}

#[derive(Default)]
struct FunctionSyntax {
    test_attribute: Option<String>,
    ignored: bool,
    unknown_flow: Vec<UnknownFlow>,
    deferred_calls: BTreeSet<(usize, usize)>,
    indirect_calls: BTreeSet<(usize, usize)>,
    bindings: BTreeSet<String>,
    macros: Vec<(usize, usize, String)>,
}

#[derive(Default)]
struct SyntaxIndex {
    functions: HashMap<(String, usize), FunctionSyntax>,
}

impl<'ast> Visit<'ast> for SyntaxIndex {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.record(&item.sig, item.span(), &item.attrs, Some(&item.block));
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.record(&item.sig, item.span(), &item.attrs, Some(&item.block));
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.record(&item.sig, item.span(), &item.attrs, item.default.as_ref());
        syn::visit::visit_trait_item_fn(self, item);
    }
}

impl SyntaxIndex {
    fn record(
        &mut self,
        sig: &syn::Signature,
        span: proc_macro2::Span,
        attrs: &[syn::Attribute],
        block: Option<&syn::Block>,
    ) {
        let mut body = BodySyntax::default();
        for input in &sig.inputs {
            if let syn::FnArg::Typed(arg) = input {
                body.visit_pat(&arg.pat);
            }
        }
        if let Some(block) = block {
            body.visit_block(block);
        }
        body.syntax.test_attribute = attrs.iter().find_map(|attr| {
            let path = attr.path();
            path.segments
                .last()
                .filter(|segment| segment.ident == "test")
                .map(|_| {
                    path.segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::")
                })
        });
        body.syntax.ignored = attrs.iter().any(|attr| attr.path().is_ident("ignore"));
        self.functions
            .insert((sig.ident.to_string(), span.start().line), body.syntax);
    }
}

#[derive(Default)]
struct BodySyntax {
    syntax: FunctionSyntax,
    deferred: usize,
}

impl<'ast> Visit<'ast> for BodySyntax {
    // Nested declarations have their own callers and control flow.
    fn visit_item(&mut self, _: &'ast syn::Item) {}

    fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
        self.syntax.bindings.insert(pat.ident.to_string());
        syn::visit::visit_pat_ident(self, pat);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let start = mac.span().start();
        let name = mac
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        self.syntax
            .macros
            .push((start.line, start.column, format!("{name}!")));
        self.syntax.unknown_flow.push(UnknownFlow {
            kind: "macro: expansion and internal control flow unknown".into(),
            line: start.line,
            column: start.column,
            splits_paths: false,
        });
    }

    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        use syn::Expr;
        let kind = match expr {
            Expr::If(_) => Some("if alternatives: condition/value not solved"),
            Expr::Match(_) => Some("match arms/guards: discriminant/value not solved"),
            Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) => {
                Some("loop: iteration count/exit unknown")
            }
            Expr::Try(_) => Some("?: success/error split unknown"),
            Expr::Binary(e) if matches!(e.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => {
                Some("short circuit: RHS execution unknown")
            }
            Expr::Return(_) | Expr::Break(_) | Expr::Continue(_) => {
                Some("early exit: subsequent execution unknown")
            }
            Expr::Closure(_) | Expr::Async(_) => {
                Some("deferred body: invocation/polling unknown; inner calls excluded")
            }
            Expr::Await(_) => Some("await: polling/completion unknown"),
            _ => None,
        };
        if let Some(kind) = kind {
            let splits_paths = match expr {
                Expr::If(_) | Expr::While(_) | Expr::ForLoop(_) | Expr::Loop(_) | Expr::Try(_) => {
                    true
                }
                Expr::Match(e) => e.arms.len() > 1 || e.arms.iter().any(|arm| arm.guard.is_some()),
                Expr::Binary(e) => matches!(e.op, syn::BinOp::And(_) | syn::BinOp::Or(_)),
                _ => false,
            };
            self.syntax.unknown_flow.push(UnknownFlow {
                kind: kind.into(),
                line: expr.span().start().line,
                column: expr.span().start().column,
                splits_paths,
            });
        }
        let deferred = matches!(expr, Expr::Closure(_) | Expr::Async(_));
        self.deferred += usize::from(deferred);
        if matches!(expr, Expr::Call(_) | Expr::MethodCall(_) | Expr::Macro(_)) {
            let key = (expr.span().start().line, expr.span().start().column);
            if self.deferred > 0 {
                self.syntax.deferred_calls.insert(key);
            }
            if let Expr::Call(call) = expr
                && !matches!(call.func.as_ref(), Expr::Path(path) if path.qself.is_none())
            {
                self.syntax.indirect_calls.insert(key);
            }
        }
        syn::visit::visit_expr(self, expr);
        self.deferred -= usize::from(deferred);
    }
}

/// Map selected annotated tests through source-resolved call candidates.
/// Conditions and argument values are deliberately not interpreted. Deferred bodies,
/// receiver dispatch and indirect calls stop traversal and remain explicit blind spots.
pub fn map_test_coverage(
    project: &ProjectData,
    options: &TestCoverageOptions,
) -> TestCoverageReport {
    let mut report = TestCoverageReport {
        analysis: "possible-static-test-reachability",
        execution_coverage_percent: None,
        runtime: None,
        limitations: [
            "No tests were run. Every edge/path is possible source evidence, never proof of execution, assertions, or coverage.",
            "Branch/path coverage UNKNOWN: argument values, conditions, match guards, loop counts, early exits and panics are not solved. Even if(false) remains possible source evidence.",
            "No static path found does NOT mean untested. Unresolved calls can hide additional paths.",
            "Resolution is heuristic (names/modules, not compiler types). Receiver dispatch, local callable bindings and deferred closure/async bodies do not propagate reachability.",
            "Macros/proc macros, callbacks, function pointers, dynamic dispatch, FFI, generated code, doctests and custom harnesses can hide calls/tests. #[...::test] is an annotation candidate, not verified Cargo discovery.",
            "cfg/cfg_attr, features and Cargo targets are not evaluated; mutually exclusive code can coexist. Ignored tests are excluded as roots unless requested. File discovery follows the index ignore rules.",
        ].into_iter().map(str::to_string).collect(),
        nodes: Vec::new(), edges: Vec::new(), blind_spots: Vec::new(), parse_errors: project.parse_errors.clone(),
    };
    let mut syntax_by_id = HashMap::new();
    let mut files: BTreeMap<&str, SyntaxIndex> = BTreeMap::new();
    for function in &project.functions {
        if files.contains_key(function.file_path.as_str()) {
            continue;
        }
        let mut syntax = SyntaxIndex::default();
        if let Some(ast) = project.parsed_file_by_str(&function.file_path) {
            syntax.visit_file(ast);
        } else {
            // Deserialized indexes may not contain the AST cache.
            match std::fs::read_to_string(&function.file_path)
                .ok()
                .and_then(|s| syn::parse_file(&s).ok())
            {
                Some(ast) => syntax.visit_file(&ast),
                None => report.parse_errors.push(format!(
                    "{}: syntax unavailable for test/control-flow discovery",
                    function.file_path
                )),
            }
        }
        files.insert(&function.file_path, syntax);
    }
    for function in &project.functions {
        let id = function_id(function);
        let syntax = files
            .get_mut(function.file_path.as_str())
            .and_then(|file| {
                file.functions
                    .remove(&(function.name.clone(), function.start_line))
            })
            .unwrap_or_default();
        let selected = syntax.test_attribute.is_some()
            && (!syntax.ignored || options.include_ignored_tests)
            && options
                .test_filter
                .as_ref()
                .is_none_or(|filter| id.contains(filter));
        report.nodes.push(CoverageNode {
            id: id.clone(),
            name: function.name.clone(),
            file: function.file_path.clone(),
            line: function.start_line,
            end_line: function.end_line,
            test_attribute: syntax.test_attribute.clone(),
            ignored: syntax.ignored,
            selected_root: selected,
            test_code: function.is_test,
            cfg: function.cfg_attrs.clone(),
            possible_from_tests: BTreeSet::new(),
            unknown_flow: syntax.unknown_flow.clone(),
            execution: None,
        });
        syntax_by_id.insert(id, syntax);
    }
    let resolved: HashMap<_, _> = resolve_call_targets(&project.functions, &project.call_sites)
        .into_iter()
        .map(|site| {
            (
                (site.caller_id, site.line, site.column),
                site.resolved_internal_id,
            )
        })
        .collect();
    for call in &project.call_sites {
        let syntax = call.caller_id.as_ref().and_then(|id| syntax_by_id.get(id));
        let key = (call.line, call.column);
        let target = call
            .caller_id
            .as_ref()
            .and_then(|id| resolved.get(&(id.clone(), call.line, call.column)))
            .and_then(|id| id.as_ref());
        let reason = if call.caller_id.is_none() {
            Some("call outside an indexed function")
        } else if syntax.is_some_and(|s| s.deferred_calls.contains(&key)) {
            Some("deferred closure/async body; invocation or polling not established")
        } else if syntax.is_some_and(|s| s.indirect_calls.contains(&key)) {
            Some("indirect or qualified trait call; target not established")
        } else if call.call_kind.starts_with("method") {
            Some("receiver type/dispatch not established")
        } else if call.call_kind.starts_with("macro") {
            Some("macro expansion not analyzed")
        } else if !call.callee.contains("::")
            && syntax.is_some_and(|s| s.bindings.contains(&call.callee_base))
        {
            Some("local binding may shadow a function; callable target not established")
        } else if target.is_none() {
            Some("external, ambiguous or unresolved target")
        } else {
            None
        };
        if let Some(reason) = reason {
            report.blind_spots.push(CoverageBlindSpot {
                caller: call.caller_id.clone(),
                file: call.file_path.clone(),
                line: call.line,
                column: call.column,
                expression: call.callee.clone(),
                reason: reason.into(),
            });
        } else if let (Some(from), Some(to)) = (&call.caller_id, target) {
            report.edges.push(CoverageEdge {
                from: from.clone(),
                to: to.clone(),
                line: call.line,
                column: call.column,
                evidence: "possible source-resolved call; execution and path feasibility unknown",
            });
        }
    }
    // syn statement macros are not necessarily present in the shared call index.
    // Preserve their missing evidence rather than silently dropping assert!/etc.
    let mut known_spots: BTreeSet<_> = report
        .blind_spots
        .iter()
        .map(|s| (s.caller.clone(), s.line, s.column))
        .collect();
    for node in &report.nodes {
        if let Some(syntax) = syntax_by_id.get(&node.id) {
            for (line, column, expression) in &syntax.macros {
                if known_spots.insert((Some(node.id.clone()), *line, *column)) {
                    report.blind_spots.push(CoverageBlindSpot {
                        caller: Some(node.id.clone()),
                        file: node.file.clone(),
                        line: *line,
                        column: *column,
                        expression: expression.clone(),
                        reason: "macro expansion not analyzed".into(),
                    });
                }
            }
        }
    }
    report.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    report.edges.sort_by(|a, b| {
        (&a.from, a.line, a.column, &a.to).cmp(&(&b.from, b.line, b.column, &b.to))
    });
    report.edges.dedup_by(|a, b| {
        a.from == b.from && a.to == b.to && a.line == b.line && a.column == b.column
    });
    report.blind_spots.sort_by(|a, b| {
        (&a.file, a.line, a.column, &a.expression).cmp(&(&b.file, b.line, b.column, &b.expression))
    });
    let node_indices: HashMap<_, _> = report
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.clone(), i))
        .collect();
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &report.edges {
        adjacency.entry(&edge.from).or_default().push(&edge.to);
    }
    let roots: Vec<_> = report
        .nodes
        .iter()
        .filter(|n| n.selected_root)
        .map(|n| n.id.clone())
        .collect();
    for root in roots {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([root.as_str()]);
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(&i) = node_indices.get(id) {
                report.nodes[i].possible_from_tests.insert(root.clone());
            }
            if let Some(next) = adjacency.get(id) {
                queue.extend(next.iter().copied());
            }
        }
    }
    report
}

impl TestCoverageReport {
    pub fn to_text(&self) -> String {
        let mut text = if let Some(runtime) = &self.runtime {
            format!(
                "LLVM execution evidence — {} production functions observed, {} not observed, {} without instrumentation.\nBranch/path coverage: UNKNOWN; call edges remain static.\nProfile: {}\nStatic model below (not per-test runtime attribution):\n",
                runtime.observed_production_functions,
                runtime.not_observed_production_functions,
                runtime.unmeasured_production_functions,
                runtime.source
            )
        } else {
            String::from("Static test reachability — execution/branch/path coverage: UNKNOWN\n")
        };
        let roots = self.nodes.iter().filter(|n| n.selected_root).count();
        let production: Vec<_> = self.nodes.iter().filter(|n| !n.test_code).collect();
        let possible = production
            .iter()
            .filter(|n| !n.possible_from_tests.is_empty())
            .count();
        let _ = writeln!(
            text,
            "{roots} selected annotated tests; {possible}/{} production functions with possible paths (NOT a coverage percentage); {} without a known path.\n{} call candidates; {} unresolved/excluded calls; {} parse failures.\n",
            production.len(),
            production.len() - possible,
            self.edges.len(),
            self.blind_spots.len(),
            self.parse_errors.len()
        );
        for limitation in &self.limitations {
            let _ = writeln!(text, "! {limitation}");
        }
        for node in &self.nodes {
            let status = match node.execution.as_ref().map(|e| e.state) {
                Some(ExecutionState::Observed) => "EXECUTED REGION",
                Some(ExecutionState::NotObserved) => "NOT OBSERVED IN RUN",
                Some(ExecutionState::NotMeasured) => "NO RUNTIME DATA",
                None if node.selected_root => "TEST ROOT",
                None if !node.possible_from_tests.is_empty() => "POSSIBLE",
                None => "NO KNOWN PATH",
            };
            let _ = writeln!(
                text,
                "\n[{status}] {} ({}:{}){}",
                node.name,
                node.file,
                node.line,
                if node.ignored { " [ignored]" } else { "" }
            );
            for test in &node.possible_from_tests {
                let _ = writeln!(text, "  <- {test}");
            }
            if let Some(execution) = &node.execution {
                let _ = writeln!(
                    text,
                    "  {}/{} mapped code regions executed (not branch coverage)",
                    execution.executed_regions, execution.instrumented_regions
                );
            }
            if !node.cfg.is_empty() {
                let _ = writeln!(text, "  cfg not evaluated: {}", node.cfg.join(" && "));
            }
            for flow in &node.unknown_flow {
                let _ = writeln!(text, "  ? {}:{} {}", flow.line, flow.column, flow.kind);
            }
        }
        text.push_str("\nBlind spots (calls that do not propagate reachability):\n");
        for spot in &self.blind_spots {
            let _ = writeln!(
                text,
                "  ? {}:{}:{} {} — {}",
                spot.file, spot.line, spot.column, spot.expression, spot.reason
            );
        }
        for error in &self.parse_errors {
            let _ = writeln!(text, "  ! {error}");
        }
        text
    }

    /// Production-only Graphviz. Tests still seed reachability in the full report;
    /// names are the only node labels, while evidence stays in SVG tooltips/text/JSON.
    pub fn to_dot(&self) -> String {
        let mut dot = String::from(
            r##"digraph test_coverage {
  graph [rankdir=LR, bgcolor="#f8fafc", pad=0.25, nodesep=0.16, ranksep=0.4, pack=true, packmode="array_u3", fontname="DejaVu Sans", labelloc=t, fontsize=12, label=<
    <TABLE BORDER="0" CELLBORDER="0" CELLSPACING="8">
      <TR><TD COLSPAN="5"><B>Test reachability · static</B></TD></TR>
      <TR>
        <TD><FONT COLOR="#b45309">●</FONT> Possible from tests</TD>
        <TD><FONT COLOR="#94a3b8">●</FONT> No known test path</TD>
        <TD><FONT COLOR="#b45309">····</FONT> Split</TD>
        <TD>– – Static</TD>
        <TD><FONT COLOR="#15803d">━━</FONT> Execution proof (none)</TD>
      </TR>
    </TABLE>
  >];
  node [shape=box, style="rounded,filled,dashed", fontname="DejaVu Sans", fontsize=10, height=0.32, margin="0.10,0.06"];
  edge [style=dashed, arrowsize=0.6];
"##,
        );
        if self.runtime.is_some() {
            dot = dot.replace("Test reachability · static", "Test reachability · measured")
                .replace("COLSPAN=\"5\"", "COLSPAN=\"6\"")
                .replace("<TR>\n        <TD>", "<TR>\n        <TD><FONT COLOR=\"#15803d\">●</FONT> Executed</TD>\n        <TD>")
                .replace("Possible from tests", "Static only")
                .replace("No known test path", "Not observed")
                .replace("– – Static", "– – Static edges")
                .replace("<FONT COLOR=\"#15803d\">━━</FONT> Execution proof (none)", "<FONT COLOR=\"#94a3b8\">○</FONT> No data");
        }
        let _ = writeln!(
            dot,
            "  graph [tooltip={}];",
            dot_quote(&self.limitations.join("\n"))
        );
        if !self.parse_errors.is_empty() {
            dot = dot.replacen(
                "    </TABLE>",
                &format!("      <TR><TD COLSPAN=\"{}\">{} parse failures — see report</TD></TR>\n    </TABLE>", if self.runtime.is_some() { 6 } else { 5 }, self.parse_errors.len()),
                1,
            );
        }
        let mut files: BTreeMap<&str, Vec<(usize, &CoverageNode)>> = BTreeMap::new();
        let mut ids = HashMap::new();
        let mut blind_spots: HashMap<&str, Vec<&CoverageBlindSpot>> = HashMap::new();
        for spot in &self.blind_spots {
            if let Some(caller) = spot.caller.as_deref() {
                blind_spots.entry(caller).or_default().push(spot);
            }
        }
        for (i, node) in self.nodes.iter().enumerate().filter(|(_, n)| !n.test_code) {
            files.entry(&node.file).or_default().push((i, node));
            ids.insert(node.id.as_str(), i);
        }
        for (cluster, (file, nodes)) in files.into_iter().enumerate() {
            let _ = writeln!(
                dot,
                "  subgraph cluster_{cluster} {{\n    label={}; color=\"#e2e8f0\"; fontcolor=\"#64748b\"; fontsize=10; style=rounded;",
                dot_quote(file)
            );
            for (i, node) in nodes {
                let execution = node.execution.as_ref().map(|e| e.state);
                let (fill, color, font, width) = match execution {
                    Some(ExecutionState::Observed) => ("#bbf7d0", "#15803d", "#0f172a", "2"),
                    _ if node.highlighted() => ("#fde68a", "#b45309", "#0f172a", "2"),
                    Some(ExecutionState::NotMeasured) => ("#ffffff", "#cbd5e1", "#64748b", "1"),
                    _ => ("#f1f5f9", "#cbd5e1", "#64748b", "1"),
                };
                let border = if node.has_path_splits() {
                    "dotted"
                } else if execution == Some(ExecutionState::Observed) {
                    "solid"
                } else {
                    "dashed"
                };
                let mut tooltip =
                    format!("{}\ncfg not evaluated: {}", node.id, node.cfg.join(" && "));
                if let Some(e) = &node.execution {
                    let _ = write!(
                        tooltip,
                        "\nRuntime: {:?}; {}/{} mapped code regions executed. No branch or per-test attribution.",
                        e.state, e.executed_regions, e.instrumented_regions
                    );
                } else {
                    tooltip.push_str("\nExecution unverified");
                }
                for test in &node.possible_from_tests {
                    let _ = write!(tooltip, "\nPossible from: {test}");
                }
                for flow in &node.unknown_flow {
                    let _ = write!(tooltip, "\nL{}: {}", flow.line, flow.kind);
                }
                if let Some(spots) = blind_spots.get(node.id.as_str()) {
                    for spot in spots {
                        let _ = write!(
                            tooltip,
                            "\nL{} {}: {}",
                            spot.line, spot.expression, spot.reason
                        );
                    }
                }
                let _ = writeln!(
                    dot,
                    "    n{i} [style=\"rounded,filled,{border}\", fillcolor=\"{fill}\", color=\"{color}\", fontcolor=\"{font}\", penwidth={width}, label={}, tooltip={}];",
                    dot_quote(&node.name),
                    dot_quote(&tooltip)
                );
            }
            dot.push_str("  }\n");
        }
        // Coalesce repeated call sites without inventing edges through hidden test helpers.
        let mut edges: BTreeMap<(usize, usize), Vec<&CoverageEdge>> = BTreeMap::new();
        for edge in &self.edges {
            if let (Some(&from), Some(&to)) =
                (ids.get(edge.from.as_str()), ids.get(edge.to.as_str()))
            {
                edges.entry((from, to)).or_default().push(edge);
            }
        }
        for ((from, to), sites) in edges {
            let caller = &self.nodes[from];
            let style = if caller.has_path_splits() {
                "dotted"
            } else {
                "dashed"
            };
            let (color, width) = if caller.highlighted() {
                ("#b45309", "1.8")
            } else {
                ("#cbd5e1", "1")
            };
            let mut tooltip = sites
                .iter()
                .map(|site| format!("L{}:{}: {}", site.line, site.column, site.evidence))
                .collect::<Vec<_>>()
                .join("\n");
            if caller.has_path_splits() {
                tooltip.push_str("\nCaller has unresolved splits; outgoing calls marked conservatively, not exact branch assignments.");
            }
            let _ = writeln!(
                dot,
                "  n{from} -> n{to} [style={style}, color=\"{color}\", penwidth={width}, tooltip={}];",
                dot_quote(&tooltip)
            );
        }
        dot.push_str("}\n");
        dot
    }
}

fn dot_quote(value: &str) -> String {
    let mut escaped = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => {}
            '\t' => escaped.push(' '),
            c if c.is_control() => escaped.push(' '),
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}
