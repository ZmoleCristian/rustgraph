use std::{fs, path::Path, process::Command};

use rustgraph::{
    project::ProjectData,
    test_coverage::{TestCoverageOptions, TestCoverageReport, map_test_coverage},
};

const SOURCE: &str = r#"
pub fn choose(value: bool) { if value { left(); } else { right(); } }
fn left() { cycle(); }
fn right() {}
fn cycle() { left(); }
fn never_called() {}
fn deferred_only() {}
fn async_only() {}
fn shadowed() {}
fn hidden_in_macro() {}
fn indirect() {}
struct Thing;
impl Thing { fn method(&self) { never_called(); } }
#[cfg(test)]
mod tests {
    use super::*;
    fn helper() { choose(false); }
    fn unused_helper() { never_called(); }
    #[test]
    fn checks_false() {
        helper();
        let _deferred = || deferred_only();
        let _future = async { async_only(); };
        let shadowed = || {};
        shadowed();
        let obj = Thing;
        obj.method();
        assert!(hidden_in_macro());
        external::never_called();
        (indirect)();
    }
    #[tokio::test]
    async fn async_test() { right(); }
    #[test]
    #[ignore = "manual"]
    fn ignored_test() { never_called(); }
}
"#;

fn fixture(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), source).unwrap();
    dir
}

fn node<'a>(
    report: &'a TestCoverageReport,
    name: &str,
) -> &'a rustgraph::test_coverage::CoverageNode {
    report.nodes.iter().find(|n| n.name == name).unwrap()
}

#[test]
fn maps_possible_paths_without_claiming_value_or_branch_coverage() {
    let dir = fixture(SOURCE);
    let report = map_test_coverage(
        &ProjectData::load(dir.path(), false),
        &TestCoverageOptions::default(),
    );
    assert_eq!(report.nodes.iter().filter(|n| n.selected_root).count(), 2);
    assert!(!node(&report, "helper").selected_root);
    assert!(!node(&report, "unused_helper").selected_root);
    assert!(node(&report, "choose").possible_from_tests.len() == 1);
    // false was passed, but this analysis must NOT pretend it solved that value.
    assert!(!node(&report, "left").possible_from_tests.is_empty());
    assert_eq!(node(&report, "right").possible_from_tests.len(), 2);
    assert!(!node(&report, "cycle").possible_from_tests.is_empty());
    assert!(
        node(&report, "choose")
            .unknown_flow
            .iter()
            .any(|f| f.kind.contains("if alternatives"))
    );
    assert_eq!(report.execution_coverage_percent, None);
    assert!(report.to_text().contains("NOT a coverage percentage"));
    assert!(report.to_text().contains("NO KNOWN PATH"));
    assert!(report.to_dot().contains("Execution proof (none)"));
}

#[test]
fn blind_spots_do_not_create_phantom_reachability() {
    let dir = fixture(SOURCE);
    let report = map_test_coverage(
        &ProjectData::load(dir.path(), false),
        &TestCoverageOptions::default(),
    );
    for name in [
        "deferred_only",
        "async_only",
        "shadowed",
        "method",
        "never_called",
        "hidden_in_macro",
        "indirect",
    ] {
        assert!(
            node(&report, name).possible_from_tests.is_empty(),
            "phantom: {name}"
        );
    }
    for reason in [
        "deferred",
        "receiver",
        "macro expansion",
        "local binding",
        "external",
        "indirect",
    ] {
        assert!(
            report.blind_spots.iter().any(|s| s.reason.contains(reason)),
            "missing: {reason}"
        );
    }
    assert!(node(&report, "ignored_test").ignored);
    assert!(!node(&report, "ignored_test").selected_root);
}

#[test]
fn ignored_test_opt_in_and_root_filter_are_explicit() {
    let dir = fixture(SOURCE);
    let report = map_test_coverage(
        &ProjectData::load(dir.path(), false),
        &TestCoverageOptions {
            test_filter: Some("ignored_test".into()),
            include_ignored_tests: true,
        },
    );
    assert_eq!(report.nodes.iter().filter(|n| n.selected_root).count(), 1);
    assert_eq!(node(&report, "never_called").possible_from_tests.len(), 1);
    assert!(node(&report, "choose").possible_from_tests.is_empty());
}

#[test]
fn records_control_flow_without_leaking_nested_function_branches() {
    let dir = fixture(
        r#"
fn flow(x: bool, value: Result<(), ()>) -> Result<(), ()> {
    fn nested() { if true {} }
    match x { true if x => (), _ => () }
    let _ = x && false || true;
    for _ in 0..2 { continue; }
    while x { break; }
    value?;
    return Ok(());
}
#[test] fn entry() { flow(false, Ok(())); }
"#,
    );
    let report = map_test_coverage(
        &ProjectData::load(dir.path(), false),
        &TestCoverageOptions::default(),
    );
    let flow = node(&report, "flow");
    for kind in ["match", "short circuit", "loop", "?:", "early exit"] {
        assert!(
            flow.unknown_flow.iter().any(|f| f.kind.contains(kind)),
            "missing {kind}"
        );
    }
    assert!(
        !flow
            .unknown_flow
            .iter()
            .any(|f| f.kind.contains("if alternatives"))
    );
    assert!(node(&report, "nested").possible_from_tests.is_empty());
}

