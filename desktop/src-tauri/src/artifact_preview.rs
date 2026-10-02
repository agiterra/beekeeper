//! Previewing an HTML document artifact, with its scripts running.
//!
//! A mockup an agent wrote is worth reviewing as the thing it is, which means
//! its JavaScript has to run. That is the same stored-XSS shape the media
//! layer refuses (`buzz_media::validation::BLOCKED_FILE_MIME_TYPES` blocks
//! `image/svg+xml` and `application/javascript`, and serves `text/html` as a
//! download), so the isolation here is three independent layers and none of
//! them is decorative:
//!
//! 1. **Its own origin.** A `buzz-doc://` scheme, registered beside
//!    `buzz-media`. An inline `<iframe srcdoc>` would not do: a srcdoc
//!    document *inherits* the embedder's CSP, so under the app's
//!    `script-src 'self'` the scripts would silently not run — a preview that
//!    lies about what it is showing.
//! 2. **Its own CSP**, on the response. Scripts run; `connect-src 'none'`
//!    and `default-src 'none'` mean the document cannot phone home, load
//!    remote code, or submit a form.
//! 3. **No capabilities.** The window is labelled [`PREVIEW_LABEL_PREFIX`],
//!    which matches no `windows` entry in `capabilities/default.json`, so
//!    every Tauri command — `core:default` included — is denied in it.
//!
//! What the scheme serves is a **snapshot**, not the repository: the caller
//! materializes the document and its sibling assets into a map, that map is
//! registered under an opaque token, and the handler can answer nothing else.
//! There is no path to traverse, because there is no filesystem behind it.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{http, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::managed_agents::agents_repo_read::{read_tip, resolve_agents_repo, AgentsRepoCheckout};
use crate::AppState;

/// Window labels under this prefix match no capability, so a page in one can
/// call nothing. Changing it without changing `capabilities/default.json`
/// would hand the preview the app's whole command surface, which is why
/// `preview_window_label_matches_no_capability` pins the pair.
pub const PREVIEW_LABEL_PREFIX: &str = "artifact-preview-";

/// The policy a previewed document runs under.
///
/// `'unsafe-inline'` and `'unsafe-eval'` for scripts and styles are the point:
/// a mockup is inline HTML and its interactivity is inline script. What is
/// withheld is reach — no `connect-src`, so no fetch, no XHR, no WebSocket, no
/// beacon; no remote `script-src`, so no CDN; no `form-action`; no
/// `frame-ancestors`, so the page cannot be embedded back into the app.
pub const PREVIEW_CSP: &str = "default-src 'none'; \
     script-src 'unsafe-inline' 'unsafe-eval' buzz-doc:; \
     style-src 'unsafe-inline' buzz-doc:; \
     img-src buzz-doc: data:; \
     font-src buzz-doc: data:; \
     media-src buzz-doc: data:; \
     connect-src 'none'; \
     form-action 'none'; \
     frame-ancestors 'none'; \
     base-uri 'none'";

/// One registered preview: the files it may serve, and nothing else.
struct PreviewSnapshot {
    /// Path (relative to the document's folder) → bytes.
    files: HashMap<String, Vec<u8>>,
    /// The document's own path within the snapshot.
    entry: String,
}

/// Every open preview, by token. Managed state so the scheme handler, which
/// has no window of its own, can find the one snapshot it is answering for.
#[derive(Default)]
pub struct ArtifactPreviews(Mutex<HashMap<String, PreviewSnapshot>>);

/// What a caller gets back: where to point a window, and what was left out.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactPreviewHandle {
    /// The opaque token the scheme serves this snapshot under.
    pub token: String,
    /// The window label, for closing it.
    pub label: String,
    /// The url the window loads.
    pub url: String,
    /// Sibling assets the snapshot carries, by path.
    pub assets: Vec<String>,
    /// Assets the document references that the snapshot could not supply —
    /// named so the preview says what will be missing rather than drawing a
    /// broken image and leaving the reviewer to wonder.
    pub missing: Vec<String>,
}

fn mime_for(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    match lower.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("md") => "text/plain; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Serve `buzz-doc://localhost/<token>/<path>` from the registered snapshot.
///
/// Anything else is a 404 with no detail: the handler is reachable from a page
/// whose scripts run, so it answers only what a snapshot already holds and
/// never distinguishes "no such token" from "no such file".
pub fn handle_buzz_doc(
    app: &tauri::AppHandle,
    request: &http::Request<Vec<u8>>,
) -> http::Response<Vec<u8>> {
    let previews = app.state::<ArtifactPreviews>();
    let path = request.uri().path().trim_start_matches('/');
    let Some((token, want)) = path.split_once('/') else {
        return not_found();
    };
    let Ok(open) = previews.0.lock() else {
        return not_found();
    };
    let Some(snapshot) = open.get(token) else {
        return not_found();
    };
    let want = percent_decode(want);
    let Some(bytes) = snapshot.files.get(&want) else {
        return not_found();
    };
    http::Response::builder()
        .status(200)
        .header("content-type", mime_for(&want))
        .header("content-security-policy", PREVIEW_CSP)
        .header("cache-control", "no-store")
        // Belt and braces with the CSP: a previewed document is never a frame
        // in the app, and nothing may sniff its type into something else.
        .header("x-content-type-options", "nosniff")
        .body(bytes.clone())
        .unwrap_or_else(|_| not_found())
}

fn not_found() -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(404)
        .header("content-type", "text/plain; charset=utf-8")
        .header("cache-control", "no-store")
        .body(b"not found".to_vec())
        .expect("a static 404 builds")
}

