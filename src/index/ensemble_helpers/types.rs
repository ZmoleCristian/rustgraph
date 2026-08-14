use crate::index::FunctionInfo;

/// Return `true` when `token` appears as a complete identifier word in `text`
/// (i.e. not embedded inside a longer identifier).
pub fn contains_identifier_token(text: &str, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }

    let mut current = String::new();
    for ch in text.chars().chain(std::iter::once(' ')) {
        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
            continue;
        }

        if !current.is_empty() {
            if current == token {
                return true;
            }
            current.clear();
        }
    }

    false
}

/// Strip path prefix, reference sigils, angle brackets, and trailing
/// punctuation from a type name, returning just the bare identifier.
pub fn normalize_type_query(type_name: &str) -> String {
    let base = type_name
        .trim()
        .rsplit("::")
        .next()
        .unwrap_or(type_name)
        .trim();
    base.trim_matches(|c: char| c == '&' || c == '<' || c == '>' || c == ',' || c == ' ')
        .to_string()
}

/// Return `true` when `func` mentions `type_token` in any of its parameters,
/// return type, kind string, or full signature.
pub fn function_mentions_type(func: &FunctionInfo, type_token: &str) -> bool {
    func.parameters
        .iter()
        .any(|p| contains_identifier_token(p, type_token))
        || func
            .return_type
            .as_deref()
            .is_some_and(|ret| contains_identifier_token(ret, type_token))
        || contains_identifier_token(&func.kind, type_token)
        || contains_identifier_token(&func.signature, type_token)
}

/// Return `true` when `func`'s return type contains `type_token` as a whole
/// identifier, indicating the function produces values of that type.
pub fn function_produces_type(func: &FunctionInfo, type_token: &str) -> bool {
    func.return_type
        .as_deref()
        .is_some_and(|ret| contains_identifier_token(ret, type_token))
}

/// Return `true` when `func` accepts `type_token` as a parameter type or
/// declares it via its kind string (e.g. an `impl` method receiver).
pub fn function_consumes_type(func: &FunctionInfo, type_token: &str) -> bool {
    func.parameters
        .iter()
        .any(|p| contains_identifier_token(p, type_token))
        || contains_identifier_token(&func.kind, type_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_func(params: &[&str], ret: Option<&str>, kind: &str) -> FunctionInfo {
        FunctionInfo {
            name: "f".to_string(),
            signature: "fn f()".to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 1,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: Vec::new(),
            parameters: params.iter().map(|p| p.to_string()).collect(),
            return_type: ret.map(|r| r.to_string()),
            kind: kind.to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    #[test]
    fn contains_identifier_token_matches_whole_word() {
        assert!(contains_identifier_token("foo bar", "foo"));
        assert!(contains_identifier_token(
            "get_user(req: Request)",
            "Request"
        ));
        assert!(!contains_identifier_token("foo_bar", "foo"));
        assert!(!contains_identifier_token("foobar", "foo"));
    }

    #[test]
    fn contains_identifier_token_empty_returns_false() {
        assert!(!contains_identifier_token("text", ""));
    }

    #[test]
    fn contains_identifier_token_handles_punctuation_word_boundaries() {
        assert!(contains_identifier_token("Vec<Item>", "Item"));
        assert!(contains_identifier_token("&Item", "Item"));
        assert!(contains_identifier_token("(Item)", "Item"));
    }

    #[test]
    fn normalize_type_query_strips_punctuation_and_paths() {
        assert_eq!(normalize_type_query("crate::api::Foo"), "Foo");
        assert_eq!(normalize_type_query("&Item"), "Item");
        assert_eq!(normalize_type_query("<Item>"), "Item");
        assert_eq!(normalize_type_query("crate::Item,"), "Item");
        assert_eq!(normalize_type_query("  Item  "), "Item");
    }

    #[test]
    fn function_mentions_type_via_parameters() {
        let f = make_func(&["x: Item"], None, "function");
        assert!(function_mentions_type(&f, "Item"));
        assert!(!function_mentions_type(&f, "Other"));
    }

    #[test]
    fn function_mentions_type_via_return() {
        let f = make_func(&[], Some("Result<Item, Err>"), "function");
        assert!(function_mentions_type(&f, "Item"));
    }

    #[test]
    fn function_mentions_type_via_kind() {
        let f = make_func(&[], None, "method(MyContainer)");
        assert!(function_mentions_type(&f, "MyContainer"));
    }

    #[test]
    fn function_produces_type_only_for_return_value() {
        let f_ret = make_func(&[], Some("Item"), "function");
        let f_param = make_func(&["x: Item"], None, "function");
        assert!(function_produces_type(&f_ret, "Item"));
        assert!(!function_produces_type(&f_param, "Item"));
    }

    #[test]
    fn function_consumes_type_for_params_and_kind() {
        let f_param = make_func(&["x: Item"], None, "function");
        let f_ret = make_func(&[], Some("Item"), "function");
        let f_method = make_func(&[], None, "method(Item)");
        assert!(function_consumes_type(&f_param, "Item"));
        assert!(function_consumes_type(&f_method, "Item"));
        assert!(!function_consumes_type(&f_ret, "Item"));
    }
}
