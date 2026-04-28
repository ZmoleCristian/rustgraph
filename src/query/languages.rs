//! Language detection and project-scope assessment for a directory tree.
//!
//! Walks the filesystem (respecting `.gitignore`) to count code lines per
//! language and to decide whether a directory is safe to auto-index.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use ignore::WalkBuilder;
use tracing::{debug, info};

use std::path::PathBuf;

use crate::api::ProjectLanguageStat;

/// Language composition of a directory, produced by [`language_profile`] or
/// [`quick_language_profile`].
#[derive(Debug, Clone)]
pub struct LanguageProfile {
    /// The dominant language, or `None` if no recognisable source files were found.
    pub primary_language: Option<String>,
    /// Per-language statistics sorted by relevance (Rust first when a Cargo.toml is present).
    pub languages: Vec<ProjectLanguageStat>,
    /// `true` when the primary language is Rust and the project can be indexed.
    pub supported: bool,
}

/// Result of probing a directory to determine whether auto-indexing is safe.
#[derive(Debug, Clone)]
pub struct ProjectScopeAssessment {
    /// Whether the directory is within scope for automatic indexing.
    pub allow_auto_index: bool,
    /// Human-readable explanation of the decision.
    pub reason: &'static str,
    /// Total filesystem entries visited during the probe.
    pub entries_scanned: usize,
    /// Directory entries among those visited.
    pub directories_scanned: usize,
    /// Regular file entries among those visited.
    pub files_scanned: usize,
    /// `true` when the probe reached the entry cap before finishing.
    pub hit_entry_cap: bool,
    /// `true` when a recognised project marker (e.g. `Cargo.toml`) was found.
    pub has_project_marker: bool,
}

/// Return the final path component of `path` as a display string.
pub fn display_name_for_path(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .to_string()
}

/// Probe `path` to decide whether it is a bounded project suitable for auto-indexing.
///
/// Returns immediately with `allow_auto_index = true` if a project marker is
/// found. Otherwise scans up to 2500 entries at most 3 levels deep and rejects
/// shallow roots that look too broad (many files or directories).
pub fn assess_project_scope(path: &Path) -> ProjectScopeAssessment {
    const PROBE_MAX_DEPTH: usize = 3;
    const PROBE_ENTRY_CAP: usize = 2500;
    const BROAD_ENTRY_THRESHOLD: usize = 1800;
    const BROAD_DIR_THRESHOLD: usize = 180;

    if has_project_marker(path) {
        return ProjectScopeAssessment {
            allow_auto_index: true,
            reason: "project marker present",
            entries_scanned: 0,
            directories_scanned: 0,
            files_scanned: 0,
            hit_entry_cap: false,
            has_project_marker: true,
        };
    }

    if is_hidden_top_level_home_dir(path) {
        return ProjectScopeAssessment {
            allow_auto_index: false,
            reason: "top-level hidden home directory without project marker",
            entries_scanned: 0,
            directories_scanned: 0,
            files_scanned: 0,
            hit_entry_cap: false,
            has_project_marker: false,
        };
    }

    let mut entries_scanned = 0usize;
    let mut directories_scanned = 0usize;
    let mut files_scanned = 0usize;
    let mut builder = WalkBuilder::new(path);
    builder.hidden(false);
    builder.git_ignore(true);
    builder.git_exclude(true);
    builder.parents(true);
    builder.max_depth(Some(PROBE_MAX_DEPTH));

    for entry in builder.build().flatten() {
        if entry.depth() == 0 {
            continue;
        }
        entries_scanned += 1;
        if entry.path().is_dir() {
            directories_scanned += 1;
        } else if entry.path().is_file() {
            files_scanned += 1;
        }
        if entries_scanned >= PROBE_ENTRY_CAP {
            break;
        }
    }

    let depth = path.components().count();
    let hit_entry_cap = entries_scanned >= PROBE_ENTRY_CAP;
    let shallow_root = depth <= 3;
    let too_broad = hit_entry_cap
        || entries_scanned >= BROAD_ENTRY_THRESHOLD
        || directories_scanned >= BROAD_DIR_THRESHOLD;
    let allow_auto_index = !(shallow_root && too_broad);
    let reason = if allow_auto_index {
        "scope probe accepted"
    } else if hit_entry_cap {
        "scope probe hit entry cap on shallow unmarked root"
    } else {
        "scope probe marked shallow unmarked root as too broad"
    };

    ProjectScopeAssessment {
        allow_auto_index,
        reason,
        entries_scanned,
        directories_scanned,
        files_scanned,
        hit_entry_cap,
        has_project_marker: false,
    }
}

fn is_hidden_top_level_home_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    if !name.starts_with('.') || name == "." || name == ".." {
        return false;
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    path.parent().is_some_and(|parent| parent == home)
}

