//! Plain-text renderer for [`FunctionEnsembleResult`] output.

use crate::FunctionEnsembleResult;

/// Prints a full ensemble result in plain-text format to stdout.
///
/// Emits all available sections: code listing, structs used, call sites, neighborhood
/// (upstream/downstream callers with distances), type lifecycles, dataflow hints, and
/// I/O boundaries.
pub fn print_ensemble_text(ensemble: &FunctionEnsembleResult) {
    print_ensemble_text_inner(ensemble, false);
}

/// Prints an ensemble result in compact summary form, abbreviating the neighborhood section.
///
/// The neighborhood is rendered as single-line upstream/downstream summaries (up to 3 names
/// each) rather than the full per-function listing produced by [`print_ensemble_text`].
pub fn print_ensemble_text_summary_compact(ensemble: &FunctionEnsembleResult) {
    print_ensemble_text_inner(ensemble, true);
}

fn print_ensemble_text_inner(ensemble: &FunctionEnsembleResult, summary_neighborhood: bool) {
    for entry in &ensemble.matches {
        println!(
            "{}:{}-{} - {}",
            entry.info.file_path, entry.info.start_line, entry.info.end_line, entry.info.signature
        );

        if !entry.code.lines.is_empty() {
            println!("\nCode:");
            for line in &entry.code.lines {
                println!("{:>6} | {}", line.line, line.text);
            }
        }

        if !entry.structs_used.is_empty() {
            println!("\nStructs used:");
            for struct_used in &entry.structs_used {
                println!(
                    "{}:{}-{} - struct {}",
                    struct_used.info.file_path,
                    struct_used.info.start_line,
                    struct_used.info.end_line,
                    struct_used.info.name
                );
                for line in &struct_used.code.lines {
                    println!("{:>6} | {}", line.line, line.text);
                }
                println!();
            }
        }

        if entry.call_sites_total > 0 || !entry.call_sites.is_empty() {
            let shown = entry.call_sites.len();
            if entry.call_sites_truncated {
                println!(
                    "Call sites ({} total, showing {}):",
                    entry.call_sites_total, shown
                );
            } else {
                println!("Call sites ({} total):", entry.call_sites_total);
            }

            for call_site in &entry.call_sites {
                println!(
                    "  {}:{}:{} - {}",
                    call_site.file_path,
                    call_site.line,
                    call_site.column,
                    call_site.line_text.trim()
                );
            }
        }

        if entry.neighborhood.upstream_total > 0
            || entry.neighborhood.downstream_total > 0
            || !entry.neighborhood.unresolved_outgoing_calls.is_empty()
        {
            if summary_neighborhood {


                if entry.neighborhood.upstream_total > 0 {
                    let names: Vec<&str> = entry
                        .neighborhood
                        .upstream
                        .iter()
                        .map(|r| r.name.as_str())
                        .take(3)
                        .collect();
                    let extra = entry.neighborhood.upstream_total.saturating_sub(names.len());
                    let extra_part = if extra > 0 {
                        format!(", +{} more", extra)
                    } else {
                        String::new()
                    };
                    println!(
                        "\nUpstream: {} caller(s) ({}{})",
                        entry.neighborhood.upstream_total,
                        names.join(", "),
                        extra_part
                    );
                }
                if entry.neighborhood.downstream_total > 0 {
                    let names: Vec<&str> = entry
                        .neighborhood
                        .downstream
                        .iter()
                        .map(|r| r.name.as_str())
                        .take(3)
                        .collect();
                    let extra = entry
                        .neighborhood
                        .downstream_total
                        .saturating_sub(names.len());
                    let extra_part = if extra > 0 {
                        format!(", +{} more", extra)
                    } else {
                        String::new()
                    };
                    let leading = if entry.neighborhood.upstream_total > 0 { "" } else { "\n" };
                    println!(
                        "{}Downstream: {} callee(s) ({}{})",
                        leading,
                        entry.neighborhood.downstream_total,
                        names.join(", "),
                        extra_part
                    );
                }
                if !entry.neighborhood.unresolved_outgoing_calls.is_empty() {
                    let total = entry.neighborhood.unresolved_outgoing_calls.len();
                    let names: Vec<&str> = entry
                        .neighborhood
                        .unresolved_outgoing_calls
                        .iter()
                        .map(|s| s.as_str())
                        .take(3)
                        .collect();
                    let extra = total.saturating_sub(names.len());
                    let extra_part = if extra > 0 {
                        format!(", +{} more", extra)
                    } else {
                        String::new()
                    };
                    println!(
                        "Unresolved outgoing: {} ({}{})",
                        total,
                        names.join(", "),
                        extra_part
                    );
                }
            } else {
                println!(
                    "\nNeighborhood: upstream {}{}, downstream {}{} (depth {})",
                    entry.neighborhood.upstream_total,
                    if entry.neighborhood.upstream_truncated {
                        format!(", showing {}", entry.neighborhood.upstream.len())
                    } else {
                        String::new()
                    },
                    entry.neighborhood.downstream_total,
                    if entry.neighborhood.downstream_truncated {
                        format!(", showing {}", entry.neighborhood.downstream.len())
                    } else {
                        String::new()
                    },
                    entry.neighborhood.call_depth
                );

                if !entry.neighborhood.upstream.is_empty() {
                    println!("Upstream callers:");
                    for related_function in &entry.neighborhood.upstream {
                        println!(
                            "  d{} {}:{}-{} {}",
                            related_function.distance,
                            related_function.file_path,
                            related_function.start_line,
                            related_function.end_line,
                            related_function.signature
                        );
                    }
                }

                if !entry.neighborhood.downstream.is_empty() {
                    println!("Downstream callees:");
                    for related_function in &entry.neighborhood.downstream {
                        println!(
                            "  d{} {}:{}-{} {}",
                            related_function.distance,
                            related_function.file_path,
                            related_function.start_line,
                            related_function.end_line,
                            related_function.signature
                        );
                    }
                }

                if !entry.neighborhood.unresolved_outgoing_calls.is_empty() {
                    println!("Unresolved outgoing calls:");
                    for callee in &entry.neighborhood.unresolved_outgoing_calls {
                        println!("  {}", callee);
                    }
                }
            }
        }

        if !entry.lifecycles.is_empty() {
            println!("\nType lifecycles:");
            for lifecycle in &entry.lifecycles {
                println!(
                    "  {}: {} funcs (entry {}, transit {}, end {}, isolated {})",
                    lifecycle.type_name,
                    lifecycle.functions_total,
                    lifecycle.entrypoints_total,
                    lifecycle.transit_total,
                    lifecycle.endpoints_total,
                    lifecycle.isolated_total
                );

                if !lifecycle.sample_paths.is_empty() {
                    let path_summary = if lifecycle.paths_truncated {
                        format!(
                            "sample paths (showing {}, truncated):",
                            lifecycle.sample_paths.len()
                        )
                    } else {
                        format!("sample paths ({}):", lifecycle.sample_paths.len())
                    };
                    println!("    {}", path_summary);

                    for path in &lifecycle.sample_paths {
                        let labels = path
                            .hops
                            .iter()
                            .map(|hop| format!("{}@{}:{}", hop.name, hop.file_path, hop.start_line))
                            .collect::<Vec<_>>();
                        println!("      {}", labels.join(" -> "));
                    }
                }
            }
        }

        if !entry.dataflow_hints.is_empty() {
            println!("\nValue flow hints:");
            for hint in &entry.dataflow_hints {
                println!(
                    "  {}:{} {} -> {} via {}",
                    hint.file_path,
                    hint.consumer_line,
                    hint.variable,
                    hint.consumer_call,
                    hint.producer_call
                );
            }
        }

        if !entry.io_boundaries.is_empty() {
            println!("\nI/O boundaries:");
            for signal in &entry.io_boundaries {
                println!(
                    "  {} {} {}:{} ({:.2}) [{}]",
                    signal.boundary,
                    signal.role,
                    signal.file_path,
                    signal.start_line,
                    signal.confidence,
                    signal.evidence.join(", ")
                );
            }
        }

        println!();
    }
}
