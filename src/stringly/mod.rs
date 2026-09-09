//! Evidence-ranked detection of declarations that use `String`/`&str` where a
//! more precise type is likely to encode the program's existing invariants.
//!
//! This is deliberately a recommendation engine rather than a linter verdict:
//! every finding carries its evidence, confidence, source origin, and Cargo
//! dependency impact.

mod analyze;
mod dependencies;
mod model;

pub use analyze::detect_stringly;
pub use model::{
    StringSiteKind, StringlyCaveat, StringlyCaveatKind, StringlyConfig, StringlyEvidence,
    StringlyEvidenceKind, StringlyFinding, StringlyLiteralStats, StringlyReport,
    StringlySuppressionCounts, SuggestedType, TypeOrigin,
};
