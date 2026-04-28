use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Controls which call edges are included in a call-graph query.
///
/// `Internal` restricts edges to functions defined within the indexed crate;
/// `External` shows only cross-crate/unresolved callees; `All` combines both.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
pub enum CallGraphDetail {
    /// Only intra-crate call edges.
    Internal,
    /// Only cross-crate or unresolved callees.
    External,
    /// All call edges regardless of resolution.
    All,
}

impl CallGraphDetail {
    /// Returns the lowercase string label for the variant (`"internal"`,
    /// `"external"`, `"all"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::External => "external",
            Self::All => "all",
        }
    }
}

/// Named depth profile for the `ensemble` command.
///
/// Sets numeric limits (max results, max call-sites, neighborhood depth,
/// lifecycle path counts) for ensemble building. Section selection is
/// independent and controlled by [`EnsembleView`] / `--section`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
pub enum EnsemblePreset {
    /// Tightest limits (fewest results, shallow neighborhood).
    Quick,
    /// Default limits — moderate result counts and depth-2 neighborhood.
    Balanced,
    /// Largest limits (more results, deeper neighborhood, no call-site cap).
    Deep,
}


/// Output view for the `ensemble` command.
///
/// Selects which sections are included in the rendered output. Independent
/// of [`EnsemblePreset`], which only governs numeric limits.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
pub enum EnsembleView {
    /// Default view: structs, call-sites, neighborhood, and dataflow, with the
    /// neighborhood section rendered in abbreviated form.
    Summary,

    /// Call-sites and neighborhood only.
    Usage,

    /// Neighborhood, lifecycle, dataflow, and boundaries.
    Flow,

    /// Every section enabled (largest output).
    Full,
}

/// Lightweight description of a Claude / AI-assistant session file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    /// URL-safe slug derived from the project path.
    pub project_slug: String,
    /// Human-readable project name.
    pub project_name: String,
    /// Absolute path to the project root, if known.
    pub project_path: Option<String>,
    /// Unique session identifier (typically the JSONL filename stem).
    pub session_id: String,
    /// Absolute path to the session JSONL file.
    pub session_path: String,
    /// File size in bytes.
    pub bytes: u64,
    /// Last-modified timestamp in milliseconds since the Unix epoch.
    pub modified_unix_ms: u64,
}

/// Aggregate symbol counts for an indexed project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCounts {
    /// Number of Rust source files parsed.
    pub files: usize,
    /// Number of functions indexed.
    pub functions: usize,
    /// Number of structs indexed.
    pub structs: usize,
    /// Number of enums indexed.
    pub enums: usize,
    /// Number of resolved call-site edges.
    pub call_sites: usize,
}

/// Per-language file and line-count statistics for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectLanguageStat {
    /// Language name (e.g. `"Rust"`, `"TOML"`).
    pub language: String,
    /// Number of files of this language.
    pub files: usize,
    /// Non-blank, non-comment source lines.
    pub code_lines: usize,
}

/// Top-level metadata record for a discovered project, as surfaced by
/// rustgraph's project-listing APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    /// URL-safe identifier for the project.
    pub slug: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Absolute filesystem path to the project root, if available.
    pub path: Option<String>,
    /// Millisecond Unix timestamp of the most recent session activity.
    pub last_active_unix_ms: u64,
    /// Dominant language detected (e.g. `"Rust"`).
    pub primary_language: Option<String>,
    /// Per-language breakdown of file and line counts.
    pub languages: Vec<ProjectLanguageStat>,
    /// Whether rustgraph can index this project (Rust crate with `Cargo.toml`).
    pub supported: bool,
    /// Whether the project has been manually pinned in the UI.
    pub pinned: bool,
    /// Discovery source label (e.g. `"cargo"`, `"git"`, `"manual"`).
    pub discovered_from: String,
    /// Active AI-assistant sessions associated with this project.
    pub active_sessions: Vec<SessionSummary>,
    /// Millisecond Unix timestamp of the last successful index run, if any.
    pub indexed_at_unix_ms: Option<u64>,
    /// Symbol counts from the last index run, if available.
    pub counts: Option<ProjectCounts>,
}

