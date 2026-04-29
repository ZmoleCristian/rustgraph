use std::collections::{HashMap, HashSet};

use crate::FunctionInfo;
use crate::project::ProjectData;

use super::super::super::model::{
    collect_cfg_test_ranges, collect_cfg_test_ranges_from_ast, line_in_ranges,
};
use super::super::entrypoint::{
    collect_runtime_entrypoint_lines, collect_runtime_entrypoint_lines_from_ast,
};

pub const COMMON_TRAIT_METHODS: &[&str] = &[
    "from",
    "into",
    "as_ref",
    "as_mut",
    "to_string",
    "fmt",
    "default",
    "clone",
    "debug",
    "display",
    "serialize",
    "deserialize",
    "deref",
    "deref_mut",
    "borrow",
    "borrow_mut",
    "to_sql",
    "from_sql",
];

pub enum SkipReason {
    None,
    Reexported,
    CfgTest,
    RuntimeEntrypoint,
    CommonTraitMethod,
    FrameworkHandler,
}

pub fn check_skip_reason(
    func: &FunctionInfo,
    reexported: &HashSet<String>,
    called_method_names: &HashSet<String>,
    framework_live_targets: &HashSet<(String, usize)>,
    test_ranges_cache: &mut HashMap<String, Vec<(usize, usize)>>,
    runtime_entrypoint_cache: &mut HashMap<String, HashSet<usize>>,
    project: Option<&ProjectData>,
) -> SkipReason {
    if framework_live_targets.contains(&(func.file_path.clone(), func.start_line)) {
        return SkipReason::FrameworkHandler;
    }

    if reexported.contains(&func.name) {
        return SkipReason::Reexported;
    }

    let func_test_ranges = test_ranges_cache
        .entry(func.file_path.clone())
        .or_insert_with(|| match project.and_then(|p| p.parsed_file_by_str(&func.file_path)) {
            Some(syntax) => collect_cfg_test_ranges_from_ast(syntax),
            None => collect_cfg_test_ranges(&func.file_path),
        });
    if line_in_ranges(func.start_line, func_test_ranges) {
        return SkipReason::CfgTest;
    }

    let runtime_lines = runtime_entrypoint_cache
        .entry(func.file_path.clone())
        .or_insert_with(|| match project.and_then(|p| p.parsed_file_by_str(&func.file_path)) {
            Some(syntax) => collect_runtime_entrypoint_lines_from_ast(syntax),
            None => collect_runtime_entrypoint_lines(&func.file_path),
        });
    if runtime_lines.contains(&func.start_line) {
        return SkipReason::RuntimeEntrypoint;
    }

    if func.kind.starts_with("method(") && COMMON_TRAIT_METHODS.contains(&func.name.as_str()) {
        let method_is_called = called_method_names.contains(&func.name);
        let corresponding_method = match func.name.as_str() {
            "from" => Some("into"),
            "into" => Some("from"),
            "as_ref" => Some("as_ref"),
            "as_mut" => Some("as_mut"),
            _ => None,
        };
        let corresponding_is_called = corresponding_method
            .and_then(|m| {
                if called_method_names.contains(m) {
                    Some(true)
                } else {
                    None
                }
            })
            .unwrap_or(false);

        if method_is_called || corresponding_is_called {
            return SkipReason::CommonTraitMethod;
        }
    }

    SkipReason::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp_rust(content: &str) -> NamedTempFile {
        let mut tmp = tempfile::Builder::new()
            .suffix(".rs")
            .tempfile()
            .expect("create tempfile");
        tmp.write_all(content.as_bytes()).unwrap();
        tmp.flush().unwrap();
        tmp
    }

    fn make_function(file: &str, line: usize, name: &str, kind: &str) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}()"),
            file_path: file.to_string(),
            start_line: line,
            end_line: line + 1,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: Vec::new(),
            parameters: Vec::new(),
            return_type: None,
            kind: kind.to_string(),
            cfg_attrs: Vec::new(),
        }
    }

    #[test]
    fn check_skip_reason_recognizes_framework_handler() {
        let file = write_temp_rust("fn _x(){}\n");
        let path = file.path().to_string_lossy().to_string();
        let func = make_function(&path, 5, "handler", "function");
        let mut framework_targets = HashSet::new();
        framework_targets.insert((path.clone(), 5));

        let mut test_cache = HashMap::new();
        let mut runtime_cache = HashMap::new();
        let reason = check_skip_reason(
            &func,
            &HashSet::new(),
            &HashSet::new(),
            &framework_targets,
            &mut test_cache,
            &mut runtime_cache,
            None,
        );
        assert!(matches!(reason, SkipReason::FrameworkHandler));
    }

    #[test]
    fn check_skip_reason_recognizes_reexported_name() {
        let file = write_temp_rust("fn _x(){}\n");
        let path = file.path().to_string_lossy().to_string();
        let func = make_function(&path, 1, "exported_api", "function");

        let mut reexported = HashSet::new();
        reexported.insert("exported_api".to_string());
        let mut test_cache = HashMap::new();
        let mut runtime_cache = HashMap::new();
        let reason = check_skip_reason(
            &func,
            &reexported,
            &HashSet::new(),
            &HashSet::new(),
            &mut test_cache,
            &mut runtime_cache,
            None,
        );
        assert!(matches!(reason, SkipReason::Reexported));
    }

    #[test]
    fn check_skip_reason_recognizes_cfg_test_function() {
        let file = write_temp_rust("#[cfg(test)]\npub fn helper() {}\n");
        let path = file.path().to_string_lossy().to_string();
        let func = make_function(&path, 1, "helper", "function");

        let mut test_cache = HashMap::new();
        let mut runtime_cache = HashMap::new();
        let reason = check_skip_reason(
            &func,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &mut test_cache,
            &mut runtime_cache,
            None,
        );
        assert!(matches!(reason, SkipReason::CfgTest));
    }

    #[test]
    fn check_skip_reason_recognizes_runtime_entrypoint() {
        let file = write_temp_rust("#[no_mangle]\npub fn boot() {}\n");
        let path = file.path().to_string_lossy().to_string();
        let func = make_function(&path, 1, "boot", "function");

        let mut test_cache = HashMap::new();
        let mut runtime_cache = HashMap::new();
        let reason = check_skip_reason(
            &func,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &mut test_cache,
            &mut runtime_cache,
            None,
        );
        assert!(matches!(reason, SkipReason::RuntimeEntrypoint));
    }

    #[test]
    fn check_skip_reason_recognizes_common_trait_method_called_via_corresponding() {
        let file = write_temp_rust("fn _x(){}\n");
        let path = file.path().to_string_lossy().to_string();
        let func = make_function(&path, 5, "from", "method(MyType)");

        let mut called = HashSet::new();
        called.insert("into".to_string());
        let mut test_cache = HashMap::new();
        let mut runtime_cache = HashMap::new();
        let reason = check_skip_reason(
            &func,
            &HashSet::new(),
            &called,
            &HashSet::new(),
            &mut test_cache,
            &mut runtime_cache,
            None,
        );
        assert!(matches!(reason, SkipReason::CommonTraitMethod));
    }

    #[test]
    fn check_skip_reason_returns_none_when_nothing_matches() {
        let file = write_temp_rust("pub fn ordinary() {}\n");
        let path = file.path().to_string_lossy().to_string();
        let func = make_function(&path, 1, "ordinary", "function");

        let mut test_cache = HashMap::new();
        let mut runtime_cache = HashMap::new();
        let reason = check_skip_reason(
            &func,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &mut test_cache,
            &mut runtime_cache,
            None,
        );
        assert!(matches!(reason, SkipReason::None));
    }

    #[test]
    fn common_trait_methods_contains_known_methods() {
        for m in ["from", "into", "as_ref", "to_string", "default"] {
            assert!(COMMON_TRAIT_METHODS.contains(&m), "missing trait method {m}");
        }
    }
}
