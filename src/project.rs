use crate::{
    CallSite, EnumInfo, FunctionInfo, ProjectAnalysis, StructInfo, find_rust_files,
    parse_rust_file_with_ast, search_items_with_options,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, info, warn};

/// All parsed symbols and call-graph data for a single crate root.
///
/// Produced by [`ProjectData::load`]; consumed by the analysis backends in
/// `app` (call-graph, ensemble, dead-code, etc.) and by
/// [`ProjectData::to_analysis`] for conversion to [`crate::ProjectAnalysis`].
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProjectData {
    /// Paths to every `.rs` file discovered under the crate root.
    pub rust_files: Vec<PathBuf>,
    /// All functions parsed from `rust_files`.
    pub functions: Vec<FunctionInfo>,
    /// All structs parsed from `rust_files`.
    pub structs: Vec<StructInfo>,
    /// All enums parsed from `rust_files`.
    pub enums: Vec<EnumInfo>,
    /// Caller-id → list-of-callee-base-names adjacency map built from AST
    /// call expressions.
    pub call_map: HashMap<String, Vec<String>>,
    /// Detailed per-call-site records including kind and location.
    pub call_sites: Vec<CallSite>,

    /// Type alias edges `(alias_name, canonical_name)` for call resolution.
    #[serde(default)]
    pub aliases: Vec<(String, String)>,

    /// Re-export edges `(module_path, original_name, visible_name)` used by
    /// dead-code analysis to avoid false positives on re-exported symbols.
    #[serde(default)]
    pub reexports: Vec<(String, String, String)>,
    /// Human-readable parse error messages for files that could not be parsed.
    pub parse_errors: Vec<String>,

    /// Cache of parsed `syn::File` ASTs keyed by the file path string used
    /// elsewhere in the index (the `to_string_lossy()` form of each
    /// `rust_files` entry, matching `FunctionInfo::file_path` / `CallSite::file_path`).
    ///
    /// Populated eagerly by [`ProjectData::load`] so downstream consumers
    /// (refs, impls, def, members, usages, dead-code, ensemble) can reuse a
    /// single parse instead of calling `syn::parse_file` 5–12 more times per
    /// source file.  Skipped during serialization because `syn::File` is not
    /// `Serialize`/`Deserialize`; deserialized `ProjectData` instances simply
    /// have an empty cache and consumers must fall back to re-parsing.
    #[serde(skip)]
    parsed_files: HashMap<String, syn::File>,
}

