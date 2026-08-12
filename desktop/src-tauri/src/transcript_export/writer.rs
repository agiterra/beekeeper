//! Filesystem execution of a validated transcript-export plan.
//!
//! Behavioral backstops of the banked laws live here: export refuses when
//! the viewer dist is absent, and an existing export directory is never
//! overwritten (`fs::create_dir`, not `create_dir_all`, on the leaf — the
//! mechanical enforcement of the collision law whose naming half lives in
//! the TS engine).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::{validate_file_name_segment, TranscriptExportPlan};

pub(crate) struct ExportOutcome {
    pub output_dir: PathBuf,
    pub index_html_path: PathBuf,
    pub transcript_json_path: PathBuf,
    pub bundled_attachment_count: usize,
}

pub(crate) fn write_transcript_export(
    plan: &TranscriptExportPlan,
    viewer_dist_dir: &Path,
    export_root: &Path,
) -> Result<ExportOutcome, String> {
    // The banked refusal law: no export without the viewer dist.
    if !viewer_dist_dir.is_dir() {
        return Err(
            "Export viewer bundle not found — cannot export without the viewer dist".to_string(),
        );
    }

    validate_file_name_segment(&plan.directory_name)?;
    let mut seen_names = HashSet::new();
    for copy in &plan.attachment_copies {
        validate_file_name_segment(&copy.exported_file_name)?;
        if !seen_names.insert(copy.exported_file_name.as_str()) {
            return Err(format!(
                "Duplicate exported attachment name: {:?}",
                copy.exported_file_name,
            ));
        }
    }

    let output_dir = export_root.join(&plan.directory_name);
    // create_dir, never create_dir_all on the leaf: AlreadyExists refuses.
    fs::create_dir(&output_dir).map_err(|error| {
        format!(
            "Could not create export directory {}: {error}",
            output_dir.display(),
        )
    })?;

    copy_dir_contents(viewer_dist_dir, &output_dir)?;

    let transcript_json_path = output_dir.join("transcript.json");
    fs::write(&transcript_json_path, &plan.transcript_json)
        .map_err(|error| format!("Could not write transcript.json: {error}"))?;

    if !plan.attachment_copies.is_empty() {
        let attachments_dir = output_dir.join("attachments");
        fs::create_dir_all(&attachments_dir)
            .map_err(|error| format!("Could not create attachments dir: {error}"))?;
        for copy in &plan.attachment_copies {
            fs::copy(
                &copy.source_absolute_path,
                attachments_dir.join(&copy.exported_file_name),
            )
            .map_err(|error| {
                format!(
                    "Could not copy attachment {:?}: {error}",
                    copy.source_absolute_path,
                )
            })?;
        }
    }

    Ok(ExportOutcome {
        index_html_path: output_dir.join("index.html"),
        transcript_json_path,
        bundled_attachment_count: plan.attachment_copies.len(),
        output_dir,
    })
}

