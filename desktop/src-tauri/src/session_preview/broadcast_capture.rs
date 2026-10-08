//! The broadcaster's side effects: pictures (WKWebView snapshot → frame
//! within budget), signing and handing frames to the pump, publishing
//! announces and snapshots, and finding this machine's provider key.
//!
//! [`prepare_frame`] is pure (bytes in, frame out) so the budget step-down is
//! unit-tested; everything else needs the app.

use beekeeper_core_pkg::surface_snapshot::MAX_SURFACE_SNAPSHOT_ALT_BYTES;
use beekeeper_core_pkg::surface_snapshot::{SurfaceSnapshot, SurfaceSnapshotType};
use beekeeper_core_pkg::surface_watch::{
    encode_frame_content, redact_free_text, Surface, SurfaceFrameHeader,
    PREVIEW_FRAME_BUDGET_BYTES, PREVIEW_FRAME_MAX_LONG_EDGE,
};
use beekeeper_sdk_pkg::surface::{
    build_session_preview_announce, build_surface_frame, build_surface_snapshot,
};
use image::imageops::FilterType;
use image::RgbImage;
use nostr::{JsonUtil, PublicKey};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use super::super::view::{self, ImageEncoding};
use super::super::{PreviewError, Unavailable};
use super::{
    clamp_dims, clean_title, emit_share_state, now_ms, page_for, read_facts, with_entry,
    AnnounceSpec, FramePlan, PreparedFrame, ShareConfig, BROADCAST_PUBLISH_EVENT,
    NO_PROVIDER_SENTENCE, SHARE_OFF_SENTENCE,
};

/// JPEG qualities tried in order at each size before shrinking further.
const QUALITIES: [u8; 4] = [80, 65, 50, 35];
/// Never shrink a frame's long edge below this to fit the budget.
const MIN_LONG_EDGE: u32 = 160;

/// Downscale a WebKit JPEG to [`PREVIEW_FRAME_MAX_LONG_EDGE`] and re-encode
/// it until its base64 fits `budget`. `Ok(None)` when its pixels hash to
/// `last_hash` and `force` is false (change-only).
pub(crate) fn prepare_frame(
    jpeg: &[u8],
    last_hash: Option<&[u8]>,
    force: bool,
    budget: usize,
) -> Result<Option<PreparedFrame>, String> {
    let image =
        image::load_from_memory(jpeg).map_err(|e| format!("cannot decode the snapshot: {e}"))?;
    let max = PREVIEW_FRAME_MAX_LONG_EDGE;
    let image = if image.width().max(image.height()) > max {
        image.resize(max, max, FilterType::Triangle)
    } else {
        image
    };
    let rgb = image.to_rgb8();
    let hash = Sha256::digest(rgb.as_raw()).to_vec();
    if !force && last_hash == Some(hash.as_slice()) {
        return Ok(None);
    }
    let (content, width, height) = encode_within_budget(rgb, budget)?;
    Ok(Some(PreparedFrame {
        hash,
        content,
        width,
        height,
    }))
}

fn encode_jpeg(rgb: &RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("cannot encode a frame: {e}"))?;
    Ok(out)
}

/// Step quality down, then size (×¾ each round), until the base64 fits.
fn encode_within_budget(mut rgb: RgbImage, budget: usize) -> Result<(String, u32, u32), String> {
    loop {
        for quality in QUALITIES {
            let content = encode_frame_content(&encode_jpeg(&rgb, quality)?);
            if content.len() <= budget {
                return Ok((content, rgb.width(), rgb.height()));
            }
        }
        let (width, height) = rgb.dimensions();
        if width.max(height) <= MIN_LONG_EDGE {
            return Err(format!(
                "a {width}x{height} frame does not fit {budget} bytes"
            ));
        }
        rgb = image::imageops::resize(
            &rgb,
            (width * 3 / 4).max(1),
            (height * 3 / 4).max(1),
            FilterType::Triangle,
        );
    }
}

fn signing_keys(app: &AppHandle) -> Result<nostr::Keys, String> {
    app.state::<crate::app_state::AppState>().signing_keys()
}

