//! Core data types emitted by the AST visitor and consumed by all higher-level
//! analysis passes.

use serde::{Deserialize, Serialize};


/// Format a slice of `cfg` predicate strings into a single bracketed annotation.
///
/// Returns `None` when the slice is empty.  Multiple predicates are joined with
/// `" && "`.
pub fn format_cfg_annotation(cfg_attrs: &[String]) -> Option<String> {
    if cfg_attrs.is_empty() {
        return None;
    }
    Some(format!("[cfg({})]", cfg_attrs.join(" && ")))
}

/// A single call expression recorded during AST traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSite {
    /// Stable ID of the enclosing function (`file:line:name`), or `None` for module-level code.
    pub caller_id: Option<String>,
    /// Display name of the enclosing function, or `None` for module-level code.
    pub caller_name: Option<String>,
    /// Full callee expression as written in source (e.g. `module::Type::method`).
    pub callee: String,
    /// Bare function/method name without qualification (last path segment).
    pub callee_base: String,
    /// Call kind tag: `"function"`, `"method"`, or `"macro"`.
    pub call_kind: String,
    /// Source file containing this call site.
    pub file_path: String,
    /// 1-based line number of the call expression.
    pub line: usize,
    /// 0-based column offset of the call expression.
    pub column: usize,
}

/// A call site after the resolution pass has attempted to identify the callee's definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedCallTarget {
    /// Stable ID of the calling function.
    pub caller_id: String,
    /// Full callee expression as written in source.
    pub callee: String,
    /// Bare callee name (last path segment).
    pub callee_base: String,
    /// Stable ID of the resolved callee function, or `None` if resolution failed.
    pub resolved_internal_id: Option<String>,
    /// Source file containing this call site.
    pub file_path: String,
    /// 1-based line number of the call expression.
    pub line: usize,
    /// 0-based column offset of the call expression.
    pub column: usize,
}

/// Metadata for a `struct` item extracted from source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructInfo {
    /// Unqualified name of the struct.
    pub name: String,
    /// Normalised `struct` declaration including generics (no body).
    pub signature: String,
    /// Source file that declares this struct.
    pub file_path: String,
    /// 1-based line where the struct starts.
    pub start_line: usize,
    /// 1-based line where the struct ends.
    pub end_line: usize,
    /// Whether the item has `pub` visibility.
    pub is_pub: bool,
    /// Generic parameter strings (e.g. `"T"`, `"'a"`).
    pub generics: Vec<String>,
    /// Field declarations as normalised strings.
    pub fields: Vec<String>,
    /// Item kind tag: always `"struct"`.
    pub kind: String,

    /// Accumulated `cfg(...)` predicates from enclosing modules and the item itself.
    #[serde(default)]
    pub cfg_attrs: Vec<String>,
}

/// A single variant belonging to an [`EnumInfo`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariantInfo {
    /// Variant name.
    pub name: String,
    /// Field declarations (empty for unit variants).
    pub fields: Vec<String>,
    /// Explicit discriminant expression, if present.
    pub discriminant: Option<String>,
}

/// Metadata for an `enum` item extracted from source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumInfo {
    /// Unqualified name of the enum.
    pub name: String,
    /// Normalised `enum` declaration including generics (no body).
    pub signature: String,
    /// Source file that declares this enum.
    pub file_path: String,
    /// 1-based line where the enum starts.
    pub start_line: usize,
    /// 1-based line where the enum ends.
    pub end_line: usize,
    /// Whether the item has `pub` visibility.
    pub is_pub: bool,
    /// Generic parameter strings.
    pub generics: Vec<String>,
    /// All variants of the enum.
    pub variants: Vec<EnumVariantInfo>,
    /// Item kind tag: always `"enum"`.
    pub kind: String,

    /// Accumulated `cfg(...)` predicates from enclosing modules and the item itself.
    #[serde(default)]
    pub cfg_attrs: Vec<String>,
}