/// Compact reference to a single indexed symbol, returned by search and find
/// operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRef {
    /// Canonical symbol identifier; format depends on kind:
    /// functions: `"<rel_path>:<line>:<name>"`,
    /// structs/enums: `"struct:<rel_path>:<line>:<name>"` etc.
    pub symbol_id: String,
    /// Symbol kind tag: `"function"`, `"struct"`, or `"enum"`.
    pub kind: String,
    /// Bare symbol name (unqualified).
    pub name: String,
    /// Human-readable signature (e.g. `"fn foo(x: u32) -> bool"`).
    pub signature: String,
    /// Relative path to the source file from the crate root.
    pub file_path: String,
    /// 1-based line number of the first line of the definition.
    pub start_line: usize,
    /// 1-based line number of the last line of the definition.
    pub end_line: usize,
}

/// Result envelope for a fuzzy/exact symbol search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    /// The original search query string.
    pub query: String,
    /// Matched symbols in relevance order.
    pub symbols: Vec<SymbolRef>,
}

/// A contiguous byte/line range extracted from a source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSlice {
    /// Relative path to the source file from the crate root.
    pub file_path: String,
    /// 1-based starting line of the slice.
    pub start_line: usize,
    /// 1-based ending line of the slice (inclusive).
    pub end_line: usize,
    /// Byte offset of the slice start within the file.
    pub byte_start: usize,
    /// Byte offset of the slice end within the file (exclusive).
    pub byte_end: usize,
    /// Extracted source text for the slice.
    pub content: String,
}

/// A source slice optionally anchored to a specific symbol.
///
/// When `symbol_id` is `Some`, the slice covers the exact definition of that
/// symbol. When `None`, the slice is a raw line-range extraction with no
/// symbol association.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSlice {
    /// Canonical symbol id, if the slice was resolved from a symbol query.
    pub symbol_id: Option<String>,
    /// The extracted source range. `#[serde(flatten)]` merges all
    /// [`FileSlice`] fields into the same JSON object level.
    #[serde(flatten)]
    pub slice: FileSlice,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_graph_detail_as_str_for_each_variant() {
        assert_eq!(CallGraphDetail::Internal.as_str(), "internal");
        assert_eq!(CallGraphDetail::External.as_str(), "external");
        assert_eq!(CallGraphDetail::All.as_str(), "all");
    }

    #[test]
    fn ensemble_preset_serializes_as_lowercase_kebab() {
        let val = serde_json::to_value(&EnsemblePreset::Deep).unwrap();
        assert_eq!(val, serde_json::json!("Deep"));
    }

    #[test]
    fn ensemble_view_round_trip_via_serde() {
        let original = EnsembleView::Usage;
        let json = serde_json::to_string(&original).unwrap();
        let back: EnsembleView = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn project_summary_serializes_minimal_fields() {
        let summary = ProjectSummary {
            slug: "s".to_string(),
            display_name: "d".to_string(),
            path: None,
            last_active_unix_ms: 0,
            primary_language: None,
            languages: vec![],
            supported: false,
            pinned: false,
            discovered_from: "x".to_string(),
            active_sessions: vec![],
            indexed_at_unix_ms: None,
            counts: None,
        };
        let val = serde_json::to_value(&summary).unwrap();
        assert_eq!(val["slug"], "s");
        assert_eq!(val["display_name"], "d");
    }

    #[test]
    fn source_slice_flattens_inner_slice_fields_via_serde() {
        let s = SourceSlice {
            symbol_id: Some("id".to_string()),
            slice: FileSlice {
                file_path: "src/lib.rs".to_string(),
                start_line: 1,
                end_line: 2,
                byte_start: 0,
                byte_end: 4,
                content: "fn x(){}".to_string(),
            },
        };
        let val = serde_json::to_value(&s).unwrap();
        assert_eq!(val["symbol_id"], "id");
        assert_eq!(val["file_path"], "src/lib.rs");
        assert_eq!(val["start_line"], 1);
    }

    #[test]
    fn symbol_ref_round_trips_via_serde() {
        let s = SymbolRef {
            symbol_id: "sid".to_string(),
            kind: "function".to_string(),
            name: "foo".to_string(),
            signature: "fn foo()".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 5,
            end_line: 5,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: SymbolRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "foo");
        assert_eq!(back.start_line, 5);
    }
}