/// Take one picture for the stream and send it if it changed (or a resync
/// is pending).
pub(super) async fn capture_frame(
    app: &AppHandle,
    channel_id: &str,
    last_hash: Option<Vec<u8>>,
    force: bool,
) {
    let prepared = match view::snapshot(app, channel_id, ImageEncoding::Jpeg).await {
        Ok(picture) => {
            let job = tauri::async_runtime::spawn_blocking(move || {
                prepare_frame(
                    &picture.bytes,
                    last_hash.as_deref(),
                    force,
                    PREVIEW_FRAME_BUDGET_BYTES,
                )
            });
            match job.await {
                Ok(Ok(prepared)) => prepared,
                Ok(Err(error)) => {
                    eprintln!("session-preview-broadcast: {channel_id}: {error}");
                    None
                }
                Err(error) => {
                    eprintln!("session-preview-broadcast: {channel_id}: frame job: {error}");
                    None
                }
            }
        }
        Err(error) => {
            eprintln!(
                "session-preview-broadcast: {channel_id}: snapshot failed: {}",
                error.message
            );
            None
        }
    };
    let now = now_ms();
    let plan = with_entry(channel_id, |entry| {
        entry.finish_capture(prepared.as_ref(), now)
    });
    if let (Some(plan), Some(prepared)) = (plan, prepared) {
        emit_frame(app, channel_id, &plan, &prepared.content);
        emit_share_state(channel_id);
    }
}

/// Build, sign and hand one 24321 to the pump.
pub(super) fn emit_frame(app: &AppHandle, channel_id: &str, plan: &FramePlan, content: &str) {
    let Ok(channel_uuid) = Uuid::parse_str(channel_id) else {
        return;
    };
    let Some(session_ref) = with_entry(channel_id, |entry| entry.config.session_ref.clone()) else {
        return;
    };
    let header = SurfaceFrameHeader {
        channel_id: channel_uuid,
        surface: Surface::Preview,
        key: session_ref,
        frame_type: plan.frame_type,
        seq: plan.seq,
        epoch: plan.epoch,
        cadence_ms: plan.cadence_ms,
        width: plan.dims.0,
        height: plan.dims.1,
        captured_at_ms: plan.captured_at_ms,
        actor: None,
        commit: None,
    };
    let signed = build_surface_frame(&header, content)
        .map_err(|e| e.to_string())
        .and_then(|builder| {
            let keys = signing_keys(app)?;
            builder.sign_with_keys(&keys).map_err(|e| e.to_string())
        });
    match signed {
        Ok(event) => {
            let payload = serde_json::json!({ "channelId": channel_id, "event": event.as_json() });
            if let Err(error) = app.emit(BROADCAST_PUBLISH_EVENT, payload) {
                eprintln!("session-preview-broadcast: emit frame failed: {error}");
            }
        }
        Err(error) => {
            eprintln!("session-preview-broadcast: {channel_id}: frame not signed: {error}")
        }
    }
}

/// The preview's size when nothing else knows it: a picture's pixel size.
async fn measure_viewport(app: &AppHandle, channel_id: &str) -> Option<(u32, u32)> {
    let picture = view::snapshot(app, channel_id, ImageEncoding::Jpeg)
        .await
        .ok()?;
    clamp_dims(picture.width as f64, picture.height as f64)
}

/// Publish one 30626 over HTTP; on failure the debounce retries it.
pub(super) async fn publish_announce(app: &AppHandle, channel_id: &str, spec: AnnounceSpec) {
    let viewport = match spec.viewport {
        Some(viewport) => Some(viewport),
        None => measure_viewport(app, channel_id).await,
    };
    let Some(viewport) = viewport else {
        eprintln!(
            "session-preview-broadcast: {channel_id}: announce waits: the preview's size is not known yet"
        );
        with_entry(channel_id, |entry| entry.debounce.failed(spec));
        return;
    };
    let announce = spec.to_announce(viewport);
    let result = match build_session_preview_announce(&announce) {
        Ok(builder) => {
            let state = app.state::<crate::app_state::AppState>();
            crate::relay::submit_event(builder, &state)
                .await
                .and_then(|response| {
                    if response.accepted {
                        Ok(())
                    } else {
                        Err(response.message)
                    }
                })
        }
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(()) => {
            with_entry(channel_id, |entry| {
                entry.announced = Some(spec.status);
                entry.debounce.note_viewport(&spec, viewport);
            });
            emit_share_state(channel_id);
        }
        Err(error) => {
            eprintln!("session-preview-broadcast: {channel_id}: announce failed: {error}");
            with_entry(channel_id, |entry| entry.debounce.failed(spec));
        }
    }
}

/// The receipt the camera button gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotReceipt {
    /// The 44253 id.
    pub event_id: String,
    /// The Blossom URL.
    pub url: String,
    /// The blob's sha256.
    pub sha256: String,
}

