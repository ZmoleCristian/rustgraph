use std::{fs, path::Path, process::Command};

use rustgraph::{
    project::ProjectData,
    test_coverage::{
        ExecutionState, TestCoverageOptions, TestCoverageReport, import_llvm_coverage,
        map_test_coverage,
    },
};
use serde_json::{Value, json};

fn fixture(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), source).unwrap();
    dir
}

fn report(root: &Path) -> TestCoverageReport {
    map_test_coverage(
        &ProjectData::load(root, false),
        &TestCoverageOptions::default(),
    )
}

fn export(file: &Path, regions: Value) -> Value {
    json!({"type":"llvm.coverage.json.export", "version":"3.1.0", "data":[{
        "functions":[{"filenames":[file], "regions":regions}]
    }]})
}

fn state(report: &TestCoverageReport, name: &str) -> ExecutionState {
    report
        .nodes
        .iter()
        .find(|n| n.name == name)
        .unwrap()
        .execution
        .as_ref()
        .unwrap()
        .state
}

#[test]
fn runtime_overlays_do_not_confuse_observed_zero_missing_or_edge_execution() {
    let dir = fixture(
        "fn caller(flag: bool) { if flag { callee(); } }\nfn callee() {}\nfn zero() {}\nfn absent() {}\n#[test] fn test() { caller(false); zero(); }\n",
    );
    let mut r = report(dir.path());
    let input = export(
        &dir.path().join("src/lib.rs"),
        json!([
            [1, 1, 1, 46, 0, 0, 0, 0],
            [1, 1, 1, 46, 7, 0, 0, 0], // OR across instantiations
            [2, 1, 2, 15, 1, 0, 0, 0],
            [3, 1, 3, 13, 0, 0, 0, 0],
            [4, 1, 4, 15, 99, 0, 0, 4] // Branch region is not function-execution evidence.
        ]),
    );
    import_llvm_coverage(&mut r, &input.to_string(), &[dir.path().into()], "run.json").unwrap();
    assert_eq!(state(&r, "caller"), ExecutionState::Observed);
    assert_eq!(state(&r, "callee"), ExecutionState::Observed);
    assert_eq!(state(&r, "zero"), ExecutionState::NotObserved);
    assert_eq!(state(&r, "absent"), ExecutionState::NotMeasured);
    let summary = r.runtime.as_ref().unwrap();
    assert_eq!(summary.observed_production_functions, 2);
    assert_eq!(summary.not_observed_production_functions, 1);
    assert_eq!(summary.unmeasured_production_functions, 1);
    assert_eq!(summary.mapped_code_regions, 3);
    let dot = r.to_dot();
    for name in ["caller", "callee", "zero", "absent"] {
        let i = r.nodes.iter().position(|n| n.name == name).unwrap();
        let line = dot
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("n{i} [")))
            .unwrap();
        let (fill, style) = match name {
            "caller" => ("#bbf7d0", "dotted"),
            "callee" => ("#bbf7d0", "solid"),
            "zero" => ("#f1f5f9", "dashed"),
            _ => ("#ffffff", "dashed"),
        };
        assert!(line.contains(&format!("fillcolor=\"{fill}\"")), "{line}");
        assert!(line.contains(&format!("rounded,filled,{style}")), "{line}");
    }
    // Both endpoints were observed, but the edge itself still has no runtime proof.
    for edge in dot.lines().filter(|l| l.contains(" -> ")) {
        assert!(!edge.contains("style=solid"));
    }
    assert!(
        r.to_text()
            .contains("2 production functions observed, 1 not observed, 1 without instrumentation")
    );
    assert!(r.execution_coverage_percent.is_none());
}