/// Metadata for a `const` or `static` item extracted from source.
///
/// The signature deliberately excludes the initializer expression — a const
/// table can span hundreds of lines, and the locator contract is to point at
/// `file:start-end` for the Read tool, not to inline the data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstInfo {
    /// Unqualified name of the const/static.
    pub name: String,
    /// Normalised declaration without the initializer (e.g. `pub const KIND_META: [KindMeta; 54]`).
    pub signature: String,
    /// Declared type as a normalised string.
    pub ty: String,
    /// Source file that declares this item.
    pub file_path: String,
    /// 1-based line where the item starts.
    pub start_line: usize,
    /// 1-based line where the item ends (spans the full initializer).
    pub end_line: usize,
    /// Whether the item has `pub` visibility.
    pub is_pub: bool,
    /// Item kind tag: `"const"`, `"static"`, `"static mut"`, or `"const(<Type>)"` for associated consts.
    pub kind: String,

    /// Accumulated `cfg(...)` predicates from enclosing modules and the item itself.
    #[serde(default)]
    pub cfg_attrs: Vec<String>,
}

/// Metadata for a `trait` declaration or `type` alias extracted from source.
///
/// Both are "type-shaped declarations" that share a findable surface; the
/// `kind` tag distinguishes them so one index column covers both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDeclInfo {
    /// Unqualified name of the trait or alias.
    pub name: String,
    /// Normalised declaration: `pub trait Foo: Bar` or `pub type Foo = Rhs`.
    pub signature: String,
    /// Alias right-hand side as a normalised string; `None` for traits.
    pub rhs: Option<String>,
    /// Source file that declares this item.
    pub file_path: String,
    /// 1-based line where the item starts.
    pub start_line: usize,
    /// 1-based line where the item ends (traits span their full body).
    pub end_line: usize,
    /// Whether the item has `pub` visibility.
    pub is_pub: bool,
    /// Item kind tag: `"trait"` or `"alias"`.
    pub kind: String,

    /// Accumulated `cfg(...)` predicates from enclosing modules and the item itself.
    #[serde(default)]
    pub cfg_attrs: Vec<String>,
}

/// Metadata for a free function, method, or trait function extracted from source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    /// Unqualified function name.
    pub name: String,
    /// Normalised function signature excluding the body.
    pub signature: String,
    /// Source file that declares this function.
    pub file_path: String,
    /// 1-based line where the function starts.
    pub start_line: usize,
    /// 1-based line where the function ends.
    pub end_line: usize,
    /// `true` if the function has the `async` keyword.
    pub is_async: bool,
    /// `true` if the function has the `unsafe` keyword.
    pub is_unsafe: bool,
    /// `true` if the function has `pub` visibility.
    pub is_pub: bool,
    /// `true` if the function has the `const` keyword.
    pub is_const: bool,
    /// `true` if the function is inside a `#[cfg(test)]` context or annotated with `#[test]`.
    pub is_test: bool,
    /// Generic parameter strings.
    pub generics: Vec<String>,
    /// Parameter declarations as normalised strings.
    pub parameters: Vec<String>,
    /// Return type as a normalised string, or `None` for `()`.
    pub return_type: Option<String>,
    /// Kind tag: `"function"`, `"method(<Type>)"`, or `"trait_fn(<Trait>)"`.
    pub kind: String,

    /// Accumulated `cfg(...)` predicates from enclosing modules and the item itself.
    #[serde(default)]
    pub cfg_attrs: Vec<String>,
}

/// A single numbered source line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeLine {
    /// 1-based line number.
    pub line: usize,
    /// Raw text of the line (no trailing newline).
    pub text: String,
}

/// A contiguous slice of source lines from a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSnippet {
    /// Source file the snippet was read from.
    pub file_path: String,
    /// 1-based first line of the snippet.
    pub start_line: usize,
    /// 1-based last line of the snippet.
    pub end_line: usize,
    /// Individual lines with their numbers.
    pub lines: Vec<CodeLine>,
}