/// Minimal `%XX` decoding, so a document referencing `my%20shot.png` resolves.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The relative references an HTML document makes to its siblings.
///
/// Deliberately a scan rather than a parse: the snapshot is an allowlist, so
/// over-collecting costs a byte read and under-collecting is reported as
/// `missing`. A reference that leaves the document's folder (`../`, a leading
/// `/`, or any scheme) is not a sibling and is skipped.
fn referenced_siblings(html: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for attribute in ["src=\"", "src='", "href=\"", "href='"] {
        let quote = attribute.chars().last().unwrap_or('"');
        for piece in html.split(attribute).skip(1) {
            let Some((value, _)) = piece.split_once(quote) else {
                continue;
            };
            let value = value.trim();
            if value.is_empty()
                || value.starts_with('/')
                || value.starts_with('#')
                || value.contains("://")
                || value.starts_with("data:")
                || value.split('/').any(|segment| segment == "..")
            {
                continue;
            }
            let value = value.split(['?', '#']).next().unwrap_or(value).to_owned();
            if !value.is_empty() && !found.contains(&value) {
                found.push(value);
            }
        }
    }
    found.sort();
    found
}

/// The folder part of a repository path, with its trailing slash.
fn folder_of(path: &str) -> String {
    match path.rfind('/') {
        Some(at) => path[..=at].to_owned(),
        None => String::new(),
    }
}

/// Build the snapshot: the document (the draft's text when there is one, else
/// `main`'s) plus every sibling it references that `main` has.
fn snapshot_for(
    repo: &AgentsRepoCheckout,
    path: &str,
    draft_text: Option<String>,
) -> Result<(PreviewSnapshot, Vec<String>, Vec<String>), String> {
    let entry = path
        .rsplit_once('/')
        .map(|(_, file)| file.to_owned())
        .unwrap_or_else(|| path.to_owned());
    let html = match draft_text {
        Some(text) => text,
        None => {
            let file = read_tip(repo, path)?;
            file.text.ok_or_else(|| {
                format!(
                    "{path} is {} on main, so there is nothing to preview",
                    file.state
                )
            })?
        }
    };
    let mut files = HashMap::new();
    files.insert(entry.clone(), html.clone().into_bytes());
    let folder = folder_of(path);
    let mut assets = Vec::new();
    let mut missing = Vec::new();
    for reference in referenced_siblings(&html) {
        let full = format!("{folder}{reference}");
        match crate::managed_agents::agents_repo_read::blob_bytes_at_tip(repo, &full) {
            Some(bytes) => {
                files.insert(reference.clone(), bytes);
                assets.push(full);
            }
            None => missing.push(full),
        }
    }
    Ok((PreviewSnapshot { files, entry }, assets, missing))
}

/// Open an HTML document artifact in its own, capability-less window.
///
/// `draft_text` previews the open draft rather than `main`; `None` previews
/// what is committed.
#[tauri::command]
pub async fn artifact_preview_open(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    path: String,
    draft_text: Option<String>,
) -> Result<ArtifactPreviewHandle, String> {
    let class = buzz_core_pkg::agents_repo_draft::validate_draft_path(&path)?;
    if class != buzz_core_pkg::agents_repo_draft::DraftPathClass::Document
        || !path.ends_with(".html")
    {
        return Err(format!(
            "{path} is not an HTML document; only those preview in a window"
        ));
    }
    let source = crate::managed_agents::role_packs_view::fetch_project_pack_source(
        &state,
        project_ref.trim(),
    )
    .await?
    .ok_or_else(|| "this project has no agents repository yet".to_owned())?;
    let app_for_blocking = app.clone();
    let (snapshot, assets, missing) = tokio::task::spawn_blocking(move || {
        let state = app_for_blocking.state::<AppState>();
        let repo = resolve_agents_repo(&app_for_blocking, &state, &source, false)?;
        snapshot_for(&repo, &path, draft_text)
    })
    .await
    .map_err(|error| format!("spawn_blocking failed: {error}"))??;

    let token = uuid::Uuid::new_v4().simple().to_string();
    let entry = snapshot.entry.clone();
    let label = format!("{PREVIEW_LABEL_PREFIX}{token}");
    let url = format!("buzz-doc://localhost/{token}/{entry}");
    app.state::<ArtifactPreviews>()
        .0
        .lock()
        .map_err(|_| "the preview registry is poisoned".to_owned())?
        .insert(token.clone(), snapshot);

    let parsed = url
        .parse()
        .map_err(|error| format!("preview url is not a url: {error}"))?;
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed))
        .title(format!("Preview — {entry}"))
        .inner_size(1024.0, 768.0)
        .min_inner_size(480.0, 360.0)
        .build()
        .map_err(|error| {
            // Never leave a snapshot registered for a window that does not
            // exist: the scheme would keep serving it for the life of the app.
            if let Ok(mut open) = app.state::<ArtifactPreviews>().0.lock() {
                open.remove(&token);
            }
            error.to_string()
        })?;
    Ok(ArtifactPreviewHandle {
        token,
        label,
        url,
        assets,
        missing,
    })
}

/// Drop a preview's snapshot and close its window.
///
/// The caller does this when the person closes the preview; without it the
/// bytes stay served for the life of the app, which is both a leak and a
/// surface.
#[tauri::command]
pub async fn artifact_preview_close(app: tauri::AppHandle, token: String) -> Result<(), String> {
    if let Ok(mut open) = app.state::<ArtifactPreviews>().0.lock() {
        open.remove(&token);
    }
    if let Some(window) = app.get_webview_window(&format!("{PREVIEW_LABEL_PREFIX}{token}")) {
        let _ = window.close();
    }
    Ok(())
}

#[cfg(test)]
#[path = "artifact_preview_tests.rs"]
mod tests;