/// Walk `path` fully and count non-comment code lines per language.
///
/// More accurate than [`quick_language_profile`] but reads every file.
/// Rust is promoted to primary language when a `Cargo.toml` is present.
pub fn language_profile(path: &Path) -> LanguageProfile {
    let started = Instant::now();
    let mut counts = BTreeMap::<String, (usize, usize)>::new();
    let mut visited_files = 0usize;
    let mut candidate_files = 0usize;
    let mut unreadable_files = 0usize;
    let mut zero_code_files = 0usize;
    let mut builder = WalkBuilder::new(path);
    builder.hidden(false);
    builder.git_ignore(true);
    builder.git_exclude(true);
    builder.parents(true);

    for entry in builder.build().flatten() {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }
        visited_files += 1;
        let Some(language) = language_for_path(entry_path) else {
            continue;
        };
        candidate_files += 1;
        let Ok(content) = fs::read_to_string(entry_path) else {
            unreadable_files += 1;
            debug!(
                target: "query.languages",
                path = %path.display(),
                file = %entry_path.display(),
                language,
                "skipping unreadable language candidate"
            );
            continue;
        };
        let code_lines = count_code_lines(&content, language);
        if code_lines == 0 {
            zero_code_files += 1;
            continue;
        }
        let entry = counts.entry(language.to_string()).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += code_lines;
    }

    let mut languages = counts
        .into_iter()
        .map(|(language, (files, code_lines))| ProjectLanguageStat {
            language,
            files,
            code_lines,
        })
        .collect::<Vec<_>>();
    let has_rust_crate = has_cargo_toml_within(path, 1);
    languages.sort_by(|left, right| {
        let left_rust_priority = has_rust_crate && left.language == "rust";
        let right_rust_priority = has_rust_crate && right.language == "rust";
        let left_manifest = is_manifest_language(&left.language);
        let right_manifest = is_manifest_language(&right.language);
        right_rust_priority
            .cmp(&left_rust_priority)
            .then(left_manifest.cmp(&right_manifest))
            .then(right.code_lines.cmp(&left.code_lines))
            .then(right.files.cmp(&left.files))
            .then(left.language.cmp(&right.language))
    });

    let primary_language = languages.first().map(|item| item.language.clone());
    let supported = primary_language.as_deref() == Some("rust");
    info!(
        target: "query.languages",
        path = %path.display(),
        visited_files,
        candidate_files,
        unreadable_files,
        zero_code_files,
        supported,
        primary_language = primary_language.as_deref().unwrap_or("unknown"),
        languages = languages.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "built full language profile"
    );
    LanguageProfile {
        primary_language,
        languages,
        supported,
    }
}

/// Walk `path` up to 4 levels deep, detecting which languages are present
/// without counting lines.
///
/// Faster than [`language_profile`] because it stops after seeing the first
/// file of each language. Line counts in the returned stats are always 0.
pub fn quick_language_profile(path: &Path) -> LanguageProfile {
    let started = Instant::now();
    let mut languages = Vec::new();
    let mut seen = BTreeMap::<String, bool>::new();
    let mut visited_files = 0usize;
    let mut candidate_files = 0usize;
    let mut builder = WalkBuilder::new(path);
    builder.hidden(false);
    builder.git_ignore(true);
    builder.git_exclude(true);
    builder.parents(true);
    builder.max_depth(Some(4));

    for entry in builder.build().flatten() {
        let entry_path = entry.path();
        if !entry_path.is_file() {
            continue;
        }
        visited_files += 1;
        let Some(language) = language_for_path(entry_path) else {
            continue;
        };
        candidate_files += 1;
        if seen.contains_key(language) {
            continue;
        }
        seen.insert(language.to_string(), true);
        languages.push(ProjectLanguageStat {
            language: language.to_string(),
            files: 1,
            code_lines: 0,
        });
    }

    languages.sort_by(|left, right| left.language.cmp(&right.language));
    let primary_language = languages
        .iter()
        .find(|item| item.language == "rust")
        .map(|item| item.language.clone())
        .or_else(|| languages.first().map(|item| item.language.clone()));
    info!(
        target: "query.languages",
        path = %path.display(),
        visited_files,
        candidate_files,
        supported = !seen.is_empty(),
        primary_language = primary_language.as_deref().unwrap_or("unknown"),
        languages = languages.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "built quick language profile"
    );

    LanguageProfile {
        primary_language,
        languages,
        supported: !seen.is_empty(),
    }
}

fn is_manifest_language(language: &str) -> bool {
    matches!(
        language,
        "toml" | "yaml" | "json" | "dockerfile" | "markdown" | "html" | "css"
    )
}

fn has_cargo_toml_within(root: &Path, max_depth: usize) -> bool {
    if root.join("Cargo.toml").exists() {
        return true;
    }
    if max_depth == 0 {
        return false;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && has_cargo_toml_within(&path, max_depth - 1) {
            return true;
        }
    }
    false
}