/// A struct paired with its source snippet, used inside [`FunctionEnsemble`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructEnsemble {
    /// Parsed metadata for the struct.
    pub info: StructInfo,
    /// Source code of the struct definition.
    pub code: CodeSnippet,
}

/// A call site enriched with the original source line text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSiteWithLine {
    /// Stable ID of the enclosing function, or `None` for module-level code.
    pub caller_id: Option<String>,
    /// Display name of the enclosing function.
    pub caller_name: Option<String>,
    /// Full callee expression as written in source.
    pub callee: String,
    /// Bare callee name without qualification.
    pub callee_base: String,
    /// Source file containing this call site.
    pub file_path: String,
    /// 1-based line number of the call.
    pub line: usize,
    /// 0-based column of the call.
    pub column: usize,
    /// Raw text of the source line at this call site.
    pub line_text: String,
}

/// Full context bundle for a single function, as returned by the ensemble query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionEnsemble {
    /// Parsed metadata for the function.
    pub info: FunctionInfo,
    /// Source code of the function body.
    pub code: CodeSnippet,
    /// Structs referenced in the function's signature or body.
    pub structs_used: Vec<StructEnsemble>,
    /// Total number of call sites inside this function before any truncation.
    pub call_sites_total: usize,
    /// `true` when `call_sites` was capped at `max_call_sites`.
    pub call_sites_truncated: bool,
    /// Call sites inside the function, up to the configured limit.
    pub call_sites: Vec<CallSiteWithLine>,
    /// Immediate callers and callees up to the configured call depth.
    pub neighborhood: FunctionNeighborhood,
    /// Per-type lifecycle analysis for types appearing in this function.
    pub lifecycles: Vec<TypeLifecycle>,
    /// Inferred local value-flow hints (producer → consumer pairs).
    pub dataflow_hints: Vec<ValueFlowHint>,
    /// Detected I/O or cross-boundary signals.
    pub io_boundaries: Vec<IoBoundarySignal>,
}

/// A function related to a focal function by call-graph distance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedFunction {
    /// Stable ID (`file:line:name`) of the related function.
    pub id: String,
    /// Unqualified name.
    pub name: String,
    /// Source file containing the related function.
    pub file_path: String,
    /// 1-based start line.
    pub start_line: usize,
    /// 1-based end line.
    pub end_line: usize,
    /// Normalised function signature.
    pub signature: String,
    /// Number of call-graph hops from the focal function.
    pub distance: usize,
}

/// Callers and callees of a focal function within a configurable call depth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionNeighborhood {
    /// Maximum number of hops traversed to find related functions.
    pub call_depth: usize,
    /// Cap applied to each of the upstream/downstream lists.
    pub max_related: usize,
    /// Total number of upstream (caller) functions before truncation.
    pub upstream_total: usize,
    /// `true` when `upstream` was capped at `max_related`.
    pub upstream_truncated: bool,
    /// Functions that call the focal function, up to `max_related`.
    pub upstream: Vec<RelatedFunction>,
    /// Total number of downstream (callee) functions before truncation.
    pub downstream_total: usize,
    /// `true` when `downstream` was capped at `max_related`.
    pub downstream_truncated: bool,
    /// Functions called by the focal function, up to `max_related`.
    pub downstream: Vec<RelatedFunction>,
    /// Callee names from the focal function that could not be resolved to a definition.
    pub unresolved_outgoing_calls: Vec<String>,
}

/// A single function node on a type lifecycle path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleHop {
    /// Stable ID of this function.
    pub id: String,
    /// Unqualified function name.
    pub name: String,
    /// Source file containing the function.
    pub file_path: String,
    /// 1-based start line of the function.
    pub start_line: usize,
}