impl ProjectData {
    /// Walk `path` recursively, parse every `.rs` file, and assemble the
    /// full call-graph with alias and framework-handler edges resolved.
    ///
    /// Files that fail to parse are recorded in `parse_errors`; the load
    /// continues for all remaining files.
    pub fn load(path: &Path, include_ignored: bool) -> Self {
        let started = Instant::now();
        let rust_files = find_rust_files(path, include_ignored);
        let mut functions = Vec::new();
        let mut structs = Vec::new();
        let mut enums = Vec::new();
        let mut call_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut call_sites = Vec::new();
        let mut aliases: Vec<(String, String)> = Vec::new();
        let mut reexports: Vec<(String, String, String)> = Vec::new();
        let mut parse_errors = Vec::new();
        let mut parsed_files: HashMap<String, syn::File> = HashMap::new();

        for file_path in &rust_files {
            match parse_rust_file_with_ast(file_path) {
                Ok((
                    (
                        mut file_functions,
                        mut file_structs,
                        mut file_enums,
                        file_call_map,
                        mut file_call_sites,
                        mut file_aliases,
                        mut file_reexports,
                    ),
                    syntax_tree,
                )) => {
                    functions.append(&mut file_functions);
                    structs.append(&mut file_structs);
                    enums.append(&mut file_enums);
                    merge_call_map(&mut call_map, file_call_map);
                    call_sites.append(&mut file_call_sites);
                    aliases.append(&mut file_aliases);
                    reexports.append(&mut file_reexports);
                    parsed_files.insert(file_path.to_string_lossy().to_string(), syntax_tree);
                }
                Err(error) => {
                    warn!(
                        target: "project.load",
                        file = %file_path.display(),
                        %error,
                        "failed to parse Rust source file"
                    );
                    parse_errors.push(format!("Error parsing {}: {}", file_path.display(), error));
                }
            }
        }


        {
            let mut file_cache: HashMap<String, Vec<String>> = HashMap::new();
            let framework_refs = crate::index::collect_framework_handler_refs(&call_sites, &mut file_cache);
            for fr in &framework_refs {
                if let Some(cid) = &fr.caller_id {
                    call_map
                        .entry(cid.clone())
                        .or_default()
                        .push(fr.callee.clone());
                }
            }
        }


        let mut alias_map: HashMap<String, Vec<String>> = HashMap::new();
        for (alias, target) in &aliases {
            alias_map.entry(alias.clone()).or_default().push(target.clone());
        }
        let mut extra_sites: Vec<CallSite> = Vec::new();
        for site in &call_sites {
            if let Some(targets) = alias_map.get(&site.callee_base) {
                for tgt in targets {
                    let mut copy = site.clone();
                    copy.callee_base = tgt.clone();

                    copy.call_kind = format!("{}_via_alias", site.call_kind);
                    extra_sites.push(copy);
                }
            }
        }
        call_sites.extend(extra_sites);
        let mut extra_map_entries: Vec<(String, Vec<String>)> = Vec::new();
        for (caller_id, callees) in &call_map {
            let mut new_callees: Vec<String> = Vec::new();
            for callee in callees {
                let bare = callee
                    .rsplit(|c: char| c == '.' || c == ':')
                    .next()
                    .unwrap_or(callee.as_str());
                if let Some(targets) = alias_map.get(bare) {
                    for tgt in targets {
                        new_callees.push(tgt.clone());
                    }
                }
            }
            if !new_callees.is_empty() {
                extra_map_entries.push((caller_id.clone(), new_callees));
            }
        }
        for (caller_id, mut new_callees) in extra_map_entries {
            call_map
                .entry(caller_id)
                .or_default()
                .append(&mut new_callees);
        }

        let project = Self {
            rust_files,
            functions,
            structs,
            enums,
            call_map,
            call_sites,
            aliases,
            reexports,
            parse_errors,
            parsed_files,
        };
        info!(
            target: "project.load",
            path = %path.display(),
            include_ignored,
            rust_files = project.rust_files.len(),
            functions = project.functions.len(),
            structs = project.structs.len(),
            enums = project.enums.len(),
            parse_errors = project.parse_errors.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "loaded project data"
        );
        project
    }

    /// Filter `functions`, `structs`, and `enums` in-place to those fuzzy-
    /// matching `search_terms` at the given `threshold`.
    ///
    /// Delegates to [`apply_search_filter_with_options`] with
    /// `match_signature = false`.
    pub fn apply_search_filter(&mut self, search_terms: &str, threshold: f64) {
        self.apply_search_filter_with_options(search_terms, threshold, false);
    }

    /// Filter symbols in-place using the fuzzy search engine.
    ///
    /// When `match_signature` is `true`, hits whose signature or file path
    /// fuzzy-matches the query are also included (additive union with
    /// name-tier hits). A hint is printed to stderr when the result set is
    /// large relative to the threshold.
    pub fn apply_search_filter_with_options(
        &mut self,
        search_terms: &str,
        threshold: f64,
        match_signature: bool,
    ) {
        let started = Instant::now();
        let before_functions = self.functions.len();
        let before_structs = self.structs.len();
        let before_enums = self.enums.len();
        let (functions, structs, enums) = search_items_with_options(
            &self.functions,
            &self.structs,
            &self.enums,
            search_terms,
            threshold,
            match_signature,
        );
        self.functions = functions;
        self.structs = structs;
        self.enums = enums;
        let total_matches = self.functions.len() + self.structs.len() + self.enums.len();
        if total_matches > 10 && threshold < 0.8 {
            eprintln!(
                "hint: --search '{}' returned {} matches at threshold {:.2}; for precise lookups try --search-threshold 0.9",
                search_terms, total_matches, threshold
            );
        }
        debug!(
            target: "project.load",
            search_terms,
            threshold,
            before_functions,
            before_structs,
            before_enums,
            after_functions = self.functions.len(),
            after_structs = self.structs.len(),
            after_enums = self.enums.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "applied project search filter"
        );
    }

