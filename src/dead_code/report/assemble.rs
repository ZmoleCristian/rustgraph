//! Orchestrates the full dead-code analysis pipeline.
//!
//! Collects candidate `pub fn` items, counts call sites from AST and text-based
//! detectors, applies skip filters, and assembles the final `DeadCodeReport`.

pub mod call_sites;
pub mod candidates;
pub mod filtering;
pub mod results;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::{CallSite, FunctionInfo};

use super::super::model::{DeadCodeReport, DeadFunction};
use super::super::text_refs::collect_public_reexport_names;

use self::call_sites::collect_reference_counts;
use self::candidates::build_candidate_context;
use self::filtering::{SkipReason, check_skip_reason};
use self::results::{assemble_report, build_ambiguous_details};

/// Run the full dead-code analysis and return a `DeadCodeReport`.
///
/// Accepts the project's `functions` and `call_sites` produced by the AST visitor,
/// the list of Rust source `rust_files` to scan for textual references, and the
/// `workspace_root` used to scope rust-analyzer if available.
pub fn build_dead_code_report(
    functions: &[FunctionInfo],
    call_sites: &[CallSite],
    rust_files: &[PathBuf],
    workspace_root: &Path,
) -> Result<DeadCodeReport, Box<dyn std::error::Error>> {
    let mut test_ranges_cache: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    let mut macro_rules_ranges_cache: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    let mut runtime_entrypoint_cache: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut file_lines_cache: HashMap<String, Vec<String>> = HashMap::new();

    let mut called_method_names: HashSet<String> = HashSet::new();
    for cs in call_sites.iter() {
        called_method_names.insert(cs.callee_base.clone());
    }

    let ctx = build_candidate_context(functions);
    let reexported = collect_public_reexport_names(rust_files);
    let refs = collect_reference_counts(
        &ctx,
        call_sites,
        rust_files,
        workspace_root,
        &mut test_ranges_cache,
        &mut macro_rules_ranges_cache,
        &mut file_lines_cache,
    )?;

    let mut dead = Vec::new();
    let mut skipped_reexported = 0usize;
    let mut skipped_cfg_test = 0usize;
    let mut skipped_runtime_entrypoints = 0usize;
    let mut skipped_ambiguous_name = 0usize;
    let mut skipped_framework_handler_refs = 0usize;

    let candidates_total = ctx.candidates.len();

    for func in &ctx.candidates {
        let skip_reason = check_skip_reason(
            func,
            &reexported,
            &called_method_names,
            &refs.framework_live_targets,
            &mut test_ranges_cache,
            &mut runtime_entrypoint_cache,
        );

        match skip_reason {
            SkipReason::FrameworkHandler => {
                skipped_framework_handler_refs += 1;
                continue;
            }
            SkipReason::Reexported => {
                skipped_reexported += 1;
                continue;
            }
            SkipReason::CfgTest => {
                skipped_cfg_test += 1;
                continue;
            }
            SkipReason::RuntimeEntrypoint => {
                skipped_runtime_entrypoints += 1;
                continue;
            }
            SkipReason::CommonTraitMethod => {
                continue;
            }
            SkipReason::None => {}
        }

        let is_ambiguous_name = ctx
            .candidate_name_counts
            .get(&func.name)
            .copied()
            .unwrap_or(0)
            > 1;

        let (non_test_calls, test_calls) = if is_ambiguous_name {
            let key = (func.file_path.clone(), func.start_line);
            let ra_non_test = refs.ra_non_test_calls.get(&key).copied().unwrap_or(0);
            let ra_test = refs.ra_test_calls.get(&key).copied().unwrap_or(0);

            if refs
                .unresolved_duplicate_non_test
                .get(&func.name)
                .copied()
                .unwrap_or(0)
                > 0
                && ra_non_test == 0
            {
                skipped_ambiguous_name += 1;
                continue;
            }

            let (mut non_test_calls, mut test_calls) = (
                refs.duplicate_non_test_calls
                    .get(&key)
                    .copied()
                    .unwrap_or(0),
                refs.duplicate_test_calls.get(&key).copied().unwrap_or(0),
            );
            non_test_calls += ra_non_test;
            test_calls += ra_test;
            (non_test_calls, test_calls)
        } else {
            let key = (func.file_path.clone(), func.start_line);
            let mut non_test_calls = refs
                .unique_non_test_calls
                .get(&func.name)
                .copied()
                .unwrap_or(0);
            let mut test_calls = refs.unique_test_calls.get(&func.name).copied().unwrap_or(0);
            non_test_calls += refs.ra_non_test_calls.get(&key).copied().unwrap_or(0);
            test_calls += refs.ra_test_calls.get(&key).copied().unwrap_or(0);
            (non_test_calls, test_calls)
        };

        if non_test_calls == 0 {
            dead.push(DeadFunction {
                info: (*func).clone(),
                call_sites_non_test_total: non_test_calls,
                call_sites_test_only_total: test_calls,
            });
        }
    }

    let ambiguous_name_details = build_ambiguous_details(&ctx, &refs.unresolved_duplicate_non_test);

    Ok(assemble_report(
        candidates_total,
        dead,
        skipped_reexported,
        skipped_cfg_test,
        skipped_runtime_entrypoints,
        skipped_ambiguous_name,
        skipped_framework_handler_refs,
        ambiguous_name_details,
    ))
}
