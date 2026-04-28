pub(super) fn macro_supports_identifier_refs(macro_base: &str) -> bool {
    matches!(macro_base, "entrypoint" | "entrypoint_deprecated")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_solana_entrypoint_macros() {
        assert!(macro_supports_identifier_refs("entrypoint"));
        assert!(macro_supports_identifier_refs("entrypoint_deprecated"));
    }

    #[test]
    fn rejects_other_macros() {
        assert!(!macro_supports_identifier_refs("vec"));
        assert!(!macro_supports_identifier_refs("println"));
    }
}