fn count_code_lines(content: &str, language: &str) -> usize {
    let mut count = 0;
    let mut in_multi_line_comment = false;

    for line in content.lines() {
        let mut trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match language {
            "rust" | "javascript" | "typescript" | "tsx" | "jsx" | "css" => {
                loop {
                    if in_multi_line_comment {
                        if let Some(pos) = trimmed.find("*/") {
                            in_multi_line_comment = false;
                            trimmed = trimmed[pos + 2..].trim();
                            if trimmed.is_empty() {
                                break;
                            }
                            continue;
                        } else {
                            trimmed = "";
                            break;
                        }
                    }

                    if trimmed.starts_with("//") {
                        trimmed = "";
                        break;
                    }

                    if trimmed.starts_with("/*") {
                        if let Some(pos) = trimmed.find("*/") {
                            trimmed = trimmed[pos + 2..].trim();
                            if trimmed.is_empty() {
                                break;
                            }
                            continue;
                        } else {
                            in_multi_line_comment = true;
                            trimmed = "";
                            break;
                        }
                    }


                    break;
                }

                if !trimmed.is_empty() {
                    count += 1;
                }
            }
            "python" | "shell" | "yaml" | "toml" | "dockerfile" => {
                if !trimmed.starts_with('#') {
                    count += 1;
                }
            }
            "markdown" | "html" => {
                loop {
                    if in_multi_line_comment {
                        if let Some(pos) = trimmed.find("-->") {
                            in_multi_line_comment = false;
                            trimmed = trimmed[pos + 3..].trim();
                            if trimmed.is_empty() {
                                break;
                            }
                            continue;
                        } else {
                            trimmed = "";
                            break;
                        }
                    }

                    if trimmed.starts_with("<!--") {
                        if let Some(pos) = trimmed.find("-->") {
                            trimmed = trimmed[pos + 3..].trim();
                            if trimmed.is_empty() {
                                break;
                            }
                            continue;
                        } else {
                            in_multi_line_comment = true;
                            trimmed = "";
                            break;
                        }
                    }
                    break;
                }

                if !trimmed.is_empty() {
                    count += 1;
                }
            }
            _ => {
                count += 1;
            }
        }
    }

    count
}

fn has_project_marker(path: &Path) -> bool {
    const MARKERS: &[&str] = &[
        ".git",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "pom.xml",
        "Gemfile",
        "deno.json",
        "deno.jsonc",
        "composer.json",
        "build.gradle",
        "build.gradle.kts",
    ];

    MARKERS.iter().any(|marker| path.join(marker).exists())
}