    /// Look up the cached `syn::File` AST for a Rust source file.
    ///
    /// The cache is populated eagerly during [`ProjectData::load`] for every
    /// file in `rust_files` that parsed successfully.  Returns `None` for
    /// files that failed to parse, files outside the loaded project, or
    /// when the cache was not populated (e.g. on a default-constructed
    /// `ProjectData` that was deserialized from JSON).
    pub fn parsed_file(&self, path: &Path) -> Option<&syn::File> {
        self.parsed_file_by_str(&path.to_string_lossy())
    }

    /// Same as [`ProjectData::parsed_file`] but accepts a string key directly,
    /// useful when the caller already has a `file_path` string from a
    /// `FunctionInfo` or `CallSite` and would rather not allocate a `PathBuf`.
    pub fn parsed_file_by_str(&self, path: &str) -> Option<&syn::File> {
        self.parsed_files.get(path)
    }

    /// Move the parsed-file cache out of `self`.
    ///
    /// Used by the cross-crate `--also` merge logic in
    /// [`crate::app::run`] to splice another `ProjectData`'s cache into the
    /// primary project without re-parsing.
    pub fn take_parsed_files(&mut self) -> HashMap<String, syn::File> {
        std::mem::take(&mut self.parsed_files)
    }

    /// Splice an external `parsed_files` map into `self`'s cache.
    ///
    /// On key collision the existing entry wins because it represents the
    /// primary crate that was loaded first, and downstream consumers index
    /// other state (functions, structs, call_sites) against those primary
    /// paths.
    pub fn absorb_parsed_files(&mut self, extra: HashMap<String, syn::File>) {
        for (k, v) in extra {
            self.parsed_files.entry(k).or_insert(v);
        }
    }

    /// Snapshot of every key currently in the parsed-file cache.
    ///
    /// Used by [`crate::app::run`] to register relativized aliases on top of
    /// the original absolute-path keys after `--also` merges.
    pub fn parsed_files_keys(&self) -> Vec<String> {
        self.parsed_files.keys().cloned().collect()
    }

    /// Insert a second key `alias` in the parsed-file cache that points at the
    /// same AST currently stored under `existing`.
    ///
    /// Used after path relativization so consumers can look the cached AST up
    /// using either the original PathBuf-derived form (matches `rust_files`)
    /// or the relativized string form (matches `FunctionInfo::file_path` /
    /// `CallSite::file_path`).
    pub fn alias_parsed_file(&mut self, existing: &str, alias: &str) {
        if existing == alias || self.parsed_files.contains_key(alias) {
            return;
        }
        if let Some(syntax) = self.parsed_files.get(existing).cloned() {
            self.parsed_files.insert(alias.to_string(), syntax);
        }
    }

    /// Convert to a [`crate::ProjectAnalysis`] by cloning the symbol tables.
    ///
    /// `ProjectAnalysis` is the type consumed by the lower-level query and
    /// rendering backends; this method is the bridge from the high-level
    /// loader.
    pub fn to_analysis(&self) -> ProjectAnalysis {
        ProjectAnalysis {
            functions: self.functions.clone(),
            structs: self.structs.clone(),
            enums: self.enums.clone(),
            call_map: self.call_map.clone(),
            call_sites: self.call_sites.clone(),
            files_analyzed: self.rust_files.len(),
            parse_errors: self.parse_errors.clone(),
            reexports: self.reexports.clone(),
        }
    }
}

