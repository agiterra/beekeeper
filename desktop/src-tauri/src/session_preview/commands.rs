//! Tauri commands for the Browser surface (WIRE-C4 §3). All async: they
//! wait on main-thread work, and a sync command already runs on the main
//! thread, where waiting on it would deadlock.

use beekeeper_core_pkg::coding_session_command::CodingSessionTarget;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use super::geometry::{self, SlotRect};
use super::view::{self, HistoryOp};
use super::{
    emit_state, normalize_channel_id, policy, ports, refused_origin, state_of, with_record,
    Binding, PreviewError, PreviewState, PreviewStatus,
};

/// Window labels a slot may live in: the main window and session pop-outs.
fn valid_slot_window(label: &str) -> bool {
    label == "main"
        || (label.starts_with("coding-session-")
            && label.len() <= 80
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
}

/// The slot's DOM rect as the UI reports it.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct RectArg {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Report the Browser slot's rect (or `null`: unmounted). Last write wins by
/// `seq`; a new `windowLabel` moves the view to that window.
#[tauri::command]
pub async fn session_preview_set_rect(
    app: AppHandle,
    channel_id: String,
    window_label: String,
    rect: Option<RectArg>,
    seq: u64,
) -> Result<PreviewState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    if !valid_slot_window(&window_label) {
        return Err(PreviewError::bad_request(format!(
            "{window_label:?} is not a window a Browser slot can live in."
        )));
    }
    let slot = rect.map(|r| SlotRect {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
    });
    if let Some(slot) = slot {
        geometry::view_bounds(slot).map_err(|e| PreviewError::bad_request(e.0))?;
    }
    let applied = with_record(&channel_id, |record| {
        // Unmounts are ordered like moves: the renderer's `seq` rises across
        // mounts (wall-clock based, `nextPreviewRectSeq`), and an unmount's
        // `null` can reach here after the next mount's rect (async commands
        // do not keep invoke order), so a stale `null` must not clear a
        // newer slot.
        if !record.slot_seq.accept(seq) {
            return false;
        }
        if slot.is_none() {
            record.slot = None;
            return true;
        }
        record.slot = slot;
        record.slot_window = Some(window_label);
        true
    });
    if applied {
        let dock_back = slot.is_some() && with_record(&channel_id, |record| record.auto_popped);
        if dock_back {
            // The surface an agent's `open` asked for has mounted: dock.
            return view::set_popped(&app, &channel_id, false).await;
        }
        view::sync_placement(&app, &channel_id).await?;
        return Ok(emit_state(&app, &channel_id));
    }
    Ok(state_of(&channel_id))
}

/// An overlay lease intersects the slot (or no longer does).
#[tauri::command]
pub async fn session_preview_set_occluded(
    app: AppHandle,
    channel_id: String,
    occluded: bool,
) -> Result<PreviewState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    view::set_occluded(&app, &channel_id, occluded).await
}

/// The person opens a URL from the toolbar or the empty state. `target` is
/// the session whose Browser surface they used; the preview is bound to it
/// (WIRE-C4 §9 item 8). `null` leaves it undriveable.
#[tauri::command]
pub async fn session_preview_navigate(
    app: AppHandle,
    channel_id: String,
    url: String,
    target: Option<CodingSessionTarget>,
) -> Result<PreviewState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    let url = policy::check_preview_url(&url, refused_origin(&app).as_ref())?;
    with_record(&channel_id, |record| {
        let fresh = matches!(
            record.status,
            PreviewStatus::Absent | PreviewStatus::ClosedByPerson | PreviewStatus::Unavailable
        ) || !record.has_view;
        // The person's surface decides the binding when they open a page;
        // navigating an open preview keeps whoever holds it, unless their
        // surface names a session (then it is that session's).
        if fresh || target.is_some() {
            record.binding = match &target {
                Some(target) => Binding::Person {
                    target: target.clone(),
                },
                None => Binding::None,
            };
        }
    });
    view::open(&app, &channel_id, &url, false).await
}

/// Reload.
#[tauri::command]
pub async fn session_preview_reload(
    app: AppHandle,
    channel_id: String,
) -> Result<PreviewState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    view::history(&app, &channel_id, HistoryOp::Reload).await
}

/// Back.
#[tauri::command]
pub async fn session_preview_back(
    app: AppHandle,
    channel_id: String,
) -> Result<PreviewState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    view::history(&app, &channel_id, HistoryOp::Back).await
}

