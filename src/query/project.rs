//! Helpers for loading project data and constructing `ProjectSummary` values.

use crate::api::{ProjectCounts, ProjectLanguageStat, ProjectSummary, SessionSummary};
use crate::project::ProjectData;
use std::path::{Path, PathBuf};

/// Derive a filesystem-safe slug from `path` by replacing `/` with `-`.
pub fn slug_for_path(path: &Path) -> String {
    path.to_string_lossy().replace('/', "-")
}

/// Load and parse all Rust source files under `path` into a `ProjectData`.
///
/// When `include_ignored` is `true`, files excluded by `.gitignore` are
/// indexed as well.
pub fn load_project(path: &Path, include_ignored: bool) -> ProjectData {
    ProjectData::load(path, include_ignored)
}

/// Summarise the sizes of the indexed collections in `project`.
pub fn project_counts(project: &ProjectData) -> ProjectCounts {
    ProjectCounts {
        files: project.rust_files.len(),
        functions: project.functions.len(),
        structs: project.structs.len(),
        enums: project.enums.len(),
        call_sites: project.call_sites.len(),
    }
}

/// All data required to build a [`ProjectSummary`] via [`project_summary`].
pub struct ProjectSummaryInput<'a> {
    /// URL-safe identifier derived from the project path.
    pub slug: String,
    /// Human-readable project name shown in the UI.
    pub display_name: String,
    /// Absolute filesystem path to the project root, if known.
    pub path: Option<&'a Path>,
    /// Unix timestamp (ms) of the most recent activity for this project.
    pub last_active_unix_ms: u64,
    /// Primary detected language (e.g. `"rust"`).
    pub primary_language: Option<String>,
    /// Per-language breakdown returned by a language-profile call.
    pub languages: Vec<ProjectLanguageStat>,
    /// Whether rustgraph can index this project.
    pub supported: bool,
    /// Whether the project has been pinned by the user.
    pub pinned: bool,
    /// How this project was added (e.g. `"auto"`, `"manual"`).
    pub discovered_from: String,
    /// Sessions currently open against this project.
    pub active_sessions: Vec<SessionSummary>,
    /// Loaded index data; `None` skips populating `counts` in the summary.
    pub project: Option<&'a ProjectData>,
    /// Unix timestamp (ms) when the index was last built, if available.
    pub indexed_at_unix_ms: Option<u64>,
}

/// Assemble a [`ProjectSummary`] from the provided `input` data.
///
/// `counts` is populated only when `input.project` is `Some`.
pub fn project_summary(input: ProjectSummaryInput<'_>) -> ProjectSummary {
    ProjectSummary {
        slug: input.slug,
        display_name: input.display_name,
        path: input.path.map(|value| value.to_string_lossy().to_string()),
        last_active_unix_ms: input.last_active_unix_ms,
        primary_language: input.primary_language,
        languages: input.languages,
        supported: input.supported,
        pinned: input.pinned,
        discovered_from: input.discovered_from,
        active_sessions: input.active_sessions,
        indexed_at_unix_ms: input.indexed_at_unix_ms,
        counts: input.project.map(project_counts),
    }
}

/// Return the path where a project's cached index JSON is stored.
///
/// Resolves to `<base>/<slug>.json`.
pub fn cache_path_for_slug(base: &Path, slug: &str) -> PathBuf {
    base.join(format!("{slug}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_for_path_replaces_slashes_with_dashes() {
        let p = std::path::Path::new("/home/user/project");
        assert_eq!(slug_for_path(p), "-home-user-project");
    }

    #[test]
    fn slug_for_path_handles_relative_paths() {
        let p = std::path::Path::new("foo/bar");
        assert_eq!(slug_for_path(p), "foo-bar");
    }

    #[test]
    fn cache_path_for_slug_appends_json_extension() {
        let base = std::path::Path::new("/tmp/cache");
        let p = cache_path_for_slug(base, "my-slug");
        assert_eq!(p, std::path::PathBuf::from("/tmp/cache/my-slug.json"));
    }

    #[test]
    fn project_counts_returns_lengths_of_collections() {
        let project = ProjectData::default();
        let counts = project_counts(&project);
        assert_eq!(counts.files, 0);
        assert_eq!(counts.functions, 0);
        assert_eq!(counts.structs, 0);
        assert_eq!(counts.enums, 0);
        assert_eq!(counts.call_sites, 0);
    }

    #[test]
    fn project_summary_omits_counts_when_no_project_data() {
        let p = std::path::Path::new("/proj");
        let input = ProjectSummaryInput {
            slug: "slug".to_string(),
            display_name: "Demo".to_string(),
            path: Some(p),
            last_active_unix_ms: 0,
            primary_language: Some("rust".to_string()),
            languages: vec![],
            supported: true,
            pinned: false,
            discovered_from: "manual".to_string(),
            active_sessions: vec![],
            project: None,
            indexed_at_unix_ms: None,
        };
        let summary = project_summary(input);
        assert_eq!(summary.slug, "slug");
        assert_eq!(summary.display_name, "Demo");
        assert!(summary.path.unwrap().contains("/proj"));
        assert!(summary.counts.is_none());
        assert!(summary.supported);
    }

    #[test]
    fn project_summary_includes_counts_when_project_provided() {
        let project = ProjectData::default();
        let input = ProjectSummaryInput {
            slug: "slug".to_string(),
            display_name: "Demo".to_string(),
            path: None,
            last_active_unix_ms: 0,
            primary_language: None,
            languages: vec![],
            supported: false,
            pinned: false,
            discovered_from: "auto".to_string(),
            active_sessions: vec![],
            project: Some(&project),
            indexed_at_unix_ms: Some(1234),
        };
        let summary = project_summary(input);
        assert!(summary.counts.is_some());
        assert_eq!(summary.indexed_at_unix_ms, Some(1234));
    }
}