fn unavailable(sentence: &str) -> PreviewError {
    PreviewError::new("preview_share_unavailable", sentence)
}

/// Cut redacted alt text to the wire's byte limit on a character boundary.
fn clean_alt(raw: &str) -> String {
    let mut alt = redact_free_text(raw.trim());
    if alt.len() > MAX_SURFACE_SNAPSHOT_ALT_BYTES {
        let mut cut = MAX_SURFACE_SNAPSHOT_ALT_BYTES;
        while !alt.is_char_boundary(cut) {
            cut -= 1;
        }
        alt.truncate(cut);
    }
    alt
}

/// Why no snapshot may be taken for this config, or `None`. Share off
/// publishes nothing — not even the camera button's one picture.
pub(crate) fn snapshot_refusal(config: &ShareConfig) -> Option<&'static str> {
    (!config.share).then_some(SHARE_OFF_SENTENCE)
}

/// Take a PNG of the preview, upload it to Blossom and publish a 44253.
/// `requested_by` names the watcher who asked (none for the camera button).
pub(super) async fn publish_snapshot(
    app: &AppHandle,
    channel_id: &str,
    requested_by: Option<PublicKey>,
    alt: Option<String>,
) -> Result<SnapshotReceipt, PreviewError> {
    let channel_uuid = Uuid::parse_str(channel_id)
        .map_err(|_| PreviewError::bad_request(format!("{channel_id:?} is not a channel id.")))?;
    let facts = read_facts(channel_id);
    if facts.not_macos || cfg!(not(target_os = "macos")) {
        return Err(unavailable(Unavailable::NOT_MACOS.sentence));
    }
    let config = with_entry(channel_id, |entry| entry.config.clone());
    // Checked before any picture is taken or uploaded.
    if let Some(refusal) = snapshot_refusal(&config) {
        return Err(unavailable(refusal));
    }
    let Some(session_ref) = config.session_ref else {
        return Err(unavailable(
            "This session has no session reference, so its Browser cannot be shared.",
        ));
    };
    let Some(provider) = config.provider else {
        return Err(unavailable(NO_PROVIDER_SENTENCE));
    };
    if !facts.open() {
        return Err(PreviewError::not_open());
    }
    let picture = view::snapshot(app, channel_id, ImageEncoding::Png).await?;
    let taken_at_ms = now_ms();
    let state = app.state::<crate::app_state::AppState>();
    let blob = crate::commands::media::upload_image_bytes(picture.bytes, &state)
        .await
        .map_err(|e| PreviewError::new("preview_share_upload_failed", e))?;
    let title = clean_title(facts.title.as_deref().unwrap_or(""));
    let snapshot = SurfaceSnapshot {
        channel_id: channel_uuid,
        snapshot_type: SurfaceSnapshotType::Snapshot,
        surface: Surface::Preview,
        key: session_ref,
        url: blob.url.clone(),
        sha256: blob.sha256.clone(),
        mime: blob.mime_type.clone(),
        width: picture.width,
        height: picture.height,
        taken_at_ms,
        provider,
        requested_by,
        // The desktop does not know the worktree's commit: the UI says
        // "commit not recorded" rather than guess.
        commit: None,
        reference: None,
        page: Some(page_for(facts.url.as_deref())),
        title: (!title.is_empty()).then_some(title),
        alt: clean_alt(alt.as_deref().unwrap_or("")),
    };
    let publish_failed = |e: String| PreviewError::new("preview_share_publish_failed", e);
    let builder = build_surface_snapshot(&snapshot).map_err(|e| publish_failed(e.to_string()))?;
    let response = crate::relay::submit_event(builder, &state)
        .await
        .map_err(publish_failed)?;
    if !response.accepted {
        return Err(publish_failed(response.message));
    }
    Ok(SnapshotReceipt {
        event_id: response.event_id,
        url: blob.url,
        sha256: blob.sha256,
    })
}

/// This machine's coding-session provider key for the active relay, from
/// the provider record on disk (no secrets read, nothing created).
pub(super) fn resolve_local_provider(app: &AppHandle) -> Option<PublicKey> {
    let state = app.state::<crate::app_state::AppState>();
    let relay_url = crate::relay::relay_ws_url_with_override(&state);
    let store = crate::session_provider::store::load_provider_readiness_store(app, None).ok()?;
    let record = store.get(&relay_url)?;
    PublicKey::from_hex(&record.provider_pubkey).ok()
}
