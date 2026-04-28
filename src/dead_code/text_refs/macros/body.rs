use std::collections::HashMap;
use std::fs;

use super::super::parsing::clamp_to_char_boundary;

pub(super) fn extract_macro_body_from_file(
    file_path: &str,
    line: usize,
    column: usize,
    macro_base: &str,
    file_lines_cache: &mut HashMap<String, Vec<String>>,
) -> Option<String> {
    if !file_lines_cache.contains_key(file_path) {
        let content = fs::read_to_string(file_path).ok()?;
        let lines = content.lines().map(|l| l.to_string()).collect::<Vec<_>>();
        file_lines_cache.insert(file_path.to_string(), lines);
    }
    let lines = file_lines_cache.get(file_path)?;
    if lines.is_empty() {
        return None;
    }

    let marker = format!("{}!", macro_base);
    let start_line = line.saturating_sub(1).min(lines.len().saturating_sub(1));
    let search_end = (start_line + 32).min(lines.len());
    let mut begin: Option<(usize, usize, char)> = None;

    for (idx, line_text_raw) in lines.iter().enumerate().take(search_end).skip(start_line) {
        let line_text = line_text_raw.split("//").next().unwrap_or("");
        let search_from = if idx == start_line {
            clamp_to_char_boundary(line_text, column)
        } else {
            0
        };
        let macro_start = if search_from < line_text.len() {
            if let Some(found_at) = line_text[search_from..].find(&marker) {
                search_from + found_at
            } else if let Some(found_at) = line_text.find(&marker) {
                found_at
            } else {
                continue;
            }
        } else if let Some(found_at) = line_text.find(&marker) {
            found_at
        } else {
            continue;
        };
        let macro_end = macro_start + marker.len();
        let mut iter = line_text[macro_end..].char_indices();
        let mut open: Option<(usize, char)> = None;
        for (off, ch) in iter.by_ref() {
            if ch.is_whitespace() {
                continue;
            }
            if matches!(ch, '(' | '[' | '{') {
                open = Some((macro_end + off, ch));
            }
            break;
        }
        if let Some((open_col, open_char)) = open {
            begin = Some((idx, open_col + open_char.len_utf8(), open_char));
            break;
        }
    }
    let (open_line, open_col, open_char) = begin?;
    let close_char = match open_char {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return None,
    };

    let parse_end = (open_line + 200).min(lines.len());
    let mut body = String::new();
    let mut depth = 1usize;
    let mut closed = false;
    for (idx, line_text_raw) in lines.iter().enumerate().take(parse_end).skip(open_line) {
        let line_text = line_text_raw.split("//").next().unwrap_or("");
        let segment = if idx == open_line {
            let start = clamp_to_char_boundary(line_text, open_col);
            if start >= line_text.len() {
                ""
            } else {
                &line_text[start..]
            }
        } else {
            line_text
        };
        for ch in segment.chars() {
            if ch == open_char {
                depth += 1;
                body.push(ch);
                continue;
            }
            if ch == close_char {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    closed = true;
                    break;
                }
                body.push(ch);
                continue;
            }
            body.push(ch);
        }
        if closed {
            break;
        }
        body.push('\n');
    }
    if !closed || body.trim().is_empty() {
        return None;
    }
    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, content: &str) -> std::path::PathBuf {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        std::fs::write(&path, content).expect("write");
        let leaked = dir.keep();
        leaked.join(name)
    }

    #[test]
    fn extract_macro_body_returns_balanced_paren_body() {
        let path = write_temp("a.rs", "fn x() { vec!(1, 2, 3); }\n");
        let mut cache = HashMap::new();
        let body = extract_macro_body_from_file(path.to_str().unwrap(), 1, 9, "vec", &mut cache);
        assert_eq!(body.as_deref().map(|s| s.trim()), Some("1, 2, 3"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_macro_body_returns_balanced_brace_body() {
        let path = write_temp("b.rs", "fn x() { vec!{1, 2, 3}; }\n");
        let mut cache = HashMap::new();
        let body = extract_macro_body_from_file(path.to_str().unwrap(), 1, 9, "vec", &mut cache);
        assert_eq!(body.as_deref().map(|s| s.trim()), Some("1, 2, 3"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_macro_body_returns_balanced_bracket_body() {
        let path = write_temp("c.rs", "fn x() { vec![1, 2, 3]; }\n");
        let mut cache = HashMap::new();
        let body = extract_macro_body_from_file(path.to_str().unwrap(), 1, 9, "vec", &mut cache);
        assert_eq!(body.as_deref().map(|s| s.trim()), Some("1, 2, 3"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_macro_body_handles_multiline_macros() {
        let path = write_temp(
            "d.rs",
            "fn x() {\n    vec![\n        1,\n        2,\n        3,\n    ];\n}\n",
        );
        let mut cache = HashMap::new();
        let body = extract_macro_body_from_file(path.to_str().unwrap(), 2, 4, "vec", &mut cache);
        let body_str = body.expect("body");
        assert!(body_str.contains('1'));
        assert!(body_str.contains('3'));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_macro_body_returns_none_for_missing_marker() {
        let path = write_temp("e.rs", "fn x() {}\n");
        let mut cache = HashMap::new();
        let body = extract_macro_body_from_file(path.to_str().unwrap(), 1, 0, "vec", &mut cache);
        assert!(body.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_macro_body_returns_none_for_empty_file() {
        let path = write_temp("empty.rs", "");
        let mut cache = HashMap::new();
        let body = extract_macro_body_from_file(path.to_str().unwrap(), 1, 0, "vec", &mut cache);
        assert!(body.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_macro_body_returns_none_for_missing_file() {
        let mut cache = HashMap::new();
        let body =
            extract_macro_body_from_file("/nonexistent/zzz.rs", 1, 0, "vec", &mut cache);
        assert!(body.is_none());
    }
}
