use super::super::visitor::make_function_id;
use super::super::{CallSite, FunctionInfo};
use super::types::{EdgeSemantics, TypeInfo, TypedEdge};
use std::collections::HashMap;

/// Parse a Rust type string into its structural [`TypeInfo`] components.
///
/// Strips reference sigils, `Option<>`, `Result<>`, and `Vec<>` wrappers to
/// expose the inner base type while recording each modifier as a boolean flag.
pub fn extract_type_info(ty: &str) -> TypeInfo {
    let original = ty.trim();
    let is_optional = original.starts_with("Option<") || original.starts_with("Result<");
    let is_collection =
        original.contains("Vec<") || original.contains("[]") || original.contains("HashMap");
    let is_mutable = original.contains("&mut");
    let is_reference = original.starts_with("&");

    let mut base = original.to_string();
    base = base.replace("&mut ", "").replace("&", "");

    if let Some(start) = base.find("Option<")
        && let Some(end) = base[start..].rfind('>')
    {
        base = format!("{}{}", &base[..start], &base[start + 7..end]);
    }

    if let Some(start) = base.find("Result<")
        && let Some(end) = base[start..].rfind('>')
    {
        base = format!("{}{}", &base[..start], &base[start + 7..end]);
    }

    if let Some(start) = base.find("Vec<")
        && let Some(end) = base[start..].rfind('>')
    {
        base = format!("{}{}", &base[..start], &base[start + 4..end]);
    }

    base = base.trim().to_string();

    TypeInfo {
        base,
        is_mutable,
        is_reference,
        is_optional,
        is_collection,
    }
}

