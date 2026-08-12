//! Tauri commands for the H-08 standalone transcript export.
//!
//! Two roundtrips: `begin_coding_session_transcript_export` collects the
//! filesystem facts the pure TS engine needs (export root from a folder
//! picker, taken directory names, attachment stat results, the materialized
//! viewer dist, the app version); the renderer builds the full export plan
//! with `buildTranscriptExportPlan`; `write_coding_session_transcript_export`
//! executes that plan. The write is a static redacted bundle — local only,
//! no provider credential, no upload (publication is H-05c's plane).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::transcript_export::{self, viewer::materialize_viewer_dist, TranscriptExportPlan};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptExportProbe {
    pub export_root: String,
    pub taken_directory_names: Vec<String>,
    pub attachment_source_exists: HashMap<String, bool>,
    pub app_version: String,
    pub viewer_dist_dir: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WrittenTranscriptExport {
    pub output_dir: String,
    pub index_html_path: String,
    pub transcript_json_path: String,
    pub bundled_attachment_count: usize,
}

/// Open a folder picker and, when the user chooses an export root, return
/// the filesystem facts the export planner needs. `Ok(None)` = cancelled.
#[tauri::command]
pub async fn begin_coding_session_transcript_export(
    app: AppHandle,
    attachment_absolute_paths: Vec<String>,
) -> Result<Option<TranscriptExportProbe>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    let selected = rx.await.map_err(|_| "dialog cancelled".to_string())?;
    let export_root = match selected {
        Some(folder) => folder
            .as_path()
            .ok_or_else(|| "Folder dialog returned an invalid path".to_string())?
            .to_path_buf(),
        None => return Ok(None),
    };

    let taken_directory_names = list_directory_names(&export_root)?;
    let attachment_source_exists = attachment_absolute_paths
        .into_iter()
        .map(|path| {
            let exists = Path::new(&path).is_file();
            (path, exists)
        })
        .collect();

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve the app data dir: {error}"))?;
    let app_version = app.package_info().version.to_string();
    let viewer_dist_dir = materialize_viewer_dist(&app_data_dir, &app_version)?;

    Ok(Some(TranscriptExportProbe {
        export_root: export_root.to_string_lossy().into_owned(),
        taken_directory_names,
        attachment_source_exists,
        app_version,
        viewer_dist_dir: viewer_dist_dir.to_string_lossy().into_owned(),
    }))
}

/// Execute a planned export. The viewer dist must be the one this app
/// materialized under its own data dir — anything else is refused.
#[tauri::command]
pub async fn write_coding_session_transcript_export(
    app: AppHandle,
    plan: TranscriptExportPlan,
    viewer_dist_dir: String,
    export_root: String,
) -> Result<WrittenTranscriptExport, String> {
    let export_root = PathBuf::from(export_root);
    if !export_root.is_dir() {
        return Err("Export destination is not a directory".to_string());
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve the app data dir: {error}"))?;
    let viewer_dist_dir = PathBuf::from(viewer_dist_dir);
    if !viewer_dist_dir.starts_with(app_data_dir.join("export-viewer")) {
        return Err("Refusing a viewer dist outside the app's export-viewer root".to_string());
    }

    let outcome =
        transcript_export::write_transcript_export(&plan, &viewer_dist_dir, &export_root)?;

    Ok(WrittenTranscriptExport {
        output_dir: outcome.output_dir.to_string_lossy().into_owned(),
        index_html_path: outcome.index_html_path.to_string_lossy().into_owned(),
        transcript_json_path: outcome.transcript_json_path.to_string_lossy().into_owned(),
        bundled_attachment_count: outcome.bundled_attachment_count,
    })
}

fn list_directory_names(root: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(root)
        .map_err(|error| format!("Could not read the export destination: {error}"))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Could not read a destination entry: {error}"))?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}
