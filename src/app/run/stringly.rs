use std::collections::HashSet;

use serde_json::json;

use super::super::modes::StringlyRequest;
use super::super::project::ProjectData;
use crate::cli::Args;
use crate::stringly::{
    StringlyConfig, StringlyFinding, StringlySuppressionCounts, TypeOrigin, detect_stringly,
};

/// Run the evidence-ranked stringly-typing detector and render either a compact
/// human report or a stable JSON envelope.
pub fn run(
    args: &Args,
    project: &ProjectData,
    request: StringlyRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut roots = Vec::with_capacity(1 + args.also.len());
    roots.push(args.path.clone());
    roots.extend(args.also.iter().cloned());
    let report = detect_stringly(
        project,
        &StringlyConfig {
            project_roots: roots,
            min_confidence: request.min_confidence,
            include_tests: !args.exclude_tests,
            include_suppressed: request.include_suppressed,
        },
    );

    let scanned_string_sites = report.scanned_string_sites;
    let suppressed_findings = report.suppressed_findings;
    let suppression_counts = report.suppression_counts.clone();

    let allowed_origins: HashSet<TypeOrigin> = request.origins.into_iter().collect();
    let mut findings: Vec<StringlyFinding> = report
        .findings
        .into_iter()
        .map(|mut finding| {
            if !args.absolute_paths {
                finding.file_path = super::super::relativize_for_display(
                    &finding.file_path,
                    &args.path,
                    &args.also,
                );
                for evidence in &mut finding.evidence {
                    evidence.file_path = super::super::relativize_for_display(
                        &evidence.file_path,
                        &args.path,
                        &args.also,
                    );
                }
            }
            finding
        })
        .filter(|finding| {
            request
                .in_path
                .as_ref()
                .is_none_or(|needle| finding.file_path.contains(needle))
        })
        .filter(|finding| {
            allowed_origins.is_empty() || allowed_origins.contains(&finding.suggestion.origin)
        })
        .collect();

    let total_findings = findings.len();
    let origin_counts = origin_counts(&findings);
    let cap = if request.max_results == 0 {
        usize::MAX
    } else {
        request.max_results
    };
    let truncated = findings.len() > cap;
    findings.truncate(cap);

    if args.json {
        let payload = json!({
            "min_confidence": request.min_confidence,
            "include_suppressed": request.include_suppressed,
            "scanned_string_sites": scanned_string_sites,
            "suppressed_findings": suppressed_findings,
            "suppression_counts": {
                "generated-code": suppression_counts.generated_code,
                "vendored-code": suppression_counts.vendored_code,
                "trait-contract": suppression_counts.trait_contract,
                "text-boundary": suppression_counts.text_boundary,
                "implementation-detail": suppression_counts.implementation_detail,
            },
            "total_findings": total_findings,
            "truncated": truncated,
            "origin_counts": {
                "standard-library": origin_counts[0],
                "existing-dependency": origin_counts[1],
                "external-crate": origin_counts[2],
                "local-type": origin_counts[3],
            },
            "findings": findings,
        });
        let serialized = serde_json::to_string_pretty(&payload)?;
        super::switchboard::write_string_output(args.output.as_deref(), &serialized)?;
    } else {
        let rendered = render_text(
            &findings,
            &TextSummary {
                scanned_string_sites,
                total_findings,
                min_confidence: request.min_confidence,
                counts: origin_counts,
                suppressed_findings,
                suppression_counts,
                truncated,
            },
        );
        super::switchboard::write_string_output(args.output.as_deref(), &rendered)?;
    }

    Ok(())
}

fn origin_counts(findings: &[StringlyFinding]) -> [usize; 4] {
    let mut counts = [0usize; 4];
    for finding in findings {
        let position = match finding.suggestion.origin {
            TypeOrigin::StandardLibrary => 0,
            TypeOrigin::ExistingDependency => 1,
            TypeOrigin::ExternalCrate => 2,
            TypeOrigin::LocalType => 3,
        };
        counts[position] += 1;
    }
    counts
}

struct TextSummary {
    scanned_string_sites: usize,
    total_findings: usize,
    min_confidence: f64,
    counts: [usize; 4],
    suppressed_findings: usize,
    suppression_counts: StringlySuppressionCounts,
    truncated: bool,
}

