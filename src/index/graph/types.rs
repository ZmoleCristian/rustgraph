use serde::{Deserialize, Serialize};

/// Structural metadata extracted from a single Rust type expression.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TypeInfo {
    /// Inner type name after stripping wrapper types and reference sigils.
    pub base: String,
    /// True when the original type contains `&mut`.
    pub is_mutable: bool,
    /// True when the original type starts with `&`.
    pub is_reference: bool,
    /// True when wrapped in `Option<>` or `Result<>`.
    pub is_optional: bool,
    /// True when wrapped in `Vec<>`, contains `[]`, or contains `HashMap`.
    pub is_collection: bool,
}

/// Inferred data-flow role of a directed call-graph edge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeSemantics {
    /// Caller passes the same type through to the callee unchanged.
    PassThrough,
    /// Caller feeds one type into the callee which produces a different type.
    Transform { from_type: String, to_type: String },
    /// Edge represents a conditional branch; `condition` is a hint string.
    Branch { condition: String },
    /// Callee aggregates multiple inputs into a single value.
    Aggregate,
    /// Caller distributes work to multiple downstream callees.
    FanOut,
    /// Callee mutates shared state via `&mut` parameters and returns `()`.
    SideEffect,
    /// Semantics could not be determined from available type information.
    Unknown,
}

/// A call-graph edge annotated with type and semantic information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedEdge {
    /// Name of the calling function.
    pub caller: String,
    /// Name of the called function.
    pub callee: String,
    /// Inferred data-flow role of this edge.
    pub semantics: EdgeSemantics,
    /// Base type names flowing across this edge (from caller parameters).
    pub data_types: Vec<String>,
    /// True when the caller and callee differ in `async`-ness.
    pub is_async_boundary: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_info_has_value_equality() {
        let a = TypeInfo {
            base: "Foo".to_string(),
            is_mutable: false,
            is_reference: true,
            is_optional: false,
            is_collection: false,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn edge_semantics_round_trips_through_serde() {
        for value in [
            EdgeSemantics::PassThrough,
            EdgeSemantics::Transform {
                from_type: "A".to_string(),
                to_type: "B".to_string(),
            },
            EdgeSemantics::Branch {
                condition: "x > 0".to_string(),
            },
            EdgeSemantics::Aggregate,
            EdgeSemantics::FanOut,
            EdgeSemantics::SideEffect,
            EdgeSemantics::Unknown,
        ] {
            let json = serde_json::to_string(&value).expect("serialize");
            let parsed: EdgeSemantics = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, value);
        }
    }
}
