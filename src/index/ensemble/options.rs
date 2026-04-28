/// Tuning knobs forwarded through the entire ensemble build pipeline.
#[derive(Debug, Clone, Copy)]
pub struct FunctionEnsembleOptions<'a> {
    /// Maximum number of query matches to return.
    pub max_results: usize,
    /// Maximum call-site entries included per function match.
    pub max_call_sites: usize,
    /// BFS depth for upstream/downstream neighbor traversal.
    pub call_depth: usize,
    /// Maximum neighborhood entries returned per list (applied to upstream and downstream independently).
    pub max_related: usize,
    /// Maximum sample paths per type lifecycle.
    pub max_lifecycle_paths: usize,
    /// Maximum functions shown per lifecycle role bucket.
    pub lifecycle_max_functions: usize,
    /// Restrict lifecycle analysis to these type names; `None` means auto-detect.
    pub lifecycle_types: Option<&'a [String]>,
    /// Sections to populate; `None` or empty activates all sections.
    pub ensemble_sections: Option<&'a [String]>,

    /// When true, fall back to signature matching if no name match meets the threshold.
    pub match_signature: bool,
}
