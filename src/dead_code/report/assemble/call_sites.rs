use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::project::ProjectData;
use crate::{CallSite, resolve_ambiguous_call_target_for_functions};

use super::super::super::model::{
    collect_cfg_test_ranges, collect_cfg_test_ranges_from_ast, is_test_path, line_in_ranges,
};
use super::super::super::rust_analyzer::collect_rust_analyzer_reference_counts;
use super::super::super::text_refs::{
    collect_textual_function_argument_refs, collect_textual_function_refs,
    extract_framework_handler_refs, extract_functional_arg_refs, extract_macro_function_refs,
    extract_macro_identifier_refs,
};
use super::super::entrypoint::{
    collect_local_module_roots, is_likely_associated_function_callee,
    is_likely_external_qualified_call,
};
use super::candidates::CandidateContext;

pub struct CallSiteRefCounts {
    pub unique_non_test_calls: HashMap<String, usize>,
    pub unique_test_calls: HashMap<String, usize>,
    pub duplicate_non_test_calls: HashMap<(String, usize), usize>,
    pub duplicate_test_calls: HashMap<(String, usize), usize>,
    pub unresolved_duplicate_non_test: HashMap<String, usize>,
    pub ra_non_test_calls: HashMap<(String, usize), usize>,
    pub ra_test_calls: HashMap<(String, usize), usize>,
    pub framework_live_targets: HashSet<(String, usize)>,
}

pub fn collect_reference_counts(
    ctx: &CandidateContext,
    call_sites: &[CallSite],
    rust_files: &[PathBuf],
    workspace_root: &Path,
    test_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
    macro_rules_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
    file_lines_cache: &mut HashMap<String, Vec<String>>,
    project: Option<&ProjectData>,
) -> Result<CallSiteRefCounts, Box<dyn std::error::Error>> {
    let candidate_names: HashSet<String> = ctx.candidate_name_counts.keys().cloned().collect();
    let framework_refs = extract_framework_handler_refs(call_sites, file_lines_cache);
    let functional_refs = extract_functional_arg_refs(call_sites, file_lines_cache);
    let macro_refs = extract_macro_function_refs(call_sites, file_lines_cache, &candidate_names);
    let macro_identifier_refs =
        extract_macro_identifier_refs(call_sites, file_lines_cache, &candidate_names);

    let mut effective_call_sites = Vec::with_capacity(
        call_sites.len()
            + framework_refs.len()
            + functional_refs.len()
            + macro_refs.len()
            + macro_identifier_refs.len(),
    );
    effective_call_sites.extend_from_slice(call_sites);
    effective_call_sites.extend(framework_refs.iter().cloned());
    effective_call_sites.extend(functional_refs.iter().cloned());
    effective_call_sites.extend(macro_refs.iter().cloned());
    effective_call_sites.extend(macro_identifier_refs.iter().cloned());

    let mut framework_live_targets: HashSet<(String, usize)> = HashSet::new();
    for call_ref in &framework_refs {
        let Some(group) = ctx.candidates_by_name.get(&call_ref.callee_base) else {
            continue;
        };
        if let Some(target) = resolve_ambiguous_call_target_for_functions(
            call_ref,
            group,
            &ctx.function_by_id,
            &ctx.module_lookup,
        ) {
            framework_live_targets.insert(target);
        }
    }

    let mut unique_non_test_calls: HashMap<String, usize> = HashMap::new();
    let mut unique_test_calls: HashMap<String, usize> = HashMap::new();
    let mut duplicate_non_test_calls: HashMap<(String, usize), usize> = HashMap::new();
    let mut duplicate_test_calls: HashMap<(String, usize), usize> = HashMap::new();
    let mut unresolved_duplicate_non_test: HashMap<String, usize> = HashMap::new();

    let (ra_non_test_calls, ra_test_calls) = collect_rust_analyzer_reference_counts(
        &ctx.candidates,
        workspace_root,
        test_ranges_cache,
        file_lines_cache,
    )?;

    let local_module_roots = collect_local_module_roots(rust_files);
    let duplicate_names: HashSet<String> = ctx
        .candidate_name_counts
        .iter()
        .filter_map(|(name, count)| if *count > 1 { Some(name.clone()) } else { None })
        .collect();

    for cs in effective_call_sites
        .iter()
        .filter(|cs| cs.call_kind == "function")
        .filter(|cs| ctx.candidate_name_counts.contains_key(&cs.callee_base))
    {
        let test_call = if is_test_path(&cs.file_path) {
            true
        } else {
            let ranges = test_ranges_cache
                .entry(cs.file_path.clone())
                .or_insert_with(|| {
                    match project.and_then(|p| p.parsed_file_by_str(&cs.file_path)) {
                        Some(syntax) => collect_cfg_test_ranges_from_ast(syntax),
                        None => collect_cfg_test_ranges(&cs.file_path),
                    }
                });
            line_in_ranges(cs.line, ranges)
        };

        if duplicate_names.contains(&cs.callee_base) {
            let Some(group) = ctx.candidates_by_name.get(&cs.callee_base) else {
                continue;
            };
            if let Some(key) = resolve_ambiguous_call_target_for_functions(
                cs,
                group,
                &ctx.function_by_id,
                &ctx.module_lookup,
            ) {
                if test_call {
                    *duplicate_test_calls.entry(key).or_default() += 1;
                } else {
                    *duplicate_non_test_calls.entry(key).or_default() += 1;
                }
            } else if !test_call {
                if is_likely_associated_function_callee(&cs.callee) {
                    continue;
                }
                if is_likely_external_qualified_call(cs, &local_module_roots) {
                    continue;
                }
                *unresolved_duplicate_non_test
                    .entry(cs.callee_base.clone())
                    .or_default() += 1;
            }
        } else if test_call {
            *unique_test_calls.entry(cs.callee_base.clone()).or_default() += 1;
        } else {
            *unique_non_test_calls
                .entry(cs.callee_base.clone())
                .or_default() += 1;
        }
    }

    let (text_non_test_calls, text_test_calls) = collect_textual_function_refs(
        rust_files,
        &ctx.candidate_name_counts,
        test_ranges_cache,
        macro_rules_ranges_cache,
        file_lines_cache,
    );
    for (name, count) in text_non_test_calls {
        *unique_non_test_calls.entry(name).or_default() += count;
    }
    for (name, count) in text_test_calls {
        *unique_test_calls.entry(name).or_default() += count;
    }

    let (text_arg_non_test_calls, text_arg_test_calls) = collect_textual_function_argument_refs(
        rust_files,
        &ctx.candidate_name_counts,
        test_ranges_cache,
        macro_rules_ranges_cache,
        file_lines_cache,
    );
    for (name, count) in text_arg_non_test_calls {
        *unique_non_test_calls.entry(name).or_default() += count;
    }
    for (name, count) in text_arg_test_calls {
        *unique_test_calls.entry(name).or_default() += count;
    }

    Ok(CallSiteRefCounts {
        unique_non_test_calls,
        unique_test_calls,
        duplicate_non_test_calls,
        duplicate_test_calls,
        unresolved_duplicate_non_test,
        ra_non_test_calls,
        ra_test_calls,
        framework_live_targets,
    })
}
