/// Boolean gate for each section of an ensemble response.
///
/// Set to `true` to include the corresponding section in the output.
#[derive(Debug, Clone, Copy)]
pub struct EnsembleSectionFlags {
    /// Include the raw source code snippet.
    pub code: bool,
    /// Include structs referenced by the function.
    pub structs: bool,
    /// Include annotated call-site listings.
    pub call_sites: bool,
    /// Include upstream/downstream neighbor functions.
    pub neighborhood: bool,
    /// Include type-lifecycle paths.
    pub lifecycles: bool,
    /// Include value-flow dataflow hints.
    pub dataflow: bool,
    /// Include I/O boundary signals.
    pub boundaries: bool,
}

/// Normalize a section request list into a canonical name list and a
/// [`EnsembleSectionFlags`] bitset.
///
/// `None`, an empty slice, or a slice containing `"all"` enables every
/// section.  Names are lowercased and deduplicated before comparison.
pub fn parse_ensemble_sections(
    requested: Option<&[String]>,
) -> (Vec<String>, EnsembleSectionFlags) {
    let defaults = vec![
        "code".to_string(),
        "structs".to_string(),
        "call-sites".to_string(),
        "neighborhood".to_string(),
        "lifecycle".to_string(),
        "dataflow".to_string(),
        "boundaries".to_string(),
    ];

    let mut normalized = requested
        .map(|v| {
            v.iter()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if normalized.is_empty() || normalized.iter().any(|s| s == "all") {
        return (
            defaults.clone(),
            EnsembleSectionFlags {
                code: true,
                structs: true,
                call_sites: true,
                neighborhood: true,
                lifecycles: true,
                dataflow: true,
                boundaries: true,
            },
        );
    }

    normalized.sort();
    normalized.dedup();

    let code = normalized.iter().any(|s| s == "code");
    let structs = normalized.iter().any(|s| s == "structs");
    let call_sites = normalized.iter().any(|s| s == "call-sites");
    let neighborhood = normalized.iter().any(|s| s == "neighborhood");
    let lifecycles = normalized.iter().any(|s| s == "lifecycle");
    let dataflow = normalized.iter().any(|s| s == "dataflow");
    let boundaries = normalized.iter().any(|s| s == "boundaries");
    (
        normalized,
        EnsembleSectionFlags {
            code,
            structs,
            call_sites,
            neighborhood,
            lifecycles,
            dataflow,
            boundaries,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|i| i.to_string()).collect()
    }

    #[test]
    fn defaults_to_all_sections_for_none() {
        let (sections, flags) = parse_ensemble_sections(None);
        assert_eq!(sections.len(), 7);
        assert!(flags.code);
        assert!(flags.structs);
        assert!(flags.call_sites);
        assert!(flags.neighborhood);
        assert!(flags.lifecycles);
        assert!(flags.dataflow);
        assert!(flags.boundaries);
    }

    #[test]
    fn empty_request_treated_as_all() {
        let (_, flags) = parse_ensemble_sections(Some(&[]));
        assert!(flags.code && flags.structs && flags.boundaries);
    }

    #[test]
    fn explicit_all_short_circuits() {
        let req = s(&["all"]);
        let (sections, flags) = parse_ensemble_sections(Some(&req));
        assert_eq!(sections.len(), 7);
        assert!(flags.code && flags.structs && flags.lifecycles);
    }

    #[test]
    fn parses_specific_sections() {
        let req = s(&["code", "neighborhood"]);
        let (_, flags) = parse_ensemble_sections(Some(&req));
        assert!(flags.code);
        assert!(flags.neighborhood);
        assert!(!flags.structs);
        assert!(!flags.lifecycles);
        assert!(!flags.dataflow);
        assert!(!flags.boundaries);
        assert!(!flags.call_sites);
    }

    #[test]
    fn normalizes_case_and_dedups() {
        let req = s(&["CODE", "code", "  Neighborhood  "]);
        let (sections, flags) = parse_ensemble_sections(Some(&req));
        assert_eq!(sections.len(), 2);
        assert!(flags.code);
        assert!(flags.neighborhood);
    }

    #[test]
    fn ignores_empty_strings() {
        let req = s(&["", "code"]);
        let (_, flags) = parse_ensemble_sections(Some(&req));
        assert!(flags.code);
    }

    #[test]
    fn dataflow_and_lifecycle_flags_recognized() {
        let req = s(&["dataflow", "lifecycle", "boundaries", "call-sites"]);
        let (_, flags) = parse_ensemble_sections(Some(&req));
        assert!(flags.dataflow);
        assert!(flags.lifecycles);
        assert!(flags.boundaries);
        assert!(flags.call_sites);
    }
}
