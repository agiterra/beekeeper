//! Tauri commands for sharing the Browser with the session (V-CONTRACT,
//! VB → VC). All async: snapshots wait on main-thread work.

use nostr::PublicKey;
use tauri::AppHandle;

use super::super::{normalize_channel_id, PreviewError};
use super::capture::{self, SnapshotReceipt};
use super::{
    emit_share_state, ensure_sweeper, note_publish, now_ms, read_facts, reoffer, with_entry,
    SessionPreviewShareState, WatchAction,
};

/// A 64-character lowercase hex pubkey, or `None`.
fn parse_pubkey(raw: Option<&str>) -> Option<PublicKey> {
    let raw = raw?.trim();
    let shaped = raw.len() == 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    shaped.then(|| PublicKey::from_hex(raw).ok()).flatten()
}

/// A sessionRef (umbrella UUID), lowercased and hyphenated.
fn normalize_session_ref(raw: &str) -> Result<String, PreviewError> {
    uuid::Uuid::parse_str(raw.trim())
        .map(|id| id.hyphenated().to_string())
        .map_err(|_| PreviewError::bad_request(format!("{raw:?} is not a session reference.")))
}

/// Configure sharing for a channel's preview: the session it belongs to,
/// the person's Share toggle, and the UI's idea of the provider key (used
/// only when this machine's provider record names none).
#[tauri::command]
pub async fn session_preview_share_configure(
    app: AppHandle,
    channel_id: String,
    session_ref: Option<String>,
    share: bool,
    provider_pubkey: Option<String>,
) -> Result<SessionPreviewShareState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    let session_ref = session_ref
        .as_deref()
        .map(normalize_session_ref)
        .transpose()?;
    ensure_sweeper(&app);
    let provider =
        capture::resolve_local_provider(&app).or_else(|| parse_pubkey(provider_pubkey.as_deref()));
    let facts = read_facts(&channel_id);
    // Moving the preview to another session retracts the old announce at
    // once: it is a different address, so no debounce applies.
    let retract = with_entry(&channel_id, |entry| {
        let previous = entry.debounce.last_sent.clone();
        entry.config.session_ref = session_ref.clone();
        entry.config.share = share;
        entry.config.provider = provider;
        match previous {
            Some(sent)
                if sent.status == beekeeper_core_pkg::session_preview::PreviewStatus::Open
                    && Some(&sent.session_ref) != session_ref.as_ref() =>
            {
                entry.debounce = Default::default();
                entry.announced = None;
                entry.watchers = Default::default();
                Some(sent.closed())
            }
            _ => None,
        }
    });
    if let Some(closed) = retract {
        let app = app.clone();
        let channel = channel_id.clone();
        tauri::async_runtime::spawn(async move {
            capture::publish_announce(&app, &channel, closed).await;
        });
    }
    reoffer(&channel_id, Some(facts));
    Ok(emit_share_state(&channel_id))
}

/// The share state of a channel's preview (defaults when never configured).
#[tauri::command]
pub async fn session_preview_share_state(
    channel_id: String,
) -> Result<SessionPreviewShareState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    let now = now_ms();
    Ok(with_entry(&channel_id, |entry| {
        entry.share_state(&channel_id, now)
    }))
}

/// The camera button: publish a 44253 of the preview now.
#[tauri::command]
pub async fn session_preview_share_snapshot(
    app: AppHandle,
    channel_id: String,
    alt: Option<String>,
) -> Result<SnapshotReceipt, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    capture::publish_snapshot(&app, &channel_id, None, alt).await
}

/// A 24320 addressed to this desktop (forwarded by the pump). Re-checks the
/// config: the sessionRef must match and sharing must be on (except `stop`).
#[tauri::command]
pub async fn session_preview_share_watch(
    app: AppHandle,
    channel_id: String,
    session_ref: String,
    watcher_pubkey: String,
    action: String,
) -> Result<(), PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    let session_ref = normalize_session_ref(&session_ref)?;
    let action = WatchAction::parse(&action)
        .ok_or_else(|| PreviewError::bad_request(format!("{action:?} is not a watch action.")))?;
    let watcher = parse_pubkey(Some(&watcher_pubkey))
        .ok_or_else(|| PreviewError::bad_request("The watcher is not a pubkey."))?;
    ensure_sweeper(&app);
    let now = now_ms();
    let outcome = with_entry(&channel_id, |entry| {
        if entry.config.session_ref.as_deref() != Some(session_ref.as_str()) {
            return Err(PreviewError::new(
                "preview_share_unavailable",
                "This Browser is not shared for that session.",
            ));
        }
        if !entry.config.share && action != WatchAction::Stop {
            return Err(PreviewError::new(
                "preview_share_unavailable",
                "The person turned sharing off for this Browser.",
            ));
        }
        entry.apply_watch(&watcher.to_hex(), action, now)
    })?;
    if outcome.state_changed {
        emit_share_state(&channel_id);
    }
    if outcome.snapshot {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) =
                capture::publish_snapshot(&app, &channel_id, Some(watcher), None).await
            {
                // A refusal publishes nothing; the watcher's UI times out.
                eprintln!(
                    "session-preview-broadcast: {channel_id}: requested snapshot refused: {}",
                    error.message
                );
            }
        });
    }
    Ok(())
}

/// The pump's report of the relay's OK for a frame it published.
#[tauri::command]
pub async fn session_preview_share_note_publish(
    channel_id: String,
    accepted: bool,
    message: String,
) -> Result<(), PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    note_publish(&channel_id, accepted, &message);
    Ok(())
}
