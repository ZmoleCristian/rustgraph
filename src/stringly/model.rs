use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Where a suggested replacement type comes from and what adopting it costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TypeOrigin {
    /// `core`, `alloc`, or `std`; no Cargo dependency is required.
    StandardLibrary,
    /// A crate that already appears in one of the project's Cargo manifests.
    ExistingDependency,
    /// A crate that is not currently declared and would be a new dependency.
    ExternalCrate,
    /// A project-owned enum, newtype, or other nominal type.
    LocalType,
}

impl TypeOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StandardLibrary => "standard-library",
            Self::ExistingDependency => "existing-dependency",
            Self::ExternalCrate => "external-crate",
            Self::LocalType => "local-type",
        }
    }
}

impl fmt::Display for TypeOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Declaration shape whose textual type may be overly broad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StringSiteKind {
    StructField,
    FunctionParameter,
    LocalBinding,
}

impl StringSiteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StructField => "struct-field",
            Self::FunctionParameter => "function-parameter",
            Self::LocalBinding => "local-binding",
        }
    }
}

/// Machine-readable category for one piece of detector evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StringlyEvidenceKind {
    ExplicitConversion,
    LiteralComparison,
    LiteralMatch,
    LiteralConstruction,
    LiteralCallSite,
    SharedVocabulary,
    StructuredLiteral,
    SemanticName,
}

/// How often and in which contexts the detector observed literal evidence.
///
/// `distinct` describes the vocabulary size; `observations` describes reuse.
/// A tiny vocabulary seen many times is materially stronger enum evidence than
/// one unrelated label per call site.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringlyLiteralStats {
    pub distinct: usize,
    pub observations: usize,
    pub comparisons: usize,
    pub matches: usize,
    pub constructions: usize,
    pub call_sites: usize,
}

/// Why an otherwise plausible recommendation is risky or normally omitted.
///
/// The first five variants make a finding non-actionable by default.  API and
/// representation caveats remain visible because the declaration can still be
/// improved, but its migration needs more care than a local type substitution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StringlyCaveatKind {
    GeneratedCode,
    VendoredCode,
    TraitContract,
    TextBoundary,
    ImplementationDetail,
    PublicApi,
    ExternalRepresentation,
}

impl StringlyCaveatKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GeneratedCode => "generated-code",
            Self::VendoredCode => "vendored-code",
            Self::TraitContract => "trait-contract",
            Self::TextBoundary => "text-boundary",
            Self::ImplementationDetail => "implementation-detail",
            Self::PublicApi => "public-api",
            Self::ExternalRepresentation => "external-representation",
        }
    }

    /// Non-actionable contexts omitted unless `include_suppressed` is enabled.
    pub fn suppresses_by_default(self) -> bool {
        matches!(
            self,
            Self::GeneratedCode
                | Self::VendoredCode
                | Self::TraitContract
                | Self::TextBoundary
                | Self::ImplementationDetail
        )
    }
}

/// One migration caveat attached to a recommendation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringlyCaveat {
    pub kind: StringlyCaveatKind,
    pub message: String,
    /// `true` means the finding is omitted unless suppressed results are
    /// explicitly requested.
    pub suppresses_by_default: bool,
}

/// One independently inspectable reason behind a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringlyEvidence {
    pub kind: StringlyEvidenceKind,
    pub message: String,
    pub file_path: String,
    pub line: usize,
}

/// Replacement proposed by the detector, including its dependency impact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedType {
    /// Path to write in this project. It honours renamed Cargo dependencies.
    pub display: String,
    /// Ecosystem-canonical path, independent of local dependency aliases.
    pub canonical_path: String,
    pub origin: TypeOrigin,
    /// Cargo package name for dependency-backed suggestions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
    /// Local dependency alias when it differs from `crate_name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_alias: Option<String>,
    pub new_dependency: bool,
    /// Meaningful for `local-type`: distinguishes a project type that already
    /// exists from a new enum/newtype name proposed by the detector.
    pub local_type_exists: bool,
}

/// A ranked opportunity to replace a textual declaration with a stronger type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StringlyFinding {
    pub file_path: String,
    pub line: usize,
    pub name: String,
    pub owner: String,
    pub site_kind: StringSiteKind,
    pub declared_type: String,
    pub suggestion: SuggestedType,
    /// Calibrated heuristic score in `[0.0, 1.0]`, not a compiler verdict.
    pub confidence: f64,
    pub evidence: Vec<StringlyEvidence>,
    /// Distinct, bounded string-literal previews that contributed to a local
    /// enum suggestion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub literals: Vec<String>,
    /// Literal cardinality and reuse behind a vocabulary-based suggestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub literal_stats: Option<StringlyLiteralStats>,
    /// Context that affects whether and how the declaration should migrate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caveats: Vec<StringlyCaveat>,
    /// True when this declaration is under a test module/function or test path.
    pub is_test: bool,
}

impl StringlyFinding {
    pub fn is_suppressed_by_default(&self) -> bool {
        self.caveats
            .iter()
            .any(|caveat| caveat.suppresses_by_default)
    }
}

/// Counts of candidates withheld from the default actionable report.
///
/// A single candidate can contribute to more than one reason (for example a
/// generated trait implementation containing a parser boundary), while
/// `suppressed_findings` in [`StringlyReport`] remains a unique count.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringlySuppressionCounts {
    pub generated_code: usize,
    pub vendored_code: usize,
    pub trait_contract: usize,
    pub text_boundary: usize,
    pub implementation_detail: usize,
}

impl StringlySuppressionCounts {
    pub(crate) fn record(&mut self, kind: StringlyCaveatKind) {
        match kind {
            StringlyCaveatKind::GeneratedCode => self.generated_code += 1,
            StringlyCaveatKind::VendoredCode => self.vendored_code += 1,
            StringlyCaveatKind::TraitContract => self.trait_contract += 1,
            StringlyCaveatKind::TextBoundary => self.text_boundary += 1,
            StringlyCaveatKind::ImplementationDetail => self.implementation_detail += 1,
            StringlyCaveatKind::PublicApi | StringlyCaveatKind::ExternalRepresentation => {}
        }
    }
}

/// Detector configuration shared by library, CLI, and MCP entry points.
#[derive(Debug, Clone)]
pub struct StringlyConfig {
    /// Primary crate root followed by optional merged (`--also`) roots.
    pub project_roots: Vec<PathBuf>,
    pub min_confidence: f64,
    pub include_tests: bool,
    /// Include candidates that are normally withheld because they sit on an
    /// intentional text boundary, a fixed trait signature, generated/vendor
    /// code, or point at parser machinery rather than a value type.
    pub include_suppressed: bool,
}

impl Default for StringlyConfig {
    fn default() -> Self {
        Self {
            project_roots: vec![PathBuf::from(".")],
            min_confidence: 0.85,
            include_tests: true,
            include_suppressed: false,
        }
    }
}

/// Complete uncapped result from one detector run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StringlyReport {
    pub scanned_string_sites: usize,
    /// Unique candidate count omitted from `findings` under the default
    /// configuration. This remains populated when suppressed results are
    /// included for auditing.
    pub suppressed_findings: usize,
    pub suppression_counts: StringlySuppressionCounts,
    pub findings: Vec<StringlyFinding>,
}