/// An ordered chain of functions through which a type flows, from producer to consumer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecyclePath {
    /// Ordered sequence of functions along the path.
    pub hops: Vec<LifecycleHop>,
}

/// Full lifecycle analysis for a single type: which functions create, transport, and consume it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeLifecycle {
    /// The type name that was analysed.
    pub type_name: String,
    /// Total number of functions that mention this type.
    pub functions_total: usize,
    /// Total number of entrypoint (producer-only) functions before truncation.
    pub entrypoints_total: usize,
    /// Total number of transit (producer and consumer) functions before truncation.
    pub transit_total: usize,
    /// Total number of endpoint (consumer-only) functions before truncation.
    pub endpoints_total: usize,
    /// Total number of isolated functions (no lifecycle edges) before truncation.
    pub isolated_total: usize,
    /// `true` when any of the four lists was capped at `max_related`.
    pub functions_truncated: bool,
    /// Functions that produce the type but are not themselves consumers.
    pub entrypoints: Vec<RelatedFunction>,
    /// Functions that both receive and pass along the type.
    pub transit: Vec<RelatedFunction>,
    /// Functions that consume the type but do not produce it.
    pub endpoints: Vec<RelatedFunction>,
    /// Functions that mention the type but have no resolved lifecycle edges.
    pub isolated: Vec<RelatedFunction>,
    /// Representative data-flow paths from entrypoints to endpoints.
    pub sample_paths: Vec<LifecyclePath>,
    /// `true` when `sample_paths` was capped at `max_lifecycle_paths`.
    pub paths_truncated: bool,
}

/// A heuristic hint that a local variable flows from one call to another within a function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueFlowHint {
    /// Name of the local variable that carries the value.
    pub variable: String,
    /// Call expression that appears to produce the variable.
    pub producer_call: String,
    /// Line of the producer call.
    pub producer_line: usize,
    /// Call expression that appears to consume the variable.
    pub consumer_call: String,
    /// Line of the consumer call.
    pub consumer_line: usize,
    /// Source file where both calls appear.
    pub file_path: String,
    /// Raw source text of the producer line.
    pub producer_text: String,
    /// Raw source text of the consumer line.
    pub consumer_text: String,
}

/// A detected I/O or cross-boundary signal associated with a function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoBoundarySignal {
    /// Boundary category (e.g. `"network"`, `"filesystem"`, `"ipc"`).
    pub boundary: String,
    /// Role of the function at this boundary (e.g. `"reader"`, `"writer"`).
    pub role: String,
    /// Confidence score in [0, 1].
    pub confidence: f64,
    /// Stable ID of the function where the signal was detected.
    pub function_id: String,
    /// Unqualified name of the function.
    pub function_name: String,
    /// Source file of the function.
    pub file_path: String,
    /// 1-based start line of the function.
    pub start_line: usize,
    /// Call names or patterns that contributed to the signal classification.
    pub evidence: Vec<String>,
}

/// Top-level response returned by an ensemble query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionEnsembleResult {
    /// The original search query string.
    pub query: String,
    /// Fuzzy-match threshold used for the search.
    pub threshold: f64,
    /// Maximum number of function matches included.
    pub max_results: usize,
    /// Maximum number of call sites per function.
    pub max_call_sites: usize,
    /// Call-graph depth used for neighborhood expansion.
    pub call_depth: usize,
    /// Cap applied to each neighborhood list.
    pub max_related: usize,
    /// Maximum number of lifecycle paths per type.
    pub max_lifecycle_paths: usize,
    /// Maximum number of functions shown per lifecycle role.
    pub lifecycle_max_functions: usize,
    /// Type names for which lifecycle analysis was requested.
    pub lifecycle_types: Vec<String>,
    /// Section names included in the response (controls which fields are populated).
    pub sections: Vec<String>,
    /// One ensemble bundle per matched function.
    pub matches: Vec<FunctionEnsemble>,
}
