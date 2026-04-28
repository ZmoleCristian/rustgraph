pub(super) fn split_macro_call_candidates(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars = body.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    while i < chars.len() {
        if !(chars[i].is_alphabetic() || chars[i] == '_') {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == ':')
        {
            i += 1;
        }
        let token = chars[start..i].iter().collect::<String>();
        if token.ends_with("::")
            || matches!(
                token.as_str(),
                "if" | "match" | "while" | "for" | "loop" | "return"
            )
        {
            continue;
        }
        let mut j = i;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j < chars.len() && chars[j] == '(' {
            let base = token.rsplit("::").next().unwrap_or("").trim().to_string();
            if !base.is_empty() {
                out.push((token.clone(), base));
            }
        }
    }
    out
}

pub(super) fn split_macro_identifier_candidates(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars = body.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    while i < chars.len() {
        if !(chars[i].is_alphabetic() || chars[i] == '_') {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == ':')
        {
            i += 1;
        }
        let token = chars[start..i].iter().collect::<String>();
        if token.ends_with("::") {
            continue;
        }
        let base = token.rsplit("::").next().unwrap_or("").trim().to_string();
        if base.is_empty() {
            continue;
        }
        let first = match base.chars().next() {
            Some(ch) => ch,
            None => continue,
        };
        if !first.is_alphabetic() && first != '_' {
            continue;
        }
        if matches!(
            base.as_str(),
            "self"
                | "Self"
                | "super"
                | "crate"
                | "if"
                | "match"
                | "while"
                | "for"
                | "loop"
                | "return"
                | "let"
                | "const"
                | "fn"
                | "pub"
                | "mod"
                | "impl"
                | "struct"
                | "enum"
                | "use"
                | "where"
                | "async"
                | "move"
        ) {
            continue;
        }
        out.push((token, base));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_macro_call_candidates_returns_function_calls() {
        let candidates = split_macro_call_candidates("foo(); bar(x, y);");
        let bases: Vec<_> = candidates.iter().map(|(_, b)| b.clone()).collect();
        assert!(bases.contains(&"foo".to_string()));
        assert!(bases.contains(&"bar".to_string()));
    }

    #[test]
    fn split_macro_call_candidates_strips_path_prefixes() {
        let candidates = split_macro_call_candidates("crate::api::handle(x);");
        let entry = candidates
            .iter()
            .find(|(full, _)| full == "crate::api::handle")
            .expect("entry");
        assert_eq!(entry.1, "handle");
    }

    #[test]
    fn split_macro_call_candidates_skips_keywords() {
        let candidates = split_macro_call_candidates("if (true) { for x in 0..10 {} }");
        let bases: Vec<_> = candidates.iter().map(|(_, b)| b.clone()).collect();
        assert!(!bases.iter().any(|b| matches!(b.as_str(), "if" | "for" | "while" | "loop" | "match" | "return")));
    }

    #[test]
    fn split_macro_call_candidates_returns_empty_for_no_calls() {
        let candidates = split_macro_call_candidates("let x = 1;");
        assert!(candidates.is_empty());
    }

    #[test]
    fn split_macro_identifier_candidates_returns_identifiers() {
        let candidates = split_macro_identifier_candidates("foo bar baz");
        let bases: Vec<_> = candidates.iter().map(|(_, b)| b.clone()).collect();
        assert_eq!(bases, vec!["foo".to_string(), "bar".to_string(), "baz".to_string()]);
    }

    #[test]
    fn split_macro_identifier_candidates_filters_keywords_and_self() {
        let candidates =
            split_macro_identifier_candidates("self super crate fn pub use foo if");
        let bases: Vec<_> = candidates.iter().map(|(_, b)| b.clone()).collect();
        assert_eq!(bases, vec!["foo".to_string()]);
    }

    #[test]
    fn split_macro_identifier_candidates_handles_path_prefix() {
        let candidates = split_macro_identifier_candidates("crate::api::process_inst");
        let entry = candidates
            .iter()
            .find(|(full, _)| full == "crate::api::process_inst")
            .expect("entry");
        assert_eq!(entry.1, "process_inst");
    }

    #[test]
    fn split_macro_identifier_candidates_skips_trailing_double_colon() {
        let candidates = split_macro_identifier_candidates("crate::");
        assert!(candidates.is_empty());
    }
}
