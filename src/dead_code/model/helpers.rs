//! Path and name classification helpers for filtering test and generated items.

/// Return `true` if `path` belongs to a test or bench module.
///
/// Matches paths containing `/tests/`, ending in `tests.rs` / `test_utils.rs`,
/// containing `/test_utils/`, or containing `/benches/`.
/// Windows path separators are normalised before matching.
pub(crate) fn is_test_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.contains("/tests/")
        || normalized.ends_with("tests.rs")
        || normalized.ends_with("test_utils.rs")
        || normalized.contains("/test_utils/")
        || normalized.contains("/benches/")
}

/// Return `true` if `path` points to a machine-generated source file.
///
/// Matches paths containing `generated.rs` or `/flatbuffers/`.
pub(crate) fn is_generated_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.contains("generated.rs") || normalized.contains("/flatbuffers/")
}

/// Return `true` if `name` follows a convention for test-only helpers.
///
/// Matches names prefixed with `test_` or suffixed with `_for_test`,
/// `_for_tests`, or `_for_tests_only`.
pub(crate) fn is_test_helper_name(name: &str) -> bool {
    name.starts_with("test_")
        || name.ends_with("_for_test")
        || name.ends_with("_for_tests")
        || name.ends_with("_for_tests_only")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_test_path_recognises_canonical_layouts() {
        assert!(is_test_path("/proj/tests/foo.rs"));
        assert!(is_test_path("/proj/src/foo/tests.rs"));
        assert!(is_test_path("/proj/src/test_utils.rs"));
        assert!(is_test_path("/proj/src/test_utils/api.rs"));
        assert!(is_test_path("/proj/benches/bench_a.rs"));
    }

    #[test]
    fn is_test_path_handles_windows_separators() {
        assert!(is_test_path(r"C:\proj\tests\foo.rs"));
    }

    #[test]
    fn is_test_path_returns_false_for_normal_files() {
        assert!(!is_test_path("/proj/src/lib.rs"));
        assert!(!is_test_path("/proj/src/api/users.rs"));
    }

    #[test]
    fn is_generated_path_recognises_generated_files() {
        assert!(is_generated_path("/proj/src/protocol_generated.rs"));
        assert!(is_generated_path("/proj/src/flatbuffers/foo.rs"));
    }

    #[test]
    fn is_generated_path_returns_false_for_normal_files() {
        assert!(!is_generated_path("/proj/src/lib.rs"));
    }

    #[test]
    fn is_test_helper_name_matches_known_suffixes_and_prefix() {
        assert!(is_test_helper_name("test_helper"));
        assert!(is_test_helper_name("setup_for_test"));
        assert!(is_test_helper_name("setup_for_tests"));
        assert!(is_test_helper_name("seed_for_tests_only"));
    }

    #[test]
    fn is_test_helper_name_returns_false_for_unrelated_names() {
        assert!(!is_test_helper_name("compute"));
        assert!(!is_test_helper_name("attest"));
        assert!(!is_test_helper_name("rotate_test_keys"));
    }
}