/// Build a [`TypedEdge`] for each call-site where both the caller and callee
/// are known indexed functions.
///
/// Edge semantics (pass-through, transform, side-effect, etc.) are inferred
/// from the caller's parameter types and the callee's return type.  An
/// `is_async_boundary` flag is set when the two functions differ in asyncness.
pub fn build_type_aware_edges(
    functions: &[FunctionInfo],
    call_sites: &[CallSite],
) -> Vec<TypedEdge> {
    let mut typed_edges = Vec::new();

    let func_by_id: HashMap<String, &FunctionInfo> = functions
        .iter()
        .map(|f| (make_function_id(&f.file_path, f.start_line, &f.name), f))
        .collect();

    let func_by_name: HashMap<String, Vec<&FunctionInfo>> = functions.iter().fold(
        HashMap::new(),
        |mut acc: HashMap<String, Vec<&FunctionInfo>>, f| {
            acc.entry(f.name.clone()).or_default().push(f);
            acc
        },
    );

    for call in call_sites {
        let Some(caller_key) = &call.caller_id else {
            continue;
        };
        let Some(caller) = func_by_id.get(caller_key) else {
            continue;
        };
        let callee_name = &call.callee_base;

        let Some(callees) = func_by_name.get(callee_name) else {
            continue;
        };
        if callees.is_empty() {
            continue;
        }

        let Some(callee) = callees.first() else {
            continue;
        };


        let caller_input_types: Vec<TypeInfo> = caller
            .parameters
            .iter()
            .filter_map(|p| {
                let p = p.trim();
                if p == "self" || p == "&self" || p == "&mut self" {
                    return None;
                }
                Some(extract_type_info(p))
            })
            .collect();

        let callee_output: Option<TypeInfo> = callee.return_type.as_ref().and_then(|r| {
            let r = r.trim();
            if r == "()" {
                return None;
            }
            Some(extract_type_info(r))
        });

        let semantics = if let (Some(callee_out), Some(caller_in)) =
            (callee_output.as_ref(), caller_input_types.first())
        {
            if callee_out.base == caller_in.base {
                EdgeSemantics::PassThrough
            } else if callee_out.base != caller_in.base {
                EdgeSemantics::Transform {
                    from_type: caller_in.base.clone(),
                    to_type: callee_out.base.clone(),
                }
            } else {
                EdgeSemantics::Unknown
            }
        } else if caller.parameters.iter().any(|p| p.contains("&mut"))
            && callee
                .return_type
                .as_ref()
                .is_some_and(|r| r.trim() == "()")
        {
            EdgeSemantics::SideEffect
        } else {
            EdgeSemantics::Unknown
        };

        let data_types: Vec<String> = caller
            .parameters
            .iter()
            .filter_map(|p| {
                let info = extract_type_info(p);
                if info.base.is_empty() || info.base == "self" {
                    return None;
                }
                Some(info.base)
            })
            .collect();

        typed_edges.push(TypedEdge {
            caller: caller.name.clone(),
            callee: callee_name.clone(),
            semantics,
            data_types,
            is_async_boundary: caller.is_async != callee.is_async,
        });
    }

    typed_edges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_func(name: &str, params: Vec<String>, ret: Option<&str>, is_async: bool) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}"),
            file_path: format!("src/{name}.rs"),
            start_line: 1,
            end_line: 1,
            is_async,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: vec![],
            parameters: params,
            return_type: ret.map(String::from),
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    #[test]
    fn extract_type_info_detects_optional_and_collection() {
        let info = extract_type_info("Option<Vec<u32>>");
        assert!(info.is_optional);
        assert!(info.is_collection);
        assert!(!info.is_reference);
    }

    #[test]
    fn extract_type_info_detects_reference_and_mutability() {
        let info = extract_type_info("&mut Vec<u32>");
        assert!(info.is_mutable);
        assert!(info.is_reference);
        assert!(info.is_collection);
    }

    #[test]
    fn extract_type_info_detects_result_as_optional() {
        let info = extract_type_info("Result<u32, String>");
        assert!(info.is_optional);
    }

    #[test]
    fn extract_type_info_strips_wrappers_from_base() {
        let info = extract_type_info("Option<Vec<MyType>>");
        assert!(info.base.contains("MyType"));
    }

    #[test]
    fn extract_type_info_handles_simple_type() {
        let info = extract_type_info("u32");
        assert_eq!(info.base, "u32");
        assert!(!info.is_optional);
        assert!(!info.is_collection);
        assert!(!info.is_reference);
        assert!(!info.is_mutable);
    }

    #[test]
    fn extract_type_info_detects_hashmap_as_collection() {
        let info = extract_type_info("HashMap<String, u32>");
        assert!(info.is_collection);
    }

    #[test]
    fn build_type_aware_edges_returns_empty_when_no_callsites() {
        let funcs = vec![make_func("a", vec![], None, false)];
        let edges = build_type_aware_edges(&funcs, &[]);
        assert!(edges.is_empty());
    }

    #[test]
    fn build_type_aware_edges_skips_callsites_with_unknown_caller() {
        let funcs = vec![make_func("callee", vec![], None, false)];
        let cs = CallSite {
            caller_id: Some("nonexistent".to_string()),
            caller_name: Some("nonexistent".to_string()),
            callee: "callee".to_string(),
            callee_base: "callee".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/x.rs".to_string(),
            line: 1,
            column: 0,
        };
        let edges = build_type_aware_edges(&funcs, &[cs]);
        assert!(edges.is_empty());
    }

    #[test]
    fn build_type_aware_edges_marks_async_boundary() {
        let caller = make_func("caller", vec!["x: u32".to_string()], None, true);
        let callee = make_func("callee", vec![], None, false);
        let funcs = vec![caller.clone(), callee.clone()];

        let cs = CallSite {
            caller_id: Some(make_function_id(
                &caller.file_path,
                caller.start_line,
                &caller.name,
            )),
            caller_name: Some(caller.name.clone()),
            callee: "callee".to_string(),
            callee_base: "callee".to_string(),
            call_kind: "function".to_string(),
            file_path: "src/caller.rs".to_string(),
            line: 1,
            column: 0,
        };
        let edges = build_type_aware_edges(&funcs, &[cs]);
        assert_eq!(edges.len(), 1);
        assert!(edges[0].is_async_boundary);
    }

    #[test]
    fn build_type_aware_edges_resolves_production_shaped_caller_id() {
        // Regression: in production, `caller_id` is built via
        // `make_function_id` and has the 3-part `file:line:name` shape.
        // The lookup table must be keyed identically or the function
        // silently returns an empty edge list for every real input.
        let caller = make_func("caller", vec!["x: u32".to_string()], None, false);
        let callee = make_func("callee", vec![], None, false);
        let funcs = vec![caller.clone(), callee.clone()];

        let caller_id =
            make_function_id(&caller.file_path, caller.start_line, &caller.name);
        // Sanity: production format is 3 parts (file:line:name), not 2.
        assert_eq!(caller_id.split(':').count(), 3);

        let cs = CallSite {
            caller_id: Some(caller_id),
            caller_name: Some(caller.name.clone()),
            callee: "callee".to_string(),
            callee_base: "callee".to_string(),
            call_kind: "function".to_string(),
            file_path: caller.file_path.clone(),
            line: 1,
            column: 0,
        };
        let edges = build_type_aware_edges(&funcs, &[cs]);
        assert_eq!(
            edges.len(),
            1,
            "edge must be produced for a production-shaped caller_id"
        );
        assert_eq!(edges[0].caller, "caller");
        assert_eq!(edges[0].callee, "callee");
    }

    #[test]
    fn build_type_aware_edges_rejects_legacy_two_part_caller_id() {
        // The old (broken) 2-part `file:line` shape must NOT match: the
        // lookup is keyed on the 3-part production id. This pins the fix
        // so a regression to the 2-part key would fail loudly.
        let caller = make_func("caller", vec![], None, false);
        let callee = make_func("callee", vec![], None, false);
        let funcs = vec![caller.clone(), callee.clone()];

        let cs = CallSite {
            caller_id: Some(format!("{}:{}", caller.file_path, caller.start_line)),
            caller_name: Some(caller.name.clone()),
            callee: "callee".to_string(),
            callee_base: "callee".to_string(),
            call_kind: "function".to_string(),
            file_path: caller.file_path.clone(),
            line: 1,
            column: 0,
        };
        let edges = build_type_aware_edges(&funcs, &[cs]);
        assert!(
            edges.is_empty(),
            "2-part legacy caller_id must not resolve against 3-part keys"
        );
    }
}
