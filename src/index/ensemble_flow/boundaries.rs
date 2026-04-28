use super::super::{FunctionInfo, IoBoundarySignal};

/// Classify which I/O boundaries a function touches based on keyword heuristics
/// applied to its name, signature, and resolved outgoing callee names.
///
/// Returns one [`IoBoundarySignal`] per detected boundary category (http,
/// database, queue, file, network, cache), sorted by boundary name then
/// location.  Returns an empty vec for neutral/pure-logic functions.
pub(in crate::index) fn classify_io_boundaries_for_function(
    id: &str,
    func: &FunctionInfo,
    outgoing_callees: &[String],
) -> Vec<IoBoundarySignal> {
    let mut corpus = format!(
        "{} {}",
        func.name.to_lowercase(),
        func.signature.to_lowercase()
    );
    if !outgoing_callees.is_empty() {
        corpus.push(' ');
        corpus.push_str(&outgoing_callees.join(" ").to_lowercase());
    }

    let boundary_keywords: [(&str, &[&str]); 6] = [
        (
            "http",
            &[
                "http", "request", "response", "route", "handler", "endpoint", "axum", "actix",
                "rocket", "warp",
            ],
        ),
        (
            "database",
            &[
                "db",
                "database",
                "sql",
                "query",
                "insert",
                "update",
                "delete",
                "select",
                "transaction",
                "sqlx",
                "diesel",
                "postgres",
                "mysql",
                "sqlite",
            ],
        ),
        (
            "queue",
            &[
                "queue", "kafka", "rabbit", "nats", "sqs", "pubsub", "topic", "consumer",
                "producer",
            ],
        ),
        (
            "file",
            &["file", "fs", "path", "open", "read", "write", "serialize"],
        ),
        (
            "network",
            &["socket", "tcp", "udp", "grpc", "rpc", "websocket", "net"],
        ),
        ("cache", &["cache", "redis", "memcache"]),
    ];

    let source_keywords = [
        "read", "load", "fetch", "get", "receive", "consume", "pull", "input", "parse", "decode",
    ];
    let sink_keywords = [
        "write", "save", "insert", "update", "delete", "publish", "send", "emit", "store", "push",
        "output", "persist",
    ];

    let source_score = source_keywords
        .iter()
        .filter(|keyword| corpus.contains(**keyword))
        .count();
    let sink_score = sink_keywords
        .iter()
        .filter(|keyword| corpus.contains(**keyword))
        .count();
    let role = if source_score > sink_score {
        "source"
    } else if sink_score > source_score {
        "sink"
    } else {
        "transit"
    };

    let mut signals = Vec::new();
    for (boundary, keywords) in boundary_keywords {
        let mut evidence = keywords
            .iter()
            .filter(|keyword| corpus.contains(**keyword))
            .map(|keyword| keyword.to_string())
            .collect::<Vec<_>>();
        if evidence.is_empty() {
            continue;
        }
        evidence.sort();
        evidence.dedup();
        if evidence.len() > 6 {
            evidence.truncate(6);
        }
        let confidence = (0.45 + (evidence.len() as f64 * 0.1)).min(0.95);
        signals.push(IoBoundarySignal {
            boundary: boundary.to_string(),
            role: role.to_string(),
            confidence,
            function_id: id.to_string(),
            function_name: func.name.clone(),
            file_path: func.file_path.clone(),
            start_line: func.start_line,
            evidence,
        });
    }

    signals.sort_by(|left, right| {
        left.boundary
            .cmp(&right.boundary)
            .then(left.file_path.cmp(&right.file_path))
            .then(left.start_line.cmp(&right.start_line))
    });
    signals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn func(name: &str, sig: &str) -> FunctionInfo {
        FunctionInfo {
            name: name.to_string(),
            signature: sig.to_string(),
            file_path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 1,
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
    fn classify_io_boundaries_returns_empty_for_neutral_function() {
        let f = func("noop", "fn noop() {}");
        let signals = classify_io_boundaries_for_function("id", &f, &[]);
        assert!(signals.is_empty());
    }

    #[test]
    fn classify_io_boundaries_detects_http_boundary() {
        let f = func("handle_request", "fn handle_request() {}");
        let signals = classify_io_boundaries_for_function("id", &f, &["axum_handler".to_string()]);
        assert!(signals.iter().any(|s| s.boundary == "http"));
    }

    #[test]
    fn classify_io_boundaries_detects_database_boundary_and_role() {
        let f = func("update_user", "fn update_user() {}");
        let signals = classify_io_boundaries_for_function("id", &f, &["sql".to_string()]);
        let db = signals
            .iter()
            .find(|s| s.boundary == "database")
            .expect("expected database boundary");

        assert_eq!(db.role, "sink");
    }

    #[test]
    fn classify_io_boundaries_classifies_role_source_when_more_source_keywords() {
        let f = func("parse_input", "fn parse_input() {}");
        let signals =
            classify_io_boundaries_for_function("id", &f, &["read_data".to_string()]);
        if !signals.is_empty() {
            assert_eq!(signals[0].role, "source");
        }
    }

    #[test]
    fn classify_io_boundaries_returns_signals_sorted_by_boundary() {
        let f = func("save_kafka_payload", "fn save_kafka_payload() {}");
        let signals = classify_io_boundaries_for_function("id", &f, &[]);
        let names: Vec<_> = signals.iter().map(|s| s.boundary.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn classify_io_boundaries_caps_evidence_length() {

        let outgoing = vec![
            "http".to_string(),
            "request".to_string(),
            "response".to_string(),
            "route".to_string(),
            "handler".to_string(),
            "endpoint".to_string(),
            "axum".to_string(),
            "actix".to_string(),
        ];
        let f = func("everything", "fn everything() {}");
        let signals = classify_io_boundaries_for_function("id", &f, &outgoing);
        let http = signals
            .iter()
            .find(|s| s.boundary == "http")
            .expect("expected http boundary");
        assert!(http.evidence.len() <= 6);
        assert!(http.confidence <= 0.95);
    }
}
