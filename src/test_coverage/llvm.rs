//! LLVM code-region counters establish execution inside a source function.
//! They do not establish caller/callee edges, branch completeness, or per-test attribution.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::TestCoverageReport;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    Observed,
    NotObserved,
    NotMeasured,
}

#[derive(Debug, Serialize)]
pub struct FunctionExecution {
    pub state: ExecutionState,
    /// Unique source code regions, unified across generic instantiations/objects.
    pub instrumented_regions: usize,
    pub executed_regions: usize,
}

#[derive(Debug, Serialize)]
pub struct RuntimeCoverage {
    pub source: String,
    pub format_version: String,
    pub observed_production_functions: usize,
    pub not_observed_production_functions: usize,
    pub unmeasured_production_functions: usize,
    pub mapped_code_regions: usize,
    /// Code regions in indexed files that could not be assigned unambiguously.
    pub unmatched_code_regions: usize,
    pub source_verification: &'static str,
    pub test_attribution: &'static str,
    pub branch_coverage: &'static str,
}

#[derive(Deserialize)]
struct Export {
    #[serde(rename = "type")]
    kind: String,
    version: String,
    data: Vec<ExportData>,
}

#[derive(Deserialize)]
struct ExportData {
    functions: Vec<ExportFunction>,
}

#[derive(Deserialize)]
struct ExportFunction {
    filenames: Vec<String>,
    // [start line, start column, end line, end column, count, file id,
    //  expanded file id, region kind]. LLVM uses 1-based lines and columns.
    regions: Vec<[u64; 8]>,
}

#[derive(Deserialize)]
struct SourceSnapshot {
    format: String,
    files: BTreeMap<PathBuf, String>,
}

/// Check the source snapshot captured before instrumented compilation and after the run.
/// The snapshot deliberately stores exact text instead of relying on timestamps.
pub fn verify_coverage_sources(json: &str, project_roots: &[PathBuf]) -> Result<(), String> {
    let snapshot: SourceSnapshot =
        serde_json::from_str(json).map_err(|e| format!("invalid coverage source snapshot: {e}"))?;
    if snapshot.format != "rustgraph.coverage.sources.v1" || snapshot.files.is_empty() {
        return Err("invalid or empty coverage source snapshot".into());
    }
    let roots = project_roots
        .iter()
        .map(|p| absolute_path(p))
        .collect::<Result<Vec<_>, _>>()?;
    for (path, original) in &snapshot.files {
        if !path.is_absolute() || !roots.iter().any(|root| path.starts_with(root)) {
            return Err(format!(
                "coverage snapshot path is outside project roots: {}",
                path.display()
            ));
        }
        let current = std::fs::read_to_string(path).map_err(|e| {
            format!(
                "coverage source {}: {e}; recollect coverage",
                path.display()
            )
        })?;
        if current != *original {
            return Err(format!(
                "coverage is stale: {} changed; recollect coverage",
                path.display()
            ));
        }
    }
    Ok(())
}

