use crate::index::{FunctionInfo, LifecycleHop};

/// Build a [`LifecycleHop`] from the minimal fields of `func` and its stable `id`.
pub fn lifecycle_hop_from_function(func: &FunctionInfo, id: &str) -> LifecycleHop {
    LifecycleHop {
        id: id.to_string(),
        name: func.name.clone(),
        file_path: func.file_path.clone(),
        start_line: func.start_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_hop_copies_minimal_fields_from_function() {
        let f = FunctionInfo {
            name: "step".to_string(),
            signature: "fn step".to_string(),
            file_path: "src/a.rs".to_string(),
            start_line: 7,
            end_line: 11,
            is_async: false,
            is_unsafe: false,
            is_pub: true,
            is_const: false,
            is_test: false,
            generics: vec![],
            parameters: vec![],
            return_type: None,
            kind: "function".to_string(),
            cfg_attrs: Vec::new(),
        };
        let hop = lifecycle_hop_from_function(&f, "src/a.rs:7:step");
        assert_eq!(hop.id, "src/a.rs:7:step");
        assert_eq!(hop.name, "step");
        assert_eq!(hop.file_path, "src/a.rs");
        assert_eq!(hop.start_line, 7);
    }
}