fn cli(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rustgraph"))
        .current_dir(root)
        .args(["--no-auto-path", "-p", "."])
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn cli_writes_text_json_and_renderable_deterministic_dot() {
    let dir = fixture(SOURCE);
    let output = cli(dir.path(), &["test-coverage"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("2 selected annotated tests"));
    let dot = fs::read_to_string(dir.path().join("test-coverage.dot")).unwrap();
    let output = cli(dir.path(), &["test-coverage", "--dot", "second.dot", "-j"]);
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["execution_coverage_percent"].is_null());
    assert_eq!(
        dot,
        fs::read_to_string(dir.path().join("second.dot")).unwrap()
    );
    // Graphviz is optional for consumers; when available, verify both requested formats.
    if Command::new("dot").arg("-V").output().is_ok() {
        for format in ["svg", "png"] {
            let result = Command::new("dot")
                .current_dir(dir.path())
                .args([
                    &format!("-T{format}"),
                    "second.dot",
                    "-o",
                    &format!("map.{format}"),
                ])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(
                fs::metadata(dir.path().join(format!("map.{format}")))
                    .unwrap()
                    .len()
                    > 100
            );
        }
    }
}

#[test]
fn invalid_filters_and_same_output_paths_fail_without_writing() {
    let dir = fixture(SOURCE);
    for args in [
        vec!["test-coverage", "--exclude-tests"],
        vec!["test-coverage", "--search", "choose"],
        vec!["test-coverage", "--changed"],
        vec!["test-coverage", "-o", "test-coverage.dot"],
    ] {
        assert!(!cli(dir.path(), &args).status.success());
        assert!(!dir.path().join("test-coverage.dot").exists());
    }
}

#[test]
fn empty_and_partial_indexes_expose_missing_evidence() {
    let dir = fixture("fn no_tests() {}");
    fs::write(dir.path().join("src/broken.rs"), "fn {").unwrap();
    let report = map_test_coverage(
        &ProjectData::load(dir.path(), false),
        &TestCoverageOptions::default(),
    );
    assert_eq!(report.parse_errors.len(), 1);
    assert!(
        report
            .nodes
            .iter()
            .all(|n| n.possible_from_tests.is_empty())
    );
    assert!(report.to_dot().contains("1 parse failures"));
    assert!(report.to_text().contains("0 selected annotated tests"));
}

#[test]
fn dot_escapes_source_names_and_paths() {
    let dir = fixture("fn production() {}");
    let mut report = map_test_coverage(
        &ProjectData::load(dir.path(), false),
        &TestCoverageOptions::default(),
    );
    report.nodes[0].name = "quote\"slash\\line\n".into();
    assert!(report.to_dot().contains("quote\\\"slash\\\\line\\n"));
}

#[test]
fn graph_keeps_production_inventory_without_test_nodes_or_diagnostic_labels() {
    let dir = fixture(SOURCE);
    let project = ProjectData::load(dir.path(), false);
    for filter in ["checks_false", "no_matching_test"] {
        let report = map_test_coverage(
            &project,
            &TestCoverageOptions {
                test_filter: Some(filter.into()),
                ..Default::default()
            },
        );
        let dot = report.to_dot();
        assert_eq!(report.nodes.len(), project.functions.len());
        for (i, node) in report.nodes.iter().enumerate() {
            let definition = dot
                .lines()
                .find(|line| line.trim_start().starts_with(&format!("n{i} [")));
            if node.test_code {
                assert!(
                    definition.is_none(),
                    "test code leaked into graph: {}",
                    node.name
                );
                continue;
            }
            let definition = definition.unwrap();
            assert!(
                definition.contains(&format!("label=\"{}\", tooltip=", node.name)),
                "verbose label: {definition}"
            );
            let lit = node.selected_root || !node.possible_from_tests.is_empty();
            assert_eq!(
                definition.contains("fontcolor=\"#0f172a\""),
                lit,
                "{}",
                node.name
            );
            assert_eq!(
                definition.contains("fillcolor=\"#f1f5f9\""),
                !lit,
                "{}",
                node.name
            );
        }
        for line in dot
            .lines()
            .filter(|line| line.trim_start().starts_with('n') && line.contains(" -> "))
        {
            assert!(!line.contains("style=solid"), "execution invented: {line}");
        }
        assert!(!dot.lines().any(|line| line.trim_start().starts_with('u')));
        assert!(report.to_text().contains("macro expansion not analyzed"));
    }
}

#[test]
fn split_borders_and_outgoing_lines_are_dotted_but_sequential_calls_are_not() {
    let dir = fixture(
        r#"
fn branch(v: bool) { if v { first(); } else { second(); } }
fn sequential() { first(); second(); }
fn one_arm(v: bool) { match v { _ => first() } }
fn first() {}
fn second() {}
#[test] fn entry() { branch(false); sequential(); one_arm(true); }
"#,
    );
    let report = map_test_coverage(
        &ProjectData::load(dir.path(), false),
        &TestCoverageOptions::default(),
    );
    let dot = report.to_dot();
    for name in ["branch", "sequential", "one_arm"] {
        let index = report
            .nodes
            .iter()
            .position(|node| node.name == name)
            .unwrap();
        let split = name == "branch";
        assert_eq!(report.nodes[index].has_path_splits(), split);
        let definition = dot
            .lines()
            .find(|line| line.trim_start().starts_with(&format!("n{index} [")))
            .unwrap();
        let style = if split { "dotted" } else { "dashed" };
        assert!(
            definition.contains(&format!("style=\"rounded,filled,{style}\"")),
            "{definition}"
        );
        let edges: Vec<_> = dot
            .lines()
            .filter(|line| line.trim_start().starts_with(&format!("n{index} -> n")))
            .collect();
        assert!(!edges.is_empty());
        for edge in edges {
            assert!(edge.contains(&format!("style={style}")), "{edge}");
        }
    }
    assert!(dot.contains("Split</TD>"));
    assert!(dot.contains("Execution proof (none)"));
}
