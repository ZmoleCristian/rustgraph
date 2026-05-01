//! Plain-text renderer for [`CallersReport`] output.

use crate::app::callers::CallersReport;
use crate::format_cfg_annotation;
use std::fmt::Write;

/// Converts a [`CallersReport`] into a multi-line plain-text string.
///
/// Each match block shows the target's file/line range and signature, a callers summary
/// line, then per-caller entries with their call-site line numbers and optional context
/// windows. Returns an empty string when `report.matches` is empty.
pub fn render_callers_text(report: &CallersReport) -> String {
    let mut out = String::new();
    for entry in &report.matches {


        let target_cfg = format_cfg_annotation(&entry.info.cfg_attrs)
            .map(|s| format!(" {}", s))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "{}:{}-{} - {}{}",
            entry.info.file_path,
            entry.info.start_line,
            entry.info.end_line,
            entry.info.signature,
            target_cfg
        );
        let _ = writeln!(
            out,
            "Callers: {} function(s), {} total call site(s){}",
            entry.callers.len(),
            entry.call_sites_total,
            if entry.unresolved_call_sites > 0 {
                format!(", {} unresolved", entry.unresolved_call_sites)
            } else {
                String::new()
            }
        );

        for caller in &entry.callers {
            let caller_cfg = format_cfg_annotation(&caller.info.cfg_attrs)
                .map(|s| format!(" {}", s))
                .unwrap_or_default();
            let _ = writeln!(
                out,
                "  {}:{}-{} - {}{}",
                caller.info.file_path,
                caller.info.start_line,
                caller.info.end_line,
                caller.info.signature,
                caller_cfg
            );
            let line_list = caller
                .call_sites
                .iter()
                .map(|call| call.line.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "    lines: {}", line_list);
            for call in &caller.call_sites {
                let _ = writeln!(
                    out,
                    "      {}:{} - {}",
                    call.line,
                    call.column,
                    call.line_text.trim()
                );
                if !call.context_lines.is_empty() {
                    for context in &call.context_lines {
                        let marker = if context.is_match { ">" } else { " " };
                        let _ = writeln!(
                            out,
                            "      {} {:>6} | {}",
                            marker,
                            context.line,
                            context.text.trim_end()
                        );
                    }
                }
            }
        }
        if !entry.unresolved_call_site_locations.is_empty() {
            let _ = writeln!(out, "  Unresolved call sites:");
            for site in &entry.unresolved_call_site_locations {
                let caller_hint = match (&site.caller_name, site.caller_kind) {
                    (Some(name), _) => format!(" [in {} — {}]", name, site.caller_kind),
                    (None, kind) => format!(" [{}]", kind),
                };
                let _ = writeln!(
                    out,
                    "    {}:{}:{} - {}{}",
                    site.file_path,
                    site.line,
                    site.column,
                    site.line_text.trim(),
                    caller_hint
                );
            }
        }
        out.push('\n');
    }
    out
}


/// Prints the [`CallersReport`] in plain-text format to stdout.
pub fn print_callers_text(report: &CallersReport) {
    print!("{}", render_callers_text(report));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FunctionInfo;
    use crate::app::callers::{CallersMatch, UnresolvedCallSite};

    fn fn_info(name: &str, file_path: &str, start_line: usize) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {}()", name),
            file_path: file_path.to_string(),
            start_line,
            end_line: start_line + 1,
            is_async: false,
            is_unsafe: false,
            is_pub: false,
            is_const: false,
            is_test: false,
            generics: Vec::new(),
            parameters: Vec::new(),
            return_type: None,
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    #[test]
    fn render_lists_unresolved_call_sites_with_locations() {
        let report = CallersReport {
            query: "target".to_string(),
            matches: vec![CallersMatch {
                info: fn_info("target", "src/lib.rs", 10),
                call_sites_total: 2,
                unresolved_call_sites: 2,
                unresolved_call_site_locations: vec![
                    UnresolvedCallSite {
                        file_path: "src/a.rs".to_string(),
                        line: 30,
                        column: 4,
                        line_text: "    target();".to_string(),
                        caller_name: Some("alpha".to_string()),
                        caller_kind: "unmapped",
                    },
                    UnresolvedCallSite {
                        file_path: "src/b.rs".to_string(),
                        line: 7,
                        column: 0,
                        line_text: "static GREETER: fn() = target;".to_string(),
                        caller_name: None,
                        caller_kind: "module-level",
                    },
                ],
                callers: Vec::new(),
            }],
        };

        let out = render_callers_text(&report);
        assert!(
            out.contains("Unresolved call sites:"),
            "missing header in output:\n{}",
            out
        );
        assert!(
            out.contains("src/a.rs:30:4 - target(); [in alpha — unmapped]"),
            "missing unmapped site (line text is trimmed):\n{}",
            out
        );
        assert!(
            out.contains("src/b.rs:7:0 - static GREETER: fn() = target; [module-level]"),
            "missing module-level site:\n{}",
            out
        );
    }

    #[test]
    fn render_omits_unresolved_block_when_empty() {
        let report = CallersReport {
            query: "target".to_string(),
            matches: vec![CallersMatch {
                info: fn_info("target", "src/lib.rs", 10),
                call_sites_total: 0,
                unresolved_call_sites: 0,
                unresolved_call_site_locations: Vec::new(),
                callers: Vec::new(),
            }],
        };

        let out = render_callers_text(&report);
        assert!(!out.contains("Unresolved call sites:"));
    }
}
