//! Fuzzy and exact symbol search across functions, structs, and enums.

use crate::api::{SearchResults, SymbolRef};
use crate::project::ProjectData;
use crate::{EnumInfo, FunctionInfo, StructInfo, function_id};

/// Search `project` for symbols whose name matches `query`.
///
/// Exact name matches are preferred; falls back to fuzzy matching against names.
/// `limit = 0` returns all matches.
pub fn search_symbols(
    project: &ProjectData,
    query: &str,
    threshold: f64,
    limit: usize,
) -> SearchResults {
    search_symbols_with_options(project, query, threshold, limit, false)
}


/// Search `project` for symbols matching `query`, with full control over options.
///
/// When `match_signature` is `true` and no name matches are found, the search
/// widens to include the full signature and file path. Results are sorted by
/// file path, then start line, then name, and truncated to `limit` (0 = unlimited).
pub fn search_symbols_with_options(
    project: &ProjectData,
    query: &str,
    threshold: f64,
    limit: usize,
    match_signature: bool,
) -> SearchResults {
    let q_lower = query.to_lowercase();


    let mut exact: Vec<SymbolRef> = Vec::new();
    for f in &project.functions {
        if f.name.to_lowercase() == q_lower {
            exact.push(symbol_from_function(f));
        }
    }
    for s in &project.structs {
        if s.name.to_lowercase() == q_lower {
            exact.push(symbol_from_struct(s));
        }
    }
    for e in &project.enums {
        if e.name.to_lowercase() == q_lower {
            exact.push(symbol_from_enum(e));
        }
    }
    let symbols = if !exact.is_empty() {
        exact
    } else {

        let mut name_hits: Vec<SymbolRef> = Vec::new();
        for f in &project.functions {
            if crate::fuzzy_match(query, &f.name, threshold) {
                name_hits.push(symbol_from_function(f));
            }
        }
        for s in &project.structs {
            if crate::fuzzy_match(query, &s.name, threshold) {
                name_hits.push(symbol_from_struct(s));
            }
        }
        for e in &project.enums {
            if crate::fuzzy_match(query, &e.name, threshold) {
                name_hits.push(symbol_from_enum(e));
            }
        }
        if !name_hits.is_empty() || !match_signature {
            name_hits
        } else {

            let mut wide: Vec<SymbolRef> = Vec::new();
            for f in &project.functions {
                if crate::fuzzy_match(query, &f.signature, threshold)
                    || crate::fuzzy_match(query, &f.file_path, threshold)
                {
                    wide.push(symbol_from_function(f));
                }
            }
            for s in &project.structs {
                if crate::fuzzy_match(query, &s.signature, threshold)
                    || crate::fuzzy_match(query, &s.file_path, threshold)
                {
                    wide.push(symbol_from_struct(s));
                }
            }
            for e in &project.enums {
                if crate::fuzzy_match(query, &e.signature, threshold)
                    || crate::fuzzy_match(query, &e.file_path, threshold)
                {
                    wide.push(symbol_from_enum(e));
                }
            }
            wide
        }
    };

    let mut symbols = symbols;
    symbols.sort_by(|left, right| {
        left.file_path
            .cmp(&right.file_path)
            .then(left.start_line.cmp(&right.start_line))
            .then(left.name.cmp(&right.name))
    });
    if limit != 0 && symbols.len() > limit {
        symbols.truncate(limit);
    }

    SearchResults {
        query: query.to_string(),
        symbols,
    }
}

/// Build a stable symbol ID for a struct or enum.
///
/// Format: `<kind>:<file_path>:<start_line>:<name>`.
pub(super) fn make_named_symbol_id(
    kind: &str,
    file_path: &str,
    start_line: usize,
    name: &str,
) -> String {
    format!("{kind}:{file_path}:{start_line}:{name}")
}

fn symbol_from_function(function: &FunctionInfo) -> SymbolRef {
    SymbolRef {
        symbol_id: function_id(function),
        kind: function.kind.clone(),
        name: function.name.clone(),
        signature: function.signature.clone(),
        file_path: function.file_path.clone(),
        start_line: function.start_line,
        end_line: function.end_line,
    }
}

