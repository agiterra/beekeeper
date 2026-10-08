//! Native custody of the preview views.
//!
//! A wry `WebView` is a main-thread object, so every view lives in a
//! thread-local map on the main thread and is touched only from closures
//! posted there with `run_on_main_thread` ([`on_main`]) or from the window
//! and WebKit callbacks that already run there. The async functions in this
//! file are the only way the rest of the app reaches a view, and they keep
//! the view in step with the channel's [`PreviewRecord`]: which window it is
//! in, its bounds, whether it is hidden.
//!
//! One view per session, built once and reused: wry activates the app on
//! every child build (`wkwebview/mod.rs:690-698`), which would steal focus
//! from whatever the person was doing each time an agent opened a page.

use std::time::Duration;

use serde_json::Value;
use tauri::AppHandle;
use tokio::sync::oneshot;
use url::Url;

use super::{PreviewError, PreviewState};

/// Default budget for one main-thread round trip.
pub const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(10);

/// History navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOp {
    /// Back one entry.
    Back,
    /// Forward one entry.
    Forward,
    /// Reload.
    Reload,
}

/// Image encodings for [`snapshot`].
pub use imp::ImageEncoding;

/// An encoded picture of the viewport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// Encoded bytes.
    pub bytes: Vec<u8>,
    /// Pixels wide.
    pub width: u32,
    /// Pixels high.
    pub height: u32,
}

/// Run `f` on the main thread with the app handle and wait for its result.
pub async fn on_main<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(&AppHandle) -> T + Send + 'static,
) -> Result<T, PreviewError> {
    on_main_with(app, MAIN_THREAD_TIMEOUT, move |app, reply| {
        let _ = reply.send(f(app));
    })
    .await
}

/// Run `f` on the main thread; it answers through `reply`, possibly later
/// from a WebKit completion block. Times out with `preview_timeout`.
pub async fn on_main_with<T: Send + 'static>(
    app: &AppHandle,
    timeout: Duration,
    f: impl FnOnce(&AppHandle, oneshot::Sender<T>) + Send + 'static,
) -> Result<T, PreviewError> {
    let (tx, rx) = oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || f(&handle, tx))
        .map_err(|e| PreviewError::new("preview_unavailable", format!("main thread: {e}")))?;
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err(PreviewError::new(
            "preview_unavailable",
            "The preview went away while answering.",
        )),
        Err(_) => Err(PreviewError::timeout()),
    }
}

/// Open `url` in the channel's preview, creating the view if there is none.
/// When no slot is registered the view pops out (never headless); `agent`
/// additionally asks the UI to mount the Browser surface.
pub async fn open(
    app: &AppHandle,
    channel_id: &str,
    url: &Url,
    agent: bool,
) -> Result<PreviewState, PreviewError> {
    imp::open(app, channel_id, url, agent).await
}

/// Back, forward or reload.
pub async fn history(
    app: &AppHandle,
    channel_id: &str,
    op: HistoryOp,
) -> Result<PreviewState, PreviewError> {
    imp::history(app, channel_id, op).await
}

/// Destroy the view. `by_person` leaves the record `closed_by_person`.
pub async fn close(
    app: &AppHandle,
    channel_id: &str,
    by_person: bool,
) -> Result<PreviewState, PreviewError> {
    imp::close(app, channel_id, by_person).await
}

/// Make the native view match the record (window, bounds, visibility).
pub async fn sync_placement(app: &AppHandle, channel_id: &str) -> Result<(), PreviewError> {
    imp::sync_placement(app, channel_id).await
}

/// Hide the view and capture the freeze frame, or show it again.
pub async fn set_occluded(
    app: &AppHandle,
    channel_id: &str,
    occluded: bool,
) -> Result<PreviewState, PreviewError> {
    imp::set_occluded(app, channel_id, occluded).await
}

/// Pop the view out into its own window, or back into the slot.
pub async fn set_popped(
    app: &AppHandle,
    channel_id: &str,
    popped: bool,
) -> Result<PreviewState, PreviewError> {
    imp::set_popped(app, channel_id, popped).await
}

/// A picture of the viewport.
pub async fn snapshot(
    app: &AppHandle,
    channel_id: &str,
    encoding: ImageEncoding,
) -> Result<Picture, PreviewError> {
    imp::snapshot(app, channel_id, encoding).await
}

/// Run an async function body in a world of the page; see
/// `webkit_macos::call_async`.
pub async fn call_js(
    app: &AppHandle,
    channel_id: &str,
    body: String,
    payload: String,
    page_world: bool,
    timeout: Duration,
) -> Result<Option<String>, PreviewError> {
    imp::call_js(app, channel_id, body, payload, page_world, timeout).await
}

/// Native facts for the debug op: host window, frame, hidden.
pub async fn debug_state(app: &AppHandle, channel_id: &str) -> Result<Value, PreviewError> {
    imp::debug_state(app, channel_id).await
}

#[cfg(target_os = "macos")]
#[path = "view_macos.rs"]
mod imp;

#[cfg(not(target_os = "macos"))]
mod imp {
    //! Every op is unavailable off macOS; the state says so.
    use super::*;
    use crate::session_preview::{emit_state, with_record, PreviewStatus, Unavailable};

    /// Encodings (unused off macOS).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ImageEncoding {
        /// PNG.
        Png,
        /// JPEG.
        Jpeg,
    }

    fn unavailable<T>(app: &AppHandle, channel_id: &str) -> Result<T, PreviewError> {
        with_record(channel_id, |record| {
            record.status = PreviewStatus::Unavailable;
            record.unavailable = Some(Unavailable::NOT_MACOS);
        });
        emit_state(app, channel_id);
        Err(PreviewError::unavailable(&Unavailable::NOT_MACOS))
    }

    pub async fn open(
        app: &AppHandle,
        channel_id: &str,
        _url: &Url,
        _agent: bool,
    ) -> Result<PreviewState, PreviewError> {
        unavailable(app, channel_id)
    }
    pub async fn history(
        app: &AppHandle,
        channel_id: &str,
        _op: HistoryOp,
    ) -> Result<PreviewState, PreviewError> {
        unavailable(app, channel_id)
    }
    pub async fn close(
        app: &AppHandle,
        channel_id: &str,
        _by_person: bool,
    ) -> Result<PreviewState, PreviewError> {
        unavailable(app, channel_id)
    }
    pub async fn sync_placement(_app: &AppHandle, _channel_id: &str) -> Result<(), PreviewError> {
        Ok(())
    }
    pub async fn set_occluded(
        app: &AppHandle,
        channel_id: &str,
        _occluded: bool,
    ) -> Result<PreviewState, PreviewError> {
        unavailable(app, channel_id)
    }
    pub async fn set_popped(
        app: &AppHandle,
        channel_id: &str,
        _popped: bool,
    ) -> Result<PreviewState, PreviewError> {
        unavailable(app, channel_id)
    }
    pub async fn snapshot(
        app: &AppHandle,
        channel_id: &str,
        _encoding: ImageEncoding,
    ) -> Result<Picture, PreviewError> {
        unavailable(app, channel_id)
    }
    pub async fn call_js(
        app: &AppHandle,
        channel_id: &str,
        _body: String,
        _payload: String,
        _page_world: bool,
        _timeout: Duration,
    ) -> Result<Option<String>, PreviewError> {
        unavailable(app, channel_id)
    }
    pub async fn debug_state(app: &AppHandle, channel_id: &str) -> Result<Value, PreviewError> {
        unavailable(app, channel_id)
    }
}
