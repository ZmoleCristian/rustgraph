//! Normalisation of call-argument expressions into a plain handler name.

/// Strip decorators from a call-argument expression and extract the handler name.
///
/// Removes leading `&`, `mut`, trailing `?`, `,`, and outer parentheses, then
/// rejects closures, blocks, and keyword pseudo-identifiers. Returns
/// `(qualified_path, base_name)` on success, or `None` when the expression
/// cannot be resolved to a plain function reference.
pub(crate) fn normalize_handler_expr(expr: &str) -> Option<(String, String)> {
    let mut token = expr.trim();
    if token.is_empty() {
        return None;
    }

    token = token.trim_start_matches('&').trim();
    token = token.trim_start_matches("mut ").trim();
    token = token.trim_end_matches('?').trim();
    token = token.trim_end_matches(',').trim();
    token = token.trim_matches(|c: char| c == '(' || c == ')');
    if token.is_empty() {
        return None;
    }
    if token.starts_with('|')
        || token.starts_with("move ")
        || token.starts_with("async ")
        || token.starts_with('{')
        || token.starts_with("||")
    {
        return None;
    }

    let ident = token
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
        .collect::<String>();
    if ident.is_empty() {
        return None;
    }

    let base = ident.rsplit("::").next()?.trim().to_string();
    if base.is_empty() {
        return None;
    }
    let first = base.chars().next()?;
    if !first.is_alphabetic() && first != '_' {
        return None;
    }
    if matches!(base.as_str(), "self" | "Self" | "super" | "crate") {
        return None;
    }

    Some((ident, base))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handler_expr_strips_leading_decorators() {
        assert_eq!(
            normalize_handler_expr("&handler"),
            Some(("handler".to_string(), "handler".to_string()))
        );
        assert_eq!(
            normalize_handler_expr("&mut handler"),
            Some(("handler".to_string(), "handler".to_string()))
        );
    }

    #[test]
    fn normalize_handler_expr_strips_trailing_decorators() {
        assert_eq!(
            normalize_handler_expr("handler?"),
            Some(("handler".to_string(), "handler".to_string()))
        );
        assert_eq!(
            normalize_handler_expr("handler,"),
            Some(("handler".to_string(), "handler".to_string()))
        );
    }

    #[test]
    fn normalize_handler_expr_strips_outer_parens() {
        assert_eq!(
            normalize_handler_expr("(handler)"),
            Some(("handler".to_string(), "handler".to_string()))
        );
    }

    #[test]
    fn normalize_handler_expr_returns_qualified_path() {
        assert_eq!(
            normalize_handler_expr("crate::api::run"),
            Some(("crate::api::run".to_string(), "run".to_string()))
        );
    }

    #[test]
    fn normalize_handler_expr_rejects_empty_input() {
        assert!(normalize_handler_expr("").is_none());
        assert!(normalize_handler_expr("   ").is_none());
    }

    #[test]
    fn normalize_handler_expr_rejects_async_block() {
        assert!(normalize_handler_expr("async move { handle() }").is_none());
        assert!(normalize_handler_expr("async { handle() }").is_none());
    }

    #[test]
    fn normalize_handler_expr_rejects_closures() {
        assert!(normalize_handler_expr("|x| handle(x)").is_none());
        assert!(normalize_handler_expr("|| handle()").is_none());
        assert!(normalize_handler_expr("move |x| x").is_none());
    }

    #[test]
    fn normalize_handler_expr_rejects_self_super_keywords() {
        assert!(normalize_handler_expr("self").is_none());
        assert!(normalize_handler_expr("Self").is_none());
        assert!(normalize_handler_expr("super").is_none());
        assert!(normalize_handler_expr("crate").is_none());
    }

    #[test]
    fn normalize_handler_expr_rejects_starting_digit() {
        assert!(normalize_handler_expr("1handler").is_none());
    }

    #[test]
    fn normalize_handler_expr_strips_block_start() {
        assert!(normalize_handler_expr("{ handler }").is_none());
    }

    #[test]
    fn normalize_handler_expr_keeps_underscored_names() {
        assert_eq!(
            normalize_handler_expr("_handler"),
            Some(("_handler".to_string(), "_handler".to_string()))
        );
    }
}
