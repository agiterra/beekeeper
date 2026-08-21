//! H-08 — standalone transcript export: the filesystem executor.
//!
//! The serialization/planning law (bundle envelope, attachment matrix, deep
//! share rewrite, naming) lives in the TS engine at
//! `desktop/src/features/coding-sessions/lib/transcriptExport/`, asserted
//! against the banked corpus (`conformance/transcript-export/`). This module
//! only executes an already-validated plan: it refuses when the viewer dist
//! is absent, creates the export directory without ever overwriting, copies
//! the viewer, writes `transcript.json` verbatim (never re-serializing, so
//! the engine's no-leak guarantee survives byte-for-byte), and copies
//! attachments. The export is a static redacted bundle: local-only, no
//! provider credential, no upload — publication is H-05c's plane.

pub(crate) mod viewer;
mod writer;

pub(crate) use writer::write_transcript_export;

use serde::Deserialize;

/// The plan built by the TS engine (`buildTranscriptExportPlan`). Field
/// names arrive camelCase over the Tauri boundary.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranscriptExportPlan {
    pub directory_name: String,
    pub transcript_json: String,
    #[serde(default)]
    pub attachment_copies: Vec<AttachmentCopy>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachmentCopy {
    pub source_absolute_path: String,
    pub exported_file_name: String,
}

/// Reject any plan segment outside ASCII `[A-Za-z0-9_.-]`, plus empty
/// names, `"."`, and `".."`. Byte-level ASCII on purpose: Rust regex `\w`
/// is Unicode-aware and would silently widen the banked ASCII naming law.
/// This only validates — sanitization is the TS engine's job, and a plan
/// that arrives with an unsanitized segment is refused, not repaired.
pub(crate) fn validate_file_name_segment(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("Invalid export file segment: {name:?}"));
    }
    let valid = name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'));
    if !valid {
        return Err(format!("Invalid export file segment: {name:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_file_name_segment;

    #[test]
    fn accepts_sanitized_segments() {
        for name in [
            "Release-Review-2026-04-23T12-34-56Z",
            "att-1-shot-final-.png",
            "chat-2026-04-23T12-34-56Z-2",
        ] {
            assert!(validate_file_name_segment(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn rejects_traversal_separators_and_non_ascii() {
        for name in ["", ".", "..", "../x", "a/b", "a\\b", "/abs", "üñïçode"] {
            assert!(validate_file_name_segment(name).is_err(), "{name:?}");
        }
    }
}
