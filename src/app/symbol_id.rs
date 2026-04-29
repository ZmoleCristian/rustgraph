//! Utilities for attaching stable symbol IDs to serialized output.
//!
//! Symbol IDs encode `file_path:start_line:name` (functions) or
//! `kind:file_path:start_line:name` (structs, enums) and are injected into
//! JSON responses so API consumers can reference individual symbols unambiguously.

use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    EnumInfo, FunctionInfo, StructInfo, enum_symbol_id, function_id, struct_symbol_id,
};

/// Wraps a serializable symbol value with its pre-computed stable symbol ID.
///
/// Serializes with `symbol_id` as a top-level field alongside all fields of `inner`
/// via `#[serde(flatten)]`.
#[derive(Debug, Serialize)]
pub struct WithSymbolId<T: Serialize> {
    /// Stable identifier for the wrapped symbol.
    pub symbol_id: String,
    #[serde(flatten)]
    /// The original symbol data.
    pub inner: T,
}

impl WithSymbolId<FunctionInfo> {
    /// Computes the function's symbol ID and wraps it together with the function info.
    pub fn wrap_fn(inner: FunctionInfo) -> Self {
        let symbol_id = function_id(&inner);
        Self { symbol_id, inner }
    }
}

impl WithSymbolId<StructInfo> {
    /// Computes the struct's symbol ID and wraps it together with the struct info.
    pub fn wrap_struct(inner: StructInfo) -> Self {
        let symbol_id = struct_symbol_id(&inner);
        Self { symbol_id, inner }
    }
}

impl WithSymbolId<EnumInfo> {
    /// Computes the enum's symbol ID and wraps it together with the enum info.
    pub fn wrap_enum(inner: EnumInfo) -> Self {
        let symbol_id = enum_symbol_id(&inner);
        Self { symbol_id, inner }
    }
}


/// Recursively walks a JSON `Value` and injects a `symbol_id` field into every object that
/// has `name`, `file_path`, and `start_line` fields but no existing `symbol_id`.
///
/// The kind is inferred from field presence: `variants` → enum, `fields` → struct,
/// otherwise fn. Modifies `value` in place.
pub fn inject_symbol_ids(value: &mut Value) {
    match value {
        Value::Object(map) => {

            if let (Some(Value::String(name)), Some(Value::String(file_path)), Some(Value::Number(line))) =
                (map.get("name"), map.get("file_path"), map.get("start_line"))
            {
                if !map.contains_key("symbol_id") {
                    let line_n = line.as_u64().unwrap_or(0);
                    let kind = guess_kind(map);


                    let id = if kind == "fn" {
                        format!("{}:{}:{}", file_path, line_n, name)
                    } else {
                        format!("{}:{}:{}:{}", kind, file_path, line_n, name)
                    };
                    map.insert("symbol_id".to_string(), json!(id));
                }
            }
            for (_, v) in map.iter_mut() {
                inject_symbol_ids(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                inject_symbol_ids(v);
            }
        }
        _ => {}
    }
}

fn guess_kind(obj: &serde_json::Map<String, Value>) -> &'static str {


    if obj.contains_key("variants") {
        return "enum";
    }
    if obj.contains_key("is_async") || obj.contains_key("parameters") || obj.contains_key("return_type") {
        return "fn";
    }
    if obj.contains_key("fields") {
        return "struct";
    }

    "fn"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_fn_emits_symbol_id_no_prefix() {
        let f = FunctionInfo {
            name: "demo".into(),
            signature: "fn demo".into(),
            file_path: "src/lib.rs".into(),
            start_line: 12,
            end_line: 14,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: vec![],
            parameters: vec![],
            return_type: None,
            kind: "function".into(),
            cfg_attrs: Vec::new(),
        };
        let wrapped = WithSymbolId::wrap_fn(f);
        let v = serde_json::to_value(&wrapped).unwrap();

        assert_eq!(v["symbol_id"].as_str().unwrap(), "src/lib.rs:12:demo");
        assert_eq!(v["name"].as_str().unwrap(), "demo");
    }

    #[test]
    fn inject_symbol_ids_walks_nested_callers_tree() {
        let mut v: Value = serde_json::from_str(r#"{
            "matches": [
                {
                    "info": {"name": "target", "file_path": "src/a.rs", "start_line": 5, "is_async": false},
                    "callers": [
                        {"info": {"name": "caller_a", "file_path": "src/b.rs", "start_line": 10, "is_async": false}}
                    ]
                }
            ]
        }"#).unwrap();
        inject_symbol_ids(&mut v);
        assert_eq!(
            v["matches"][0]["info"]["symbol_id"].as_str().unwrap(),
            "src/a.rs:5:target"
        );
        assert_eq!(
            v["matches"][0]["callers"][0]["info"]["symbol_id"].as_str().unwrap(),
            "src/b.rs:10:caller_a"
        );
    }

    #[test]
    fn inject_symbol_ids_distinguishes_struct_and_enum() {
        let mut v: Value = serde_json::from_str(r#"{
            "structs": [{"name": "S", "file_path": "src/lib.rs", "start_line": 1, "fields": []}],
            "enums":   [{"name": "E", "file_path": "src/lib.rs", "start_line": 5, "variants": []}]
        }"#).unwrap();
        inject_symbol_ids(&mut v);
        assert_eq!(v["structs"][0]["symbol_id"].as_str().unwrap(), "struct:src/lib.rs:1:S");
        assert_eq!(v["enums"][0]["symbol_id"].as_str().unwrap(), "enum:src/lib.rs:5:E");
    }
}
