//! File-URI conversion utilities used by the rust-analyzer LSP client.

use std::fs;
use std::path::Path;
use url::Url;

fn normalize_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Convert a filesystem path to a `file://` URI string.
///
/// Relative paths are canonicalised via `fs::canonicalize` before conversion.
pub(crate) fn path_to_file_uri(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        fs::canonicalize(path)?
    };
    Url::from_file_path(&absolute)
        .map(|url| url.to_string())
        .map_err(|_| format!("failed to convert path to file URI: {}", absolute.display()).into())
}

/// Parse a `file://` URI and return the corresponding filesystem path string.
///
/// Returns `None` for non-`file` URI schemes or malformed URIs.
/// Path separators are normalised to `/` on all platforms.
pub(crate) fn uri_to_file_path(uri: &str) -> Option<String> {
    let url = Url::parse(uri).ok()?;
    let path = url.to_file_path().ok()?;
    Some(normalize_path_string(&path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn path_to_file_uri_round_trips_with_uri_to_file_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("sample.rs");
        let mut f = fs::File::create(&target).expect("create");
        f.write_all(b"// sample").expect("write");

        let uri = path_to_file_uri(&target).expect("uri");
        assert!(uri.starts_with("file://"));
        let back = uri_to_file_path(&uri).expect("path");
        assert!(back.ends_with("sample.rs"));
    }

    #[test]
    fn uri_to_file_path_returns_none_for_invalid_uri() {
        assert!(uri_to_file_path("not a uri at all").is_none());
    }

    #[test]
    fn uri_to_file_path_returns_none_for_non_file_scheme() {
        assert!(uri_to_file_path("https://example.com/x").is_none());
    }

    #[test]
    fn path_to_file_uri_uses_absolute_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("a.rs");
        fs::write(&target, b"").expect("write");
        let uri = path_to_file_uri(&target).expect("uri");
        assert!(!uri.contains(".."));
    }
}