fn render_text(findings: &[StringlyFinding], summary: &TextSummary) -> String {
    let scanned_string_sites = summary.scanned_string_sites;
    let total_findings = summary.total_findings;
    let min_confidence = summary.min_confidence;
    let counts = summary.counts;
    let suppressed_findings = summary.suppressed_findings;
    let suppression_counts = &summary.suppression_counts;
    let truncated = summary.truncated;
    let mut output = String::new();
    output.push_str(&format!(
        "rustgraph stringly: {} finding(s) from {} String/&str declaration(s) (min confidence {:.2})\n",
        total_findings, scanned_string_sites, min_confidence
    ));
    output.push_str(&format!(
        "origins: standard-library {} | existing-dependency {} | external-crate {} | local-type {}\n",
        counts[0], counts[1], counts[2], counts[3]
    ));
    if suppressed_findings > 0 {
        output.push_str(&format!(
            "normally withheld: {} (generated {} | vendored {} | trait-contract {} | text-boundary {} | implementation-detail {})\n",
            suppressed_findings,
            suppression_counts.generated_code,
            suppression_counts.vendored_code,
            suppression_counts.trait_contract,
            suppression_counts.text_boundary,
            suppression_counts.implementation_detail,
        ));
    }

    if findings.is_empty() {
        output.push_str(
            "(no matching opportunities; lower --min-confidence or widen --origin/--in filters)",
        );
        return output;
    }

    for finding in findings {
        output.push('\n');
        output.push_str(&format!(
            "{}:{} {} {}.{}: {}\n",
            finding.file_path,
            finding.line,
            finding.site_kind.as_str(),
            finding.owner,
            finding.name,
            finding.declared_type,
        ));
        output.push_str(&format!(
            "  -> {} [{}; {:.0}%{}]\n",
            finding.suggestion.display,
            finding.suggestion.origin,
            finding.confidence * 100.0,
            dependency_impact(finding),
        ));
        for caveat in &finding.caveats {
            output.push_str(&format!(
                "  caveat {}{}: {}\n",
                caveat.kind.as_str(),
                if caveat.suppresses_by_default {
                    " (normally withheld)"
                } else {
                    ""
                },
                caveat.message
            ));
        }
        if !finding.literals.is_empty() {
            const MAX_TEXT_LITERALS: usize = 12;
            let shown = finding
                .literals
                .iter()
                .take(MAX_TEXT_LITERALS)
                .map(|literal| format!("{:?}", literal))
                .collect::<Vec<_>>()
                .join(", ");
            let omitted = finding.literals.len().saturating_sub(MAX_TEXT_LITERALS);
            output.push_str(&format!(
                "  literals{}: {}{}\n",
                finding
                    .literal_stats
                    .as_ref()
                    .map_or_else(String::new, |stats| {
                        format!(
                            " ({} distinct / {} observations)",
                            stats.distinct, stats.observations
                        )
                    }),
                shown,
                if omitted > 0 {
                    format!(", +{omitted} more")
                } else {
                    String::new()
                }
            ));
        }
        const MAX_TEXT_EVIDENCE: usize = 10;
        for evidence in finding.evidence.iter().take(MAX_TEXT_EVIDENCE) {
            output.push_str(&format!(
                "  evidence {}:{}: {}\n",
                evidence.file_path, evidence.line, evidence.message
            ));
        }
        if finding.evidence.len() > MAX_TEXT_EVIDENCE {
            output.push_str(&format!(
                "  evidence: +{} more (use --json for all)\n",
                finding.evidence.len() - MAX_TEXT_EVIDENCE
            ));
        }
    }

    if truncated {
        output.push_str(&format!(
            "\n(showing {} of {}; use --max-results 0 for all)",
            findings.len(),
            total_findings
        ));
    }
    output.trim_end().to_string()
}

fn dependency_impact(finding: &StringlyFinding) -> String {
    match finding.suggestion.origin {
        TypeOrigin::StandardLibrary => "; no dependency".to_string(),
        TypeOrigin::ExistingDependency => finding
            .suggestion
            .crate_name
            .as_ref()
            .map(|name| format!("; crate `{name}` already present"))
            .unwrap_or_default(),
        TypeOrigin::ExternalCrate => finding
            .suggestion
            .crate_name
            .as_ref()
            .map(|name| format!("; adds crate `{name}`"))
            .unwrap_or_else(|| "; adds dependency".to_string()),
        TypeOrigin::LocalType if finding.suggestion.local_type_exists => {
            "; existing project type".to_string()
        }
        TypeOrigin::LocalType => "; define locally".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stringly::{StringSiteKind, StringlyEvidence, SuggestedType};

    #[test]
    fn text_output_makes_dependency_cost_visible() {
        let finding = StringlyFinding {
            file_path: "src/lib.rs".into(),
            line: 4,
            name: "callback_url".into(),
            owner: "Config".into(),
            site_kind: StringSiteKind::StructField,
            declared_type: "String".into(),
            suggestion: SuggestedType {
                display: "url::Url".into(),
                canonical_path: "url::Url".into(),
                origin: TypeOrigin::ExternalCrate,
                crate_name: Some("url".into()),
                dependency_alias: None,
                new_dependency: true,
                local_type_exists: false,
            },
            confidence: 0.92,
            evidence: Vec::<StringlyEvidence>::new(),
            literals: Vec::new(),
            literal_stats: None,
            caveats: Vec::new(),
            is_test: false,
        };
        let output = render_text(
            &[finding],
            &TextSummary {
                scanned_string_sites: 1,
                total_findings: 1,
                min_confidence: 0.8,
                counts: [0, 0, 1, 0],
                suppressed_findings: 0,
                suppression_counts: StringlySuppressionCounts::default(),
                truncated: false,
            },
        );
        assert!(output.contains("external-crate; 92%; adds crate `url`"));
    }
}
