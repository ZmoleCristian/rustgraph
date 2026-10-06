use crate::cli::{Args, TestCoverageCommand};
use crate::project::ProjectData;
use crate::test_coverage::{
    TestCoverageOptions, import_llvm_coverage, map_test_coverage, verify_coverage_sources,
};

pub fn run(
    args: &Args,
    project: &ProjectData,
    request: TestCoverageCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.output.as_ref().is_some_and(|path| {
        let absolute = |p: &std::path::Path| -> std::io::Result<std::path::PathBuf> {
            if p.exists() {
                p.canonicalize()
            } else {
                std::path::absolute(p)
            }
        };
        absolute(path).ok() == absolute(&request.dot).ok()
    }) {
        return Err("--output and --dot must name different files".into());
    }
    let mut report = map_test_coverage(
        project,
        &TestCoverageOptions {
            test_filter: request.test,
            include_ignored_tests: request.include_ignored_tests,
        },
    );
    if let Some(path) = &request.llvm_coverage {
        let mut roots = vec![args.path.clone()];
        roots.extend(args.also.iter().cloned());
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(".sources.json");
        let snapshot = std::fs::read_to_string(&sidecar).map_err(|e| {
            format!("coverage source snapshot missing: {e}; collect with tools/test_coverage.py")
        })?;
        verify_coverage_sources(&snapshot, &roots)?;
        let json = std::fs::read_to_string(path)?;
        import_llvm_coverage(&mut report, &json, &roots, &path.to_string_lossy())?;
        if let Some(runtime) = &mut report.runtime {
            runtime.source_verification = "exact source snapshot verified against current files";
        }
    }
    let output = if args.json {
        serde_json::to_string_pretty(&report)?
    } else {
        report.to_text()
    };
    std::fs::write(&request.dot, report.to_dot())?;
    super::switchboard::write_string_output(args.output.as_deref(), &output)?;
    eprintln!(
        "DOT: {} (dot -Tsvg <file.dot> -o coverage.svg; use -Tpng for PNG)",
        request.dot.display()
    );
    Ok(())
}
