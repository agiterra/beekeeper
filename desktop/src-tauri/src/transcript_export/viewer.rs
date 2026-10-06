//! The embedded Beekeeper export viewer: a single static, dependency-free HTML
//! page compiled into the binary and materialized to an on-disk dist dir so
//! the writer's viewer-copy (and its refusal law) operate on a real
//! directory. The page is Beekeeper-owned and written from scratch — a recorded
//! architectural divergence from the donor's React viewer.

use std::fs;
use std::path::{Path, PathBuf};

const VIEWER_INDEX_HTML: &str = include_str!("viewer/index.html");

/// Materialize the embedded viewer as `<app_data>/export-viewer/<version>/`.
/// Idempotent: rewriting the same version's dist is fine, the content is
/// compile-time constant per app version.
pub(crate) fn materialize_viewer_dist(
    app_data_dir: &Path,
    app_version: &str,
) -> Result<PathBuf, String> {
    super::validate_file_name_segment(app_version)?;
    let dist = app_data_dir.join("export-viewer").join(app_version);
    fs::create_dir_all(&dist)
        .map_err(|error| format!("Could not create viewer dist {}: {error}", dist.display()))?;
    fs::write(dist.join("index.html"), VIEWER_INDEX_HTML)
        .map_err(|error| format!("Could not write the export viewer page: {error}"))?;
    Ok(dist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materializes_an_index_html_dist_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dist = materialize_viewer_dist(tmp.path(), "1.2.3").unwrap();
        assert_eq!(dist, tmp.path().join("export-viewer").join("1.2.3"));
        let index = fs::read_to_string(dist.join("index.html")).unwrap();
        assert!(index.contains("./transcript.json"));
        // Re-materializing the same version is idempotent.
        let again = materialize_viewer_dist(tmp.path(), "1.2.3").unwrap();
        assert_eq!(again, dist);
    }

    #[test]
    fn embedded_viewer_is_self_contained() {
        // Static-redacted authority: the viewer makes no external requests.
        assert!(VIEWER_INDEX_HTML.contains("./transcript.json"));
        assert!(!VIEWER_INDEX_HTML.contains("http://"));
        assert!(!VIEWER_INDEX_HTML.contains("https://"));
    }

    #[test]
    fn rejects_a_version_that_is_not_a_clean_segment() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(materialize_viewer_dist(tmp.path(), "../evil").is_err());
    }
}