#[test]
fn llvm_file_indices_and_same_basename_files_are_not_confused() {
    let dir = fixture("fn a() {}\n");
    fs::create_dir_all(dir.path().join("src/other")).unwrap();
    fs::write(dir.path().join("src/other/lib.rs"), "fn b() {}\n").unwrap();
    let mut r = report(dir.path());
    let input = json!({"type":"llvm.coverage.json.export", "version":"3.0.1", "data":[{"functions":[{
        "filenames":["/external/src/lib.rs",dir.path().join("src/other/lib.rs")],
        "regions":[[1,1,1,10,9,0,0,0], [1,1,1,10,2,1,0,0]]
    }]}]});
    import_llvm_coverage(&mut r, &input.to_string(), &[dir.path().into()], "run.json").unwrap();
    assert_eq!(state(&r, "a"), ExecutionState::NotMeasured);
    assert_eq!(state(&r, "b"), ExecutionState::Observed);
}

#[test]
fn ambiguous_same_line_definitions_do_not_receive_execution_proof() {
    let dir = fixture("fn a() {} fn b() {}\nfn c() {}\n");
    let mut r = report(dir.path());
    let input = export(
        &dir.path().join("src/lib.rs"),
        json!([[1, 1, 1, 10, 1, 0, 0, 0], [2, 1, 2, 10, 1, 0, 0, 0]]),
    );
    import_llvm_coverage(&mut r, &input.to_string(), &[dir.path().into()], "run.json").unwrap();
    assert_eq!(state(&r, "a"), ExecutionState::NotMeasured);
    assert_eq!(state(&r, "b"), ExecutionState::NotMeasured);
    assert_eq!(state(&r, "c"), ExecutionState::Observed);
    assert_eq!(r.runtime.as_ref().unwrap().unmatched_code_regions, 1);
}

#[test]
fn malformed_or_unrelated_exports_fail_without_changing_the_report() {
    let dir = fixture("fn a() {}\n");
    let file = dir.path().join("src/lib.rs");
    let inputs = [
        json!({"data":[]}),
        export(&file, json!([[1, 1, 1, 10, 1, 99, 0, 0]])),
        export(&file, json!([[2, 1, 1, 10, 1, 0, 0, 0]])),
        export(&file, json!([[100, 1, 100, 10, 1, 0, 0, 0]])),
        export(
            Path::new("/other/src/lib.rs"),
            json!([[1, 1, 1, 10, 1, 0, 0, 0]]),
        ),
    ];
    for input in inputs {
        let mut r = report(dir.path());
        assert!(
            import_llvm_coverage(&mut r, &input.to_string(), &[dir.path().into()], "bad.json")
                .is_err()
        );
        assert!(r.runtime.is_none());
        assert!(r.nodes.iter().all(|n| n.execution.is_none()));
    }
}

#[test]
fn cli_imports_runtime_evidence_and_rejects_per_test_filtering() {
    let dir = fixture("fn a() {}\n");
    let input = export(
        &dir.path().join("src/lib.rs"),
        json!([[1, 1, 1, 10, 1, 0, 0, 0]]),
    );
    fs::write(dir.path().join("llvm.json"), input.to_string()).unwrap();
    fs::write(
        dir.path().join("llvm.json.sources.json"),
        json!({
            "format": "rustgraph.coverage.sources.v1",
            "files": { dir.path().join("src/lib.rs").to_string_lossy().as_ref(): "fn a() {}\n" }
        })
        .to_string(),
    )
    .unwrap();
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rustgraph"))
            .current_dir(dir.path())
            .args([
                "-p",
                ".",
                "test-coverage",
                "--llvm-coverage",
                "llvm.json",
                "-j",
            ])
            .args(extra)
            .output()
            .unwrap()
    };
    let output = run(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["runtime"]["observed_production_functions"], 1);
    assert_eq!(report["nodes"][0]["execution"]["state"], "observed");
    assert!(!run(&["--test", "one"]).status.success());
    let dot = fs::read_to_string(dir.path().join("test-coverage.dot")).unwrap();
    assert!(dot.contains("Test reachability · measured"));
    if Command::new("dot").arg("-V").output().is_ok() {
        for format in ["svg", "png"] {
            let result = Command::new("dot")
                .current_dir(dir.path())
                .args([
                    &format!("-T{format}"),
                    "test-coverage.dot",
                    "-o",
                    &format!("coverage.{format}"),
                ])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    fs::write(dir.path().join("src/lib.rs"), "fn a() { panic!(); }\n").unwrap();
    let stale = run(&[]);
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("coverage is stale"));
}