/// Attach aggregate LLVM execution evidence to the existing complete inventory.
/// Paths must match exactly after normalization; basename/suffix matches are never used.
/// Source checksums are absent from LLVM JSON: callers must use the same source revision.
/// On invalid or unrelated input, leave the report unchanged and return an error.
pub fn import_llvm_coverage(
    report: &mut TestCoverageReport,
    json: &str,
    project_roots: &[PathBuf],
    source: &str,
) -> Result<(), String> {
    if report.runtime.is_some() {
        return Err("LLVM execution evidence is already attached to this report".into());
    }
    let export: Export = serde_json::from_str(json).map_err(|e| {
        format!(
            "invalid LLVM coverage export (use full `llvm-cov export`, not --summary-only): {e}"
        )
    })?;
    if export.kind != "llvm.coverage.json.export" || !export.version.starts_with("3.") {
        return Err(format!(
            "unsupported coverage format: {} {}",
            export.kind, export.version
        ));
    }
    let Some(primary) = project_roots.first() else {
        return Err("LLVM import requires a project root".into());
    };
    let primary = absolute_path(primary)?;
    let roots = project_roots
        .iter()
        .map(|p| absolute_path(p))
        .collect::<Result<Vec<_>, _>>()?;
    let mut files: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, node) in report.nodes.iter().enumerate() {
        files
            .entry(normalized_string(Path::new(&node.file)))
            .or_default()
            .push(i);
    }
    let mut mapping: HashMap<String, Vec<usize>> = HashMap::new();
    let mut regions: Vec<BTreeMap<[u64; 4], bool>> =
        (0..report.nodes.len()).map(|_| BTreeMap::new()).collect();
    let mut unmatched = BTreeSet::new();
    for data in export.data {
        for function in data.functions {
            for region in function.regions {
                // Only CodeRegion. Expansion/skipped/gap/branch regions do not prove execution.
                if region[7] != 0 {
                    continue;
                }
                let file_id =
                    usize::try_from(region[5]).map_err(|_| "LLVM file index out of range")?;
                let file = function
                    .filenames
                    .get(file_id)
                    .ok_or("LLVM code region references an invalid file id")?;
                let start = (region[0], region[1]);
                let end = (region[2], region[3]);
                if start.0 == 0 || start.1 == 0 || end <= start {
                    return Err("LLVM code region has invalid source coordinates".into());
                }
                let indices = mapping.entry(file.clone()).or_insert_with(|| {
                    let absolute = normalize_path(&primary.join(file));
                    let mut keys = vec![normalized_string(&absolute)];
                    for (i, root) in roots.iter().enumerate() {
                        if let Ok(relative) = absolute.strip_prefix(root) {
                            if i == 0 {
                                keys.push(normalized_string(relative));
                            } else if let Some(name) = root.file_name() {
                                keys.push(normalized_string(&PathBuf::from(name).join(relative)));
                            }
                        }
                    }
                    keys.into_iter()
                        .filter_map(|key| files.get(&key))
                        .flatten()
                        .copied()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect()
                });
                if indices.is_empty() {
                    continue;
                } // Dependencies or files outside this inventory.
                // Select the narrowest unique enclosing source function. Ambiguous same-line
                // definitions are deliberately unmeasured, never assigned to both functions.
                let candidates: Vec<_> = indices
                    .iter()
                    .copied()
                    .filter(|&i| {
                        let n = &report.nodes[i];
                        n.line as u64 <= start.0 && end.0 <= n.end_line as u64
                    })
                    .collect();
                let narrowest = candidates
                    .iter()
                    .map(|&i| report.nodes[i].end_line - report.nodes[i].line)
                    .min();
                let owners: Vec<_> = candidates
                    .into_iter()
                    .filter(|&i| Some(report.nodes[i].end_line - report.nodes[i].line) == narrowest)
                    .collect();
                if let [owner] = owners.as_slice() {
                    *regions[*owner]
                        .entry([region[0], region[1], region[2], region[3]])
                        .or_default() |= region[4] > 0;
                } else {
                    unmatched.insert((file.clone(), region[0], region[1], region[2], region[3]));
                }
            }
        }
    }
    let mapped = regions.iter().map(BTreeMap::len).sum();
    if mapped == 0 {
        return Err("no LLVM code regions match the indexed source; check project root, source revision and export paths".into());
    }
    let mut summary = RuntimeCoverage {
        source: source.into(),
        format_version: export.version,
        observed_production_functions: 0,
        not_observed_production_functions: 0,
        unmeasured_production_functions: 0,
        mapped_code_regions: mapped,
        unmatched_code_regions: unmatched.len(),
        source_verification: "exact normalized paths and enclosing source ranges; LLVM JSON has no source checksums",
        test_attribution: "aggregate imported run; no per-test attribution or verification that only tests ran",
        branch_coverage: "not assessed; observed code does not establish all branches/paths",
    };
    for (node, regions) in report.nodes.iter_mut().zip(regions) {
        let executed = regions.values().filter(|&&hit| hit).count();
        let state = if executed > 0 {
            ExecutionState::Observed
        } else if regions.is_empty() {
            ExecutionState::NotMeasured
        } else {
            ExecutionState::NotObserved
        };
        if !node.test_code {
            match state {
                ExecutionState::Observed => summary.observed_production_functions += 1,
                ExecutionState::NotObserved => summary.not_observed_production_functions += 1,
                ExecutionState::NotMeasured => summary.unmeasured_production_functions += 1,
            }
        }
        node.execution = Some(FunctionExecution {
            state,
            instrumented_regions: regions.len(),
            executed_regions: executed,
        });
    }
    report.analysis = "static-test-map-with-llvm-execution";
    report.limitations[0] = "LLVM counts prove execution inside observed functions in the imported run. They do not prove call edges, assertions, complete function execution or all branches/paths.".into();
    report.limitations.push("LLVM import has aggregate run attribution, not per-test attribution. Never promote an edge merely because both endpoints executed.".into());
    report.limitations.push("LLVM JSON has no source checksums: use an export from the same source revision. Missing instrumentation is not evidence of zero execution.".into());
    report.runtime = Some(summary);
    Ok(())
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    Ok(absolute
        .canonicalize()
        .unwrap_or_else(|_| normalize_path(&absolute)))
}

fn normalized_string(path: &Path) -> String {
    normalize_path(path).to_string_lossy().replace('\\', "/")
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if out.file_name().is_some() => {
                out.pop();
            }
            part => out.push(part.as_os_str()),
        }
    }
    out
}