fn copy_dir_contents(source: &Path, destination: &Path) -> Result<(), String> {
    let entries = fs::read_dir(source)
        .map_err(|error| format!("Could not read viewer dist {}: {error}", source.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("Could not read viewer dist entry: {error}"))?;
        let target = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Could not stat viewer dist entry: {error}"))?;
        if file_type.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| format!("Could not create {}: {error}", target.display()))?;
            copy_dir_contents(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .map_err(|error| format!("Could not copy {}: {error}", target.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{AttachmentCopy, TranscriptExportPlan};
    use super::*;

    fn plan(directory_name: &str, copies: Vec<AttachmentCopy>) -> TranscriptExportPlan {
        TranscriptExportPlan {
            directory_name: directory_name.to_string(),
            transcript_json: "{\n  \"version\": 1\n}\n".to_string(),
            attachment_copies: copies,
        }
    }

    fn viewer_dist(root: &Path) -> PathBuf {
        let dist = root.join("viewer-dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("index.html"), "<!doctype html>viewer").unwrap();
        dist
    }

    #[test]
    fn refuses_when_viewer_dist_is_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let export_root = tmp.path().join("exports");
        fs::create_dir_all(&export_root).unwrap();
        let missing = tmp.path().join("no-viewer-here");
        let result = write_transcript_export(&plan("export-1", vec![]), &missing, &export_root);
        let error = result.err().expect("must refuse without a viewer dist");
        assert!(error.contains("viewer"), "{error}");
        assert!(!export_root.join("export-1").exists());
    }

    #[test]
    fn never_overwrites_an_existing_export_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let export_root = tmp.path().join("exports");
        let existing = export_root.join("export-1");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("keep.txt"), "precious").unwrap();
        let dist = viewer_dist(tmp.path());
        let result = write_transcript_export(&plan("export-1", vec![]), &dist, &export_root);
        assert!(result.is_err(), "an existing export dir must refuse");
        assert_eq!(
            fs::read_to_string(existing.join("keep.txt")).unwrap(),
            "precious",
        );
    }

    #[test]
    fn writes_transcript_json_beside_the_viewer_index() {
        let tmp = tempfile::tempdir().unwrap();
        let export_root = tmp.path().join("exports");
        fs::create_dir_all(&export_root).unwrap();
        let dist = viewer_dist(tmp.path());
        let outcome =
            write_transcript_export(&plan("export-1", vec![]), &dist, &export_root).unwrap();
        assert_eq!(outcome.output_dir, export_root.join("export-1"));
        assert_eq!(
            fs::read_to_string(&outcome.transcript_json_path).unwrap(),
            "{\n  \"version\": 1\n}\n",
        );
        assert_eq!(
            fs::read_to_string(&outcome.index_html_path).unwrap(),
            "<!doctype html>viewer",
        );
        assert_eq!(outcome.bundled_attachment_count, 0);
        assert!(!outcome.output_dir.join("attachments").exists());
    }

    #[test]
    fn copies_nested_viewer_dist_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let export_root = tmp.path().join("exports");
        fs::create_dir_all(&export_root).unwrap();
        let dist = viewer_dist(tmp.path());
        fs::create_dir_all(dist.join("assets/fonts")).unwrap();
        fs::write(dist.join("assets/viewer.js"), "js").unwrap();
        fs::write(dist.join("assets/fonts/brand.woff2"), "font").unwrap();
        let outcome =
            write_transcript_export(&plan("export-1", vec![]), &dist, &export_root).unwrap();
        assert_eq!(
            fs::read_to_string(outcome.output_dir.join("assets/viewer.js")).unwrap(),
            "js",
        );
        assert_eq!(
            fs::read_to_string(outcome.output_dir.join("assets/fonts/brand.woff2")).unwrap(),
            "font",
        );
    }

    #[test]
    fn copies_attachments_under_attachments_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let export_root = tmp.path().join("exports");
        fs::create_dir_all(&export_root).unwrap();
        let dist = viewer_dist(tmp.path());
        let source = tmp.path().join("live.png");
        fs::write(&source, "png-bytes").unwrap();
        let outcome = write_transcript_export(
            &plan(
                "export-1",
                vec![AttachmentCopy {
                    source_absolute_path: source.to_string_lossy().into_owned(),
                    exported_file_name: "att-live-live.png".to_string(),
                }],
            ),
            &dist,
            &export_root,
        )
        .unwrap();
        assert_eq!(outcome.bundled_attachment_count, 1);
        assert_eq!(
            fs::read_to_string(
                outcome
                    .output_dir
                    .join("attachments")
                    .join("att-live-live.png"),
            )
            .unwrap(),
            "png-bytes",
        );
    }

    #[test]
    fn rejects_traversal_segments_in_plan() {
        let tmp = tempfile::tempdir().unwrap();
        let export_root = tmp.path().join("exports");
        fs::create_dir_all(&export_root).unwrap();
        let dist = viewer_dist(tmp.path());
        for bad_dir in ["../escape", "a/b", "..", ""] {
            let result = write_transcript_export(&plan(bad_dir, vec![]), &dist, &export_root);
            assert!(result.is_err(), "{bad_dir:?} must be rejected");
        }
        let result = write_transcript_export(
            &plan(
                "export-1",
                vec![AttachmentCopy {
                    source_absolute_path: "/tmp/x".to_string(),
                    exported_file_name: "../escape.png".to_string(),
                }],
            ),
            &dist,
            &export_root,
        );
        assert!(
            result.is_err(),
            "traversal attachment name must be rejected"
        );
        assert!(!export_root.join("export-1").exists());
    }

    #[test]
    fn rejects_duplicate_exported_attachment_names() {
        let tmp = tempfile::tempdir().unwrap();
        let export_root = tmp.path().join("exports");
        fs::create_dir_all(&export_root).unwrap();
        let dist = viewer_dist(tmp.path());
        let source = tmp.path().join("a.png");
        fs::write(&source, "bytes").unwrap();
        let copy = |name: &str| AttachmentCopy {
            source_absolute_path: source.to_string_lossy().into_owned(),
            exported_file_name: name.to_string(),
        };
        // The donor would silently overwrite inside attachments/; Buzz
        // refuses loudly — a deliberate, recorded hardening.
        let result = write_transcript_export(
            &plan("export-1", vec![copy("same.png"), copy("same.png")]),
            &dist,
            &export_root,
        );
        assert!(result.is_err(), "duplicate exported names must be rejected");
        assert!(!export_root.join("export-1").exists());
    }
}
