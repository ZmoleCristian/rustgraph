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
        out.push('\n');
    }
    out
}


/// Prints the [`CallersReport`] in plain-text format to stdout.
pub fn print_callers_text(report: &CallersReport) {
    print!("{}", render_callers_text(report));
}