fn language_for_path(path: &Path) -> Option<&'static str> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    match path.extension().and_then(|value| value.to_str()) {
        Some("rs") => Some("rust"),
        Some("toml") => Some("toml"),
        Some("md") => Some("markdown"),
        Some("json") | Some("jsonl") => Some("json"),
        Some("yaml") | Some("yml") => Some("yaml"),
        Some("sh") | Some("bash") => Some("shell"),
        Some("py") => Some("python"),
        Some("js") => Some("javascript"),
        Some("ts") => Some("typescript"),
        Some("tsx") => Some("tsx"),
        Some("jsx") => Some("jsx"),
        Some("html") => Some("html"),
        Some("css") => Some("css"),
        _ if file_name == "Dockerfile" => Some("dockerfile"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_for_path_uses_file_name() {
        let p = std::path::Path::new("/foo/bar/baz");
        assert_eq!(display_name_for_path(p), "baz");
    }

    #[test]
    fn display_name_for_path_handles_root() {
        let p = std::path::Path::new("/");

        let display = display_name_for_path(p);

        assert!(!display.is_empty());
    }

    #[test]
    fn language_for_path_recognises_common_extensions() {
        assert_eq!(language_for_path(Path::new("a.rs")), Some("rust"));
        assert_eq!(language_for_path(Path::new("a.toml")), Some("toml"));
        assert_eq!(language_for_path(Path::new("a.md")), Some("markdown"));
        assert_eq!(language_for_path(Path::new("a.json")), Some("json"));
        assert_eq!(language_for_path(Path::new("a.jsonl")), Some("json"));
        assert_eq!(language_for_path(Path::new("a.yaml")), Some("yaml"));
        assert_eq!(language_for_path(Path::new("a.yml")), Some("yaml"));
        assert_eq!(language_for_path(Path::new("a.sh")), Some("shell"));
        assert_eq!(language_for_path(Path::new("a.bash")), Some("shell"));
        assert_eq!(language_for_path(Path::new("a.py")), Some("python"));
        assert_eq!(language_for_path(Path::new("a.js")), Some("javascript"));
        assert_eq!(language_for_path(Path::new("a.ts")), Some("typescript"));
        assert_eq!(language_for_path(Path::new("a.tsx")), Some("tsx"));
        assert_eq!(language_for_path(Path::new("a.jsx")), Some("jsx"));
        assert_eq!(language_for_path(Path::new("a.html")), Some("html"));
        assert_eq!(language_for_path(Path::new("a.css")), Some("css"));
    }

    #[test]
    fn language_for_path_recognises_dockerfile_by_name() {
        assert_eq!(language_for_path(Path::new("Dockerfile")), Some("dockerfile"));
    }

    #[test]
    fn language_for_path_returns_none_for_unknown_extensions() {
        assert!(language_for_path(Path::new("a.unknown")).is_none());
        assert!(language_for_path(Path::new("README")).is_none());
    }

    #[test]
    fn count_code_lines_skips_blank_and_line_comments_for_rust() {
        let content = "fn foo() {\n    // a comment\n\n    let x = 1;\n}\n";

        assert_eq!(count_code_lines(content, "rust"), 3);
    }

    #[test]
    fn count_code_lines_handles_block_comments_for_rust() {
        let content = "/* a\n   b */\nfn foo() {}\n";
        assert_eq!(count_code_lines(content, "rust"), 1);
    }

    #[test]
    fn count_code_lines_skips_hash_comments_for_python() {
        let content = "# header\nprint('hi')\n# trailing\n";
        assert_eq!(count_code_lines(content, "python"), 1);
    }

    #[test]
    fn count_code_lines_handles_html_comments_for_markdown() {
        let content = "<!-- comment -->\n# title\n<!-- multi\nline -->\nbody\n";
        assert_eq!(count_code_lines(content, "markdown"), 2);
    }

    #[test]
    fn count_code_lines_unknown_language_counts_all_nonblank_lines() {
        assert_eq!(count_code_lines("a\n\nb\n", "weirdlang"), 2);
    }

    #[test]
    fn is_manifest_language_recognises_known_manifests() {
        assert!(is_manifest_language("toml"));
        assert!(is_manifest_language("yaml"));
        assert!(is_manifest_language("json"));
        assert!(is_manifest_language("dockerfile"));
        assert!(is_manifest_language("markdown"));
        assert!(is_manifest_language("html"));
        assert!(is_manifest_language("css"));
        assert!(!is_manifest_language("rust"));
        assert!(!is_manifest_language("python"));
    }

    #[test]
    fn has_project_marker_detects_cargo_toml_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").expect("write");
        assert!(has_project_marker(dir.path()));
    }

    #[test]
    fn has_project_marker_returns_false_for_empty_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!has_project_marker(dir.path()));
    }

    #[test]
    fn has_cargo_toml_within_finds_at_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Cargo.toml"), "").expect("write");
        assert!(has_cargo_toml_within(dir.path(), 0));
    }

    #[test]
    fn has_cargo_toml_within_finds_in_subdir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("crate1");
        std::fs::create_dir_all(&sub).expect("mkdir");
        std::fs::write(sub.join("Cargo.toml"), "").expect("write");
        assert!(has_cargo_toml_within(dir.path(), 2));
    }

    #[test]
    fn has_cargo_toml_within_returns_false_when_too_deep() {
        let dir = tempfile::tempdir().expect("tempdir");
        let deep = dir.path().join("a/b/c");
        std::fs::create_dir_all(&deep).expect("mkdir");
        std::fs::write(deep.join("Cargo.toml"), "").expect("write");
        assert!(!has_cargo_toml_within(dir.path(), 1));
    }

    #[test]
    fn assess_project_scope_allows_when_project_marker_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Cargo.toml"), "").expect("write");
        let assessment = assess_project_scope(dir.path());
        assert!(assessment.allow_auto_index);
        assert!(assessment.has_project_marker);
    }

    #[test]
    fn quick_language_profile_detects_rust() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").expect("write");
        std::fs::write(dir.path().join("a.rs"), "fn main() {}\n").expect("write");
        let profile = quick_language_profile(dir.path());
        assert_eq!(profile.primary_language.as_deref(), Some("rust"));
        assert!(profile.supported);
    }

    #[test]
    fn quick_language_profile_marks_unsupported_when_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let profile = quick_language_profile(dir.path());
        assert!(!profile.supported);
        assert!(profile.languages.is_empty());
    }

    #[test]
    fn language_profile_counts_rust_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\n").expect("write");
        std::fs::write(
            dir.path().join("a.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .expect("write");
        let profile = language_profile(dir.path());
        assert!(profile.supported);
        let rust = profile
            .languages
            .iter()
            .find(|l| l.language == "rust")
            .expect("rust entry");
        assert_eq!(rust.files, 1);
        assert!(rust.code_lines >= 3);
    }
}