/// Merge a per-file `call_map` into the project-wide `call_map`, concatenating
/// the callee lists for any caller-id key that already exists.
///
/// Caller IDs are file-scoped (`file:line:name`) so collisions are rare
/// inside a single crate, but when two crates are merged via `--also` and
/// path normalization brings them into the same key space, two unrelated
/// callers can share the same `file:line:name` triple and previously
/// `HashMap::extend` would silently drop the earlier callees.  Concatenating
/// preserves every edge — call-graph queries dedupe downstream.
pub(crate) fn merge_call_map(
    target: &mut HashMap<String, Vec<String>>,
    incoming: HashMap<String, Vec<String>>,
) {
    for (caller_id, mut callees) in incoming {
        target.entry(caller_id).or_default().append(&mut callees);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn project_data_load_collects_functions_structs_enums() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("lib.rs"),
            "pub struct Foo;\npub enum Bar { X }\npub fn baz() {}\n",
        )
        .unwrap();

        let project = ProjectData::load(dir.path(), false);
        assert_eq!(project.rust_files.len(), 1);
        assert_eq!(project.functions.len(), 1);
        assert_eq!(project.functions[0].name, "baz");
        assert_eq!(project.structs.len(), 1);
        assert_eq!(project.structs[0].name, "Foo");
        assert_eq!(project.enums.len(), 1);
        assert_eq!(project.enums[0].name, "Bar");
        assert!(project.parse_errors.is_empty());
    }

    #[test]
    fn project_data_load_records_parse_errors_for_invalid_rust() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("ok.rs"), "pub fn ok() {}\n").unwrap();
        fs::write(dir.path().join("broken.rs"), "this is not valid").unwrap();

        let project = ProjectData::load(dir.path(), false);
        assert_eq!(project.functions.len(), 1);
        assert_eq!(project.parse_errors.len(), 1);
        assert!(project.parse_errors[0].contains("broken.rs"));
    }

    #[test]
    fn project_data_apply_search_filter_narrows_collections() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("lib.rs"),
            "pub fn alpha() {}\npub fn beta() {}\npub fn gamma() {}\n",
        )
        .unwrap();

        let mut project = ProjectData::load(dir.path(), false);
        assert_eq!(project.functions.len(), 3);
        project.apply_search_filter("alpha", 0.95);
        assert_eq!(project.functions.len(), 1);
        assert_eq!(project.functions[0].name, "alpha");
    }

    #[test]
    fn project_data_to_analysis_clones_state() {
        let dir = tempdir().expect("tempdir");
        fs::write(dir.path().join("lib.rs"), "pub fn one() {}\n").unwrap();
        let project = ProjectData::load(dir.path(), false);
        let analysis = project.to_analysis();
        assert_eq!(analysis.files_analyzed, 1);
        assert_eq!(analysis.functions.len(), 1);
    }

    #[test]
    fn project_data_default_is_empty_and_serializable() {
        let project = ProjectData::default();
        assert!(project.functions.is_empty());
        assert!(project.structs.is_empty());
        assert!(project.enums.is_empty());
        assert!(project.call_map.is_empty());
        assert!(project.call_sites.is_empty());
        assert!(project.rust_files.is_empty());
        let json = serde_json::to_string(&project).expect("serialize");
        let _: ProjectData = serde_json::from_str(&json).expect("deserialize");
    }

    #[test]
    fn project_data_parsed_file_returns_cached_ast_for_loaded_files() {
        use quote::ToTokens;
        let dir = tempdir().expect("tempdir");
        let lib = dir.path().join("lib.rs");
        fs::write(&lib, "pub fn one() {}\npub struct S;\n").unwrap();

        let project = ProjectData::load(dir.path(), false);

        // Cache should be populated and the parsed AST retrievable by both
        // PathBuf and string-key forms.
        let by_path = project.parsed_file(&lib).expect("cached by Path");
        let by_str = project
            .parsed_file_by_str(&lib.to_string_lossy())
            .expect("cached by str");

        // Identity: repeated lookups must return the SAME borrow (same
        // address) — proves the AST is stored in place rather than re-parsed.
        let a = project.parsed_file(&lib).expect("first lookup");
        let b = project.parsed_file(&lib).expect("second lookup");
        assert!(std::ptr::eq(a, b), "parsed_file must return cached reference");
        assert!(std::ptr::eq(by_path, by_str));

        // Sanity-check the cached AST round-trips back to the original tokens.
        let dumped = a.to_token_stream().to_string();
        assert!(dumped.contains("one"));
        assert!(dumped.contains("struct S"));

        // Files outside the loaded project return None.
        assert!(project.parsed_file(std::path::Path::new("/no/such.rs")).is_none());
    }

    #[test]
    fn merge_call_map_concatenates_collisions() {
        let mut target: HashMap<String, Vec<String>> = HashMap::new();
        target.insert("k".into(), vec!["a".into(), "b".into()]);

        let mut incoming: HashMap<String, Vec<String>> = HashMap::new();
        incoming.insert("k".into(), vec!["c".into()]);
        incoming.insert("other".into(), vec!["d".into()]);

        merge_call_map(&mut target, incoming);

        let merged = target.get("k").expect("collision key present");
        assert_eq!(merged.len(), 3, "callees from both sides must survive");
        assert!(merged.contains(&"a".to_string()));
        assert!(merged.contains(&"b".to_string()));
        assert!(merged.contains(&"c".to_string()));
        assert_eq!(target.get("other").map(|v| v.as_slice()), Some(&["d".to_string()][..]));
    }
}