fn symbol_from_struct(struct_info: &StructInfo) -> SymbolRef {
    SymbolRef {
        symbol_id: make_named_symbol_id(
            "struct",
            &struct_info.file_path,
            struct_info.start_line,
            &struct_info.name,
        ),
        kind: struct_info.kind.clone(),
        name: struct_info.name.clone(),
        signature: struct_info.signature.clone(),
        file_path: struct_info.file_path.clone(),
        start_line: struct_info.start_line,
        end_line: struct_info.end_line,
    }
}

fn symbol_from_enum(enum_info: &EnumInfo) -> SymbolRef {
    SymbolRef {
        symbol_id: make_named_symbol_id(
            "enum",
            &enum_info.file_path,
            enum_info.start_line,
            &enum_info.name,
        ),
        kind: enum_info.kind.clone(),
        name: enum_info.name.clone(),
        signature: enum_info.signature.clone(),
        file_path: enum_info.file_path.clone(),
        start_line: enum_info.start_line,
        end_line: enum_info.end_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EnumVariantInfo;

    fn proj() -> ProjectData {
        let mut p = ProjectData::default();
        p.functions.push(FunctionInfo {
            name: "do_thing".into(),
            signature: "fn do_thing()".into(),
            file_path: "src/lib.rs".into(),
            start_line: 5,
            end_line: 7,
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
        });
        p.structs.push(StructInfo {
            name: "Foo".into(),
            signature: "struct Foo".into(),
            file_path: "src/foo.rs".into(),
            start_line: 1,
            end_line: 5,
            is_pub: true,
            generics: vec![],
            fields: vec![],
            kind: "struct".into(),
            cfg_attrs: Vec::new(),
        });
        p.enums.push(EnumInfo {
            name: "MyEnum".into(),
            signature: "enum MyEnum".into(),
            file_path: "src/foo.rs".into(),
            start_line: 10,
            end_line: 15,
            is_pub: true,
            generics: vec![],
            variants: vec![EnumVariantInfo {
                name: "VarA".into(),
                fields: vec![],
                discriminant: None,
            }],
            kind: "enum".into(),
            cfg_attrs: Vec::new(),
        });
        p
    }

    #[test]
    fn make_named_symbol_id_format() {
        let id = make_named_symbol_id("struct", "src/foo.rs", 1, "Foo");
        assert_eq!(id, "struct:src/foo.rs:1:Foo");
    }

    #[test]
    fn search_symbols_returns_all_kinds_when_query_matches() {
        let p = proj();
        let r = search_symbols(&p, "do_thing", 0.6, 0);
        assert!(r.symbols.iter().any(|s| s.name == "do_thing"));
        assert_eq!(r.query, "do_thing");
    }

    #[test]
    fn search_symbols_respects_limit() {
        let p = proj();
        let r = search_symbols(&p, "src/", 0.0, 2);
        assert!(r.symbols.len() <= 2);
    }

    #[test]
    fn search_symbols_zero_limit_disables_truncation() {
        let p = proj();
        let r = search_symbols(&p, "src/", 0.0, 0);
        assert!(r.symbols.len() >= 2);
    }

    #[test]
    fn search_symbols_filters_by_threshold() {
        let p = proj();
        let r = search_symbols(&p, "completely_unrelated_xyz", 0.99, 0);
        assert!(r.symbols.is_empty());
    }

    #[test]
    fn search_symbols_struct_id_uses_struct_prefix() {
        let p = proj();
        let r = search_symbols(&p, "Foo", 0.6, 0);
        let foo = r.symbols.iter().find(|s| s.name == "Foo").expect("Foo");
        assert!(foo.symbol_id.starts_with("struct:"));
    }

    #[test]
    fn search_symbols_enum_id_uses_enum_prefix() {
        let p = proj();
        let r = search_symbols(&p, "MyEnum", 0.6, 0);
        let e = r
            .symbols
            .iter()
            .find(|s| s.name == "MyEnum")
            .expect("MyEnum");
        assert!(e.symbol_id.starts_with("enum:"));
    }
}