/// Forward.
#[tauri::command]
pub async fn session_preview_forward(
    app: AppHandle,
    channel_id: String,
) -> Result<PreviewState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    view::history(&app, &channel_id, HistoryOp::Forward).await
}

/// The person closes the preview: agents get `preview_closed_by_person`.
#[tauri::command]
pub async fn session_preview_close(
    app: AppHandle,
    channel_id: String,
) -> Result<PreviewState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    view::close(&app, &channel_id, true).await
}

/// Pop out into `session-preview-<channelId>`, or back into the slot.
#[tauri::command]
pub async fn session_preview_popout(
    app: AppHandle,
    channel_id: String,
    popped: bool,
) -> Result<PreviewState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    view::set_popped(&app, &channel_id, popped).await
}

/// This machine's local HTTP servers; keeps the 3 s poll alive.
#[tauri::command]
pub async fn session_preview_servers(app: AppHandle) -> Result<Value, PreviewError> {
    let refused_port = refused_origin(&app).map(|origin| origin.port);
    let servers = ports::servers(refused_port)
        .await
        .map_err(|e| PreviewError::new("preview_unavailable", e))?;
    Ok(json!({ "servers": servers }))
}

/// The preview's state.
#[tauri::command]
pub async fn session_preview_status(channel_id: String) -> Result<PreviewState, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    Ok(state_of(&channel_id))
}

/// Close every open preview (not as the person: agents may open again). The
/// renderer calls this on a community switch: previews are keyed by the old
/// relay's channel ids, and a pop-out window must not outlive its community.
#[tauri::command]
pub async fn session_preview_close_all(app: AppHandle) -> Result<Value, PreviewError> {
    let channels = super::open_channel_ids();
    close_every(&channels, |channel_id| {
        let app = app.clone();
        async move { view::close(&app, &channel_id, false).await.map(|_| ()) }
    })
    .await
}

/// Close each channel's preview, continuing past failures: one stuck preview
/// must not leave the rest (and their pop-out windows) open into the next
/// community. Errors are collected and returned together at the end.
async fn close_every<F, Fut>(channels: &[String], mut close: F) -> Result<Value, PreviewError>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), PreviewError>>,
{
    let mut failures: Vec<String> = Vec::new();
    for channel_id in channels {
        if let Err(error) = close(channel_id.clone()).await {
            failures.push(format!("{channel_id}: {} ({})", error.message, error.code));
        }
    }
    if failures.is_empty() {
        return Ok(json!({ "closed": channels.len() }));
    }
    Err(PreviewError::new(
        "preview_unavailable",
        format!(
            "Closed {} of {} previews; these failed: {}",
            channels.len() - failures.len(),
            channels.len(),
            failures.join("; ")
        ),
    ))
}

/// Native placement facts (window, frame, hidden) for automated checks.
#[tauri::command]
pub async fn session_preview_debug_state(
    app: AppHandle,
    channel_id: String,
) -> Result<Value, PreviewError> {
    let channel_id = normalize_channel_id(&channel_id)?;
    view::debug_state(&app, &channel_id).await
}

#[cfg(test)]
mod tests {
    use super::{close_every, valid_slot_window, PreviewError};

    #[test]
    fn slots_live_only_in_the_main_window_or_a_session_pop_out() {
        assert!(valid_slot_window("main"));
        assert!(valid_slot_window("coding-session-abc_123"));
        assert!(!valid_slot_window("session-preview-x"));
        assert!(!valid_slot_window("artifact-preview-x"));
        assert!(!valid_slot_window("coding-session-a/b"));
        assert!(!valid_slot_window(""));
    }

    #[tokio::test]
    async fn close_all_continues_past_a_failure_and_reports_it() {
        let channels: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let mut attempted = Vec::new();
        let result = close_every(&channels, |channel| {
            attempted.push(channel.clone());
            async move {
                if channel == "a" {
                    Err(PreviewError::new("preview_unavailable", "main thread gone"))
                } else {
                    Ok(())
                }
            }
        })
        .await;
        assert_eq!(attempted, channels, "every preview is attempted");
        let error = result.expect_err("the failure is reported");
        assert!(error.message.contains("Closed 2 of 3"), "{}", error.message);
        assert!(
            error.message.contains("a: main thread gone"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn close_all_reports_the_count_when_every_close_succeeds() {
        let channels = vec!["a".to_string(), "b".to_string()];
        let result = close_every(&channels, |_| async { Ok(()) }).await;
        assert_eq!(result.ok(), Some(serde_json::json!({ "closed": 2 })));
    }
}
