use crate::index::{FunctionInfo, RelatedFunction};
use std::collections::{HashMap, HashSet, VecDeque};

/// Build a [`RelatedFunction`] from `func`, its stable `id`, and the
/// BFS `distance` from the query root.
pub fn related_from_function(func: &FunctionInfo, id: &str, distance: usize) -> RelatedFunction {
    RelatedFunction {
        id: id.to_string(),
        name: func.name.clone(),
        file_path: func.file_path.clone(),
        start_line: func.start_line,
        end_line: func.end_line,
        signature: func.signature.clone(),
        distance,
    }
}

/// Sort a slice of related functions by distance, then file path, then start
/// line, then name.
pub fn sort_related(items: &mut [RelatedFunction]) {
    items.sort_by(|a, b| {
        a.distance
            .cmp(&b.distance)
            .then(a.file_path.cmp(&b.file_path))
            .then(a.start_line.cmp(&b.start_line))
            .then(a.name.cmp(&b.name))
    });
}

/// Sort and deduplicate every value `Vec` in `map` in-place.
pub fn dedup_map_values(map: &mut HashMap<String, Vec<String>>) {
    for values in map.values_mut() {
        values.sort();
        values.dedup();
    }
}

/// BFS from `start_id` through `adjacency` up to `call_depth` hops, collecting
/// [`RelatedFunction`] entries from `function_by_id`.
///
/// Returns `(total_found, was_truncated, results)`.  A `call_depth` of 0
/// returns immediately with an empty result.  `max_related` of 0 means
/// unlimited.
pub fn collect_related_functions(
    start_id: &str,
    adjacency: &HashMap<String, Vec<String>>,
    function_by_id: &HashMap<String, &FunctionInfo>,
    call_depth: usize,
    max_related: usize,
) -> (usize, bool, Vec<RelatedFunction>) {
    if call_depth == 0 {
        return (0, false, Vec::new());
    }

    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    if let Some(neighbors) = adjacency.get(start_id) {
        for id in neighbors {
            queue.push_back((id.clone(), 1));
        }
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut related = Vec::new();

    while let Some((id, distance)) = queue.pop_front() {
        if !visited.insert(id.clone()) {
            continue;
        }

        if let Some(func) = function_by_id.get(&id) {
            related.push(related_from_function(func, &id, distance));
        }

        if distance >= call_depth {
            continue;
        }

        if let Some(next_ids) = adjacency.get(&id) {
            for next in next_ids {
                if !visited.contains(next) {
                    queue.push_back((next.clone(), distance + 1));
                }
            }
        }
    }

    sort_related(&mut related);
    let total = related.len();
    let truncated = max_related != 0 && total > max_related;
    if truncated {
        related.truncate(max_related);
    }

    (total, truncated, related)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::FunctionInfo;

    fn func(name: &str, file_path: &str, start_line: usize) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: format!("fn {name}"),
            file_path: file_path.to_string(),
            start_line,
            end_line: start_line + 2,
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
        }
    }

    #[test]
    fn related_from_function_copies_fields() {
        let f = func("foo", "src/lib.rs", 10);
        let related = related_from_function(&f, "src/lib.rs:10:foo", 2);
        assert_eq!(related.id, "src/lib.rs:10:foo");
        assert_eq!(related.distance, 2);
        assert_eq!(related.start_line, 10);
        assert_eq!(related.end_line, 12);
        assert_eq!(related.signature, "fn foo");
    }

    #[test]
    fn sort_related_orders_by_distance_then_path_then_line_then_name() {
        let a = related_from_function(&func("z_func", "src/a.rs", 5), "id1", 2);
        let b = related_from_function(&func("a_func", "src/a.rs", 5), "id2", 2);
        let c = related_from_function(&func("y_func", "src/a.rs", 5), "id3", 1);
        let mut items = vec![a, b, c];
        sort_related(&mut items);
        assert_eq!(items[0].name, "y_func");
        assert_eq!(items[1].name, "a_func");
        assert_eq!(items[2].name, "z_func");
    }

    #[test]
    fn dedup_map_values_removes_duplicates_and_sorts() {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        map.insert("k".to_string(), vec!["b".into(), "a".into(), "a".into()]);
        dedup_map_values(&mut map);
        assert_eq!(map.get("k"), Some(&vec!["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn collect_related_functions_returns_empty_when_depth_zero() {
        let adjacency = HashMap::new();
        let by_id = HashMap::new();
        let (total, truncated, items) =
            collect_related_functions("src/a.rs:1:foo", &adjacency, &by_id, 0, 10);
        assert_eq!(total, 0);
        assert!(!truncated);
        assert!(items.is_empty());
    }

    #[test]
    fn collect_related_functions_truncates_when_max_related_exceeded() {
        let f1 = func("a", "src/a.rs", 1);
        let f2 = func("b", "src/b.rs", 2);
        let f3 = func("c", "src/c.rs", 3);

        let id1 = "src/a.rs:1:a".to_string();
        let id2 = "src/b.rs:2:b".to_string();
        let id3 = "src/c.rs:3:c".to_string();

        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
        adjacency.insert(
            "root".to_string(),
            vec![id1.clone(), id2.clone(), id3.clone()],
        );

        let mut by_id: HashMap<String, &FunctionInfo> = HashMap::new();
        by_id.insert(id1, &f1);
        by_id.insert(id2, &f2);
        by_id.insert(id3, &f3);

        let (total, truncated, items) = collect_related_functions("root", &adjacency, &by_id, 1, 2);
        assert_eq!(total, 3);
        assert!(truncated);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn collect_related_functions_traverses_two_hops() {
        let leaf = func("leaf", "src/c.rs", 1);
        let mid = func("mid", "src/b.rs", 1);
        let leaf_id = "src/c.rs:1:leaf".to_string();
        let mid_id = "src/b.rs:1:mid".to_string();

        let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
        adjacency.insert("root".to_string(), vec![mid_id.clone()]);
        adjacency.insert(mid_id.clone(), vec![leaf_id.clone()]);

        let mut by_id: HashMap<String, &FunctionInfo> = HashMap::new();
        by_id.insert(mid_id.clone(), &mid);
        by_id.insert(leaf_id.clone(), &leaf);

        let (total, _truncated, items) =
            collect_related_functions("root", &adjacency, &by_id, 2, 0);
        assert_eq!(total, 2);
        assert_eq!(items[0].distance, 1);
        assert_eq!(items[1].distance, 2);
    }
}
