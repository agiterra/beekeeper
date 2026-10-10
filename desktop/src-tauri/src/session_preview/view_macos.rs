//! macOS custody: wry child WKWebViews, one per session channel.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use base64::Engine as _;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::NSWindow;
use objc2_web_kit::WKContentRuleList;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tokio::sync::oneshot;
use url::Url;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{
    NewWindowResponse, PageLoadEvent, Rect, WebViewBuilder, WebViewBuilderExtDarwin,
    WebViewExtMacOS,
};

use super::{on_main, on_main_with, HistoryOp, Picture, MAIN_THREAD_TIMEOUT};
use crate::session_preview::geometry::{self, Placement, SlotRect};
use crate::session_preview::snapshot_macos;
use crate::session_preview::webkit_macos::{self as webkit, World};
use crate::session_preview::{
    driver, emit_state, policy, popout_label, refused_origin, with_record, PreviewError,
    PreviewRecord, PreviewState, PreviewStatus, Unavailable, OPEN_REQUESTED_EVENT,
};

/// Image encodings for snapshots.
pub use snapshot_macos::Encoding as ImageEncoding;

struct NativeView {
    view: wry::WebView,
    /// Label of the Tauri window whose content view holds it.
    host: String,
}

thread_local! {
    static VIEWS: RefCell<HashMap<String, NativeView>> = RefCell::new(HashMap::new());
    static RULE_LIST: RefCell<Option<Retained<WKContentRuleList>>> = const { RefCell::new(None) };
}

fn rect(r: SlotRect) -> Rect {
    Rect {
        position: LogicalPosition::new(r.x, r.y).into(),
        size: LogicalSize::new(r.width, r.height).into(),
    }
}

/// The size a page lays out at while it has never been placed: hidden
/// pages are driven and snapshotted, so they need a real viewport, not 1×1.
const HIDDEN_VIEWPORT: SlotRect = SlotRect {
    x: 0.0,
    y: 0.0,
    width: 1280.0,
    height: 800.0,
};

/// The window a view is built in: where the record places it, else the
/// window its slot last lived in, else the main window (a hidden page still
/// needs a parent view; it is not drawn there).
fn build_host(app: &AppHandle, record: &PreviewRecord) -> Option<String> {
    [
        target_host(record),
        record.slot_window.clone(),
        Some("main".to_string()),
    ]
    .into_iter()
    .flatten()
    .find(|label| app.get_webview_window(label).is_some())
}

/// The window the record says the view belongs in, if any.
fn target_host(record: &PreviewRecord) -> Option<String> {
    if record.popped {
        Some(popout_label(&record.channel_id))
    } else if record.slot.is_some() {
        record.slot_window.clone()
    } else {
        None
    }
}

/// A full Safari user agent: dev servers and the libraries they serve sniff
/// for `Version/… Safari/…`, which a bare WKWebView does not send. Safari's
/// major version tracks macOS from macOS 26 and ran three ahead before it.
fn user_agent() -> String {
    let os = webkit::os_major_version();
    let safari = if os >= 26 { os } else { os + 3 };
    format!(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) Version/{safari}.0 Safari/605.1.15"
    )
}

async fn ensure_rule_list(app: &AppHandle) -> Result<(), String> {
    let compiled = on_main_with(app, MAIN_THREAD_TIMEOUT, |_, reply| {
        if RULE_LIST.with(|list| list.borrow().is_some()) {
            let _ = reply.send(Ok(()));
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = reply.send(Err("not on the main thread".to_string()));
            return;
        };
        let source = policy::content_rule_list_json();
        webkit::compile_rule_list(mtm, policy::CONTENT_RULE_LIST_ID, &source, move |result| {
            let _ = reply.send(result.map(|list| {
                RULE_LIST.with(|slot| *slot.borrow_mut() = Some(list));
            }));
        });
    })
    .await;
    compiled.unwrap_or_else(|error| Err(error.message))
}

fn mark_unavailable(app: &AppHandle, channel_id: &str, reason: Unavailable) -> PreviewError {
    let error = PreviewError::unavailable(&reason);
    with_record(channel_id, |record| {
        record.status = PreviewStatus::Unavailable;
        record.unavailable = Some(reason);
    });
    emit_state(app, channel_id);
    error
}

async fn ensure_popout_window(app: &AppHandle, channel_id: &str) -> Result<(), PreviewError> {
    let label = popout_label(channel_id);
    if app.get_webview_window(&label).is_some() {
        return Ok(());
    }
    let blank = Url::parse("about:blank").map_err(|e| PreviewError::bad_request(e.to_string()))?;
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(blank))
        .title("Browser")
        .inner_size(1024.0, 720.0)
        .min_inner_size(320.0, 240.0)
        .build()
        .map_err(|e| {
            eprintln!("session-preview: pop-out window failed: {e}");
            PreviewError::unavailable(&Unavailable::WEBVIEW_FAILED)
        })?;
    let events_app = app.clone();
    let channel = channel_id.to_string();
    // Window events arrive on the main thread, so they may touch the views
    // directly.
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            apply_now(&events_app, &channel);
        }
        WindowEvent::CloseRequested { .. } => popout_closing(&events_app, &channel),
        _ => {}
    });
    Ok(())
}

/// The pop-out window is closing. If the person closed it, the preview docks
/// back into its slot, or is closed by the person when there is no slot. If
/// the app closed it (`set_popped(false)`, `close`), the view has already
/// left it.
fn popout_closing(app: &AppHandle, channel_id: &str) {
    let (person, has_slot) = with_record(channel_id, |record| {
        (
            record.popped,
            record.slot.is_some() && record.slot_window.is_some(),
        )
    });
    if !person {
        return;
    }
    if has_slot {
        with_record(channel_id, |record| {
            record.popped = false;
            record.auto_popped = false;
        });
        apply_now(app, channel_id);
    } else {
        drop_view(channel_id);
        with_record(channel_id, |record| {
            reset_closed(record);
            record.status = PreviewStatus::ClosedByPerson;
        });
    }
    emit_state(app, channel_id);
}

fn reset_closed(record: &mut PreviewRecord) {
    record.has_view = false;
    record.popped = false;
    record.auto_popped = false;
    record.hidden = true;
    record.url = None;
    record.title = None;
    record.can_go_back = false;
    record.can_go_forward = false;
    record.freeze_frame = None;
    record.driving = None;
    record.ever_shown = false;
}

fn drop_view(channel_id: &str) {
    let removed = VIEWS.with(|views| {
        views
            .try_borrow_mut()
            .ok()
            .and_then(|mut views| views.remove(channel_id))
    });
    // Dropped outside the borrow: wry's drop removes it from its superview.
    drop(removed);
}

/// Bring the native view in line with the record. Main thread only.
fn apply_now(app: &AppHandle, channel_id: &str) {
    let record = with_record(channel_id, |record| record.clone());
    let target = target_host(&record);
    let visible = VIEWS.with(|views| {
        let Ok(mut views) = views.try_borrow_mut() else {
            return None;
        };
        let native = views.get_mut(channel_id)?;
        let hide = |native: &NativeView| {
            let _ = native.view.set_visible(false);
            false
        };
        let Some(target) = target else {
            return Some(hide(native));
        };
        let Some(window) = app.get_webview_window(&target) else {
            return Some(hide(native));
        };
        if native.host != target {
            match window.ns_window() {
                Ok(ns_window) => {
                    if let Err(error) = native.view.reparent(ns_window.cast::<NSWindow>()) {
                        eprintln!("session-preview: reparent failed: {error}");
                        return Some(hide(native));
                    }
                    native.host = target.clone();
                }
                Err(_) => return Some(hide(native)),
            }
        }
        let placement = if record.popped {
            let scale = window.scale_factor().unwrap_or(1.0);
            match window.inner_size() {
                Ok(size) => {
                    let size = size.to_logical::<f64>(scale);
                    Ok(Placement::Show(SlotRect {
                        x: 0.0,
                        y: 0.0,
                        width: size.width,
                        height: size.height,
                    }))
                }
                Err(e) => Err(geometry::GeometryError(e.to_string())),
            }
        } else {
            match record.slot {
                Some(slot) => geometry::view_bounds(slot),
                None => Ok(Placement::Collapse),
            }
        };
        let show = match placement {
            Ok(Placement::Show(bounds)) => {
                // wry's set_bounds unwraps the view's window; a view whose
                // window went away is hidden instead.
                if native.view.webview().window().is_none() {
                    return Some(hide(native));
                }
                let _ = native.view.set_bounds(rect(bounds));
                // An overlay in the slot window does not cover a pop-out.
                record.popped || !record.occluded
            }
            Ok(Placement::Collapse) => false,
            Err(error) => {
                eprintln!("session-preview: {}", error.0);
                false
            }
        };
        let _ = native.view.set_visible(show);
        Some(show)
    });
    if let Some(visible) = visible {
        with_record(channel_id, |record| {
            record.hidden = !visible;
            record.ever_shown |= visible;
        });
    }
}

/// Give keyboard focus back to the hosting window's own webview, so keys
/// typed into an overlay do not go to a hidden preview.
fn focus_host_webview(app: &AppHandle, host: Option<String>) {
    if let Some(window) = host.and_then(|label| app.get_webview_window(&label)) {
        let webview: &tauri::Webview = window.as_ref();
        let _ = webview.set_focus();
    }
}

fn page_load(app: &AppHandle, channel_id: &str, event: PageLoadEvent, url: String) {
    let facts = VIEWS.with(|views| {
        let views = views.try_borrow().ok()?;
        let webview = views.get(channel_id)?.view.webview();
        Some((webkit::history(&webview), webkit::title(&webview)))
    });
    let started = matches!(event, PageLoadEvent::Started);
    with_record(channel_id, |record| {
        if started {
            record.generation += 1;
            record.status = PreviewStatus::Loading;
        } else {
            record.status = PreviewStatus::Ready;
        }
        if !url.is_empty() {
            record.url = Some(url);
        }
        if let Some(((back, forward), title)) = facts {
            record.can_go_back = back;
            record.can_go_forward = forward;
            if title.is_some() || started {
                record.title = title;
            }
        }
    });
    emit_state(app, channel_id);
}

fn build(app: &AppHandle, channel_id: &str) -> Result<(), String> {
    let record = with_record(channel_id, |record| record.clone());
    let host = build_host(app, &record).ok_or("there is no window to host the preview in")?;
    let window = app
        .get_webview_window(&host)
        .ok_or_else(|| format!("window {host} is gone"))?;
    let rule_list = RULE_LIST
        .with(|list| list.borrow().clone())
        .ok_or("the local-only filter is not compiled")?;
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let refused = refused_origin(app);
    let (load_app, title_app) = (app.clone(), app.clone());
    let (load_channel, title_channel, nav_channel) = (
        channel_id.to_string(),
        channel_id.to_string(),
        channel_id.to_string(),
    );
    let per_session = webkit::os_major_version() >= 14;
    let builder = WebViewBuilder::new()
        .with_visible(false)
        .with_focused(false)
        .with_bounds(rect(HIDDEN_VIEWPORT))
        .with_devtools(tauri::is_dev())
        .with_back_forward_navigation_gestures(true)
        .with_user_agent(user_agent())
        .with_navigation_handler(move |url| {
            let allowed = policy::navigation_allowed(&url, refused.as_ref());
            if !allowed {
                eprintln!("session-preview: {nav_channel}: blocked navigation to {url}");
            }
            allowed
        })
        .with_new_window_req_handler(|url, _| {
            eprintln!("session-preview: denied a new window for {url}");
            NewWindowResponse::Deny
        })
        .with_download_started_handler(|_, _| false)
        .with_on_page_load_handler(move |event, url| {
            page_load(&load_app, &load_channel, event, url)
        })
        .with_document_title_changed_handler(move |title| {
            with_record(&title_channel, |record| {
                record.title = (!title.is_empty()).then_some(title);
            });
            emit_state(&title_app, &title_channel);
        });
    let builder = if per_session {
        builder.with_data_store_identifier(policy::data_store_id("", channel_id))
    } else {
        builder.with_incognito(true)
    };
    let view = builder.build_as_child(&window).map_err(|e| e.to_string())?;
    let manager = view.manager();
    webkit::add_rule_list(&manager, &rule_list);
    webkit::add_driver_script(mtm, &manager, driver::driver_source());
    if tauri::is_dev() {
        webkit::set_inspectable(&view.webview(), true);
    }
    VIEWS.with(|views| {
        views
            .borrow_mut()
            .insert(channel_id.to_string(), NativeView { view, host })
    });
    with_record(channel_id, |record| {
        record.has_view = true;
        record.data_store = if per_session {
            "per_session"
        } else {
            "incognito"
        };
    });
    Ok(())
}

pub async fn open(
    app: &AppHandle,
    channel_id: &str,
    url: &Url,
    agent: bool,
) -> Result<PreviewState, PreviewError> {
    let (has_view, unplaced) = with_record(channel_id, |record| {
        (
            record.has_view,
            record.popped || record.slot.is_none() || record.slot_window.is_none(),
        )
    });
    let popped = with_record(channel_id, |record| record.popped);
    if unplaced && agent {
        // An agent never pops a window (ledger 371(c)): a person-chosen
        // pop-out stays where it is; otherwise the page opens hidden and the
        // session's view is asked to show its Browser tab. If no view of the
        // session is on screen it stays hidden, and says so (placement
        // `hidden`, the Browser tab's badge).
        if popped {
            ensure_popout_window(app, channel_id).await?;
        } else {
            let payload = json!({ "channelId": channel_id, "url": url.as_str() });
            let _ = app.emit(OPEN_REQUESTED_EVENT, payload);
        }
    } else if unplaced {
        // The person opened it with no slot to dock into: it pops out.
        with_record(channel_id, |record| {
            if !record.popped {
                record.popped = true;
                record.auto_popped = true;
            }
        });
        ensure_popout_window(app, channel_id).await?;
    }
    if !has_view {
        if let Err(error) = ensure_rule_list(app).await {
            eprintln!("session-preview: content rule list did not compile: {error}");
            return Err(mark_unavailable(
                app,
                channel_id,
                Unavailable::CONTENT_FILTER_FAILED,
            ));
        }
        let channel = channel_id.to_string();
        let built = on_main(app, move |app| build(app, &channel)).await?;
        if let Err(error) = built {
            eprintln!("session-preview: web view failed: {error}");
            return Err(mark_unavailable(
                app,
                channel_id,
                Unavailable::WEBVIEW_FAILED,
            ));
        }
    }
    with_record(channel_id, |record| {
        record.status = PreviewStatus::Loading;
        record.unavailable = None;
        record.closed_reason = None;
        record.url = Some(url.to_string());
    });
    let channel = channel_id.to_string();
    let target = url.to_string();
    let loaded = on_main(app, move |app| {
        let result = VIEWS.with(|views| {
            let views = views.try_borrow().map_err(|e| e.to_string())?;
            let native = views.get(&channel).ok_or("the preview is gone")?;
            native.view.load_url(&target).map_err(|e| e.to_string())
        });
        apply_now(app, &channel);
        result
    })
    .await?;
    loaded.map_err(|e| PreviewError::new("preview_unavailable", e))?;
    Ok(emit_state(app, channel_id))
}

pub async fn history(
    app: &AppHandle,
    channel_id: &str,
    op: HistoryOp,
) -> Result<PreviewState, PreviewError> {
    let channel = channel_id.to_string();
    let done = on_main(app, move |_| {
        VIEWS.with(|views| {
            let views = views.try_borrow().ok()?;
            let native = views.get(&channel)?;
            let webview = native.view.webview();
            match op {
                HistoryOp::Back => webkit::go_back(&webview),
                HistoryOp::Forward => webkit::go_forward(&webview),
                HistoryOp::Reload => {
                    let _ = native.view.reload();
                }
            }
            Some(())
        })
    })
    .await?;
    done.ok_or_else(PreviewError::not_open)?;
    Ok(emit_state(app, channel_id))
}

pub async fn close(
    app: &AppHandle,
    channel_id: &str,
    by_person: bool,
) -> Result<PreviewState, PreviewError> {
    with_record(channel_id, |record| {
        reset_closed(record);
        record.closed_reason = None;
        record.status = if by_person {
            PreviewStatus::ClosedByPerson
        } else {
            PreviewStatus::Absent
        };
    });
    let channel = channel_id.to_string();
    let dropped = on_main(app, move |_| drop_view(&channel)).await;
    // The pop-out closes even when the view could not be dropped: a window
    // must not outlive the preview it hosted.
    if let Some(window) = app.get_webview_window(&popout_label(channel_id)) {
        let _ = window.close();
    }
    dropped?;
    Ok(emit_state(app, channel_id))
}

pub async fn sync_placement(app: &AppHandle, channel_id: &str) -> Result<(), PreviewError> {
    let channel = channel_id.to_string();
    on_main(app, move |app| apply_now(app, &channel)).await
}

pub async fn set_occluded(
    app: &AppHandle,
    channel_id: &str,
    occluded: bool,
) -> Result<PreviewState, PreviewError> {
    let (became_occluded, host) = with_record(channel_id, |record| {
        let became = occluded && !record.occluded && record.has_view && !record.hidden;
        record.occluded = occluded;
        if !occluded {
            record.freeze_frame = None;
        }
        (became && !record.popped, record.slot_window.clone())
    });
    let channel = channel_id.to_string();
    // Hide first, then take the picture: the overlay is never drawn under
    // the view, and WebKit snapshots a hidden view fine.
    on_main(app, move |app| {
        apply_now(app, &channel);
        if became_occluded {
            focus_host_webview(app, host);
        }
    })
    .await?;
    if became_occluded {
        match snapshot(app, channel_id, ImageEncoding::Jpeg).await {
            Ok(picture) => {
                let data = base64::engine::general_purpose::STANDARD.encode(&picture.bytes);
                with_record(channel_id, |record| {
                    if record.occluded {
                        record.freeze_frame = Some(format!("data:image/jpeg;base64,{data}"));
                    }
                });
            }
            Err(error) => eprintln!("session-preview: freeze frame failed: {}", error.message),
        }
    }
    Ok(emit_state(app, channel_id))
}

pub async fn set_popped(
    app: &AppHandle,
    channel_id: &str,
    popped: bool,
) -> Result<PreviewState, PreviewError> {
    if popped {
        with_record(channel_id, |record| {
            record.popped = true;
            record.auto_popped = false;
        });
        ensure_popout_window(app, channel_id).await?;
        sync_placement(app, channel_id).await?;
    } else {
        let has_slot = with_record(channel_id, |record| {
            record.slot.is_some() && record.slot_window.is_some()
        });
        if !has_slot {
            return Err(PreviewError::bad_request(
                "There is no Browser slot to dock the preview into.",
            ));
        }
        with_record(channel_id, |record| {
            record.popped = false;
            record.auto_popped = false;
        });
        sync_placement(app, channel_id).await?;
        if let Some(window) = app.get_webview_window(&popout_label(channel_id)) {
            let _ = window.close();
        }
    }
    Ok(emit_state(app, channel_id))
}

pub async fn snapshot(
    app: &AppHandle,
    channel_id: &str,
    encoding: ImageEncoding,
) -> Result<Picture, PreviewError> {
    let channel = channel_id.to_string();
    let result = on_main_with(app, MAIN_THREAD_TIMEOUT, move |_, reply| {
        let started = VIEWS.with(|views| {
            let Ok(views) = views.try_borrow() else {
                return Err(reply);
            };
            let Some(native) = views.get(&channel) else {
                return Err(reply);
            };
            let webview = native.view.webview();
            snapshot_macos::take(&webview, encoding, move |result| {
                let _ = reply.send(Some(result));
            });
            Ok(())
        });
        if let Err(reply) = started {
            let _ = reply.send(None);
        }
    })
    .await?;
    match result {
        None => Err(PreviewError::not_open()),
        Some(Ok(snapshot)) => Ok(Picture {
            bytes: snapshot.bytes,
            width: snapshot.width,
            height: snapshot.height,
        }),
        Some(Err(error)) => Err(PreviewError::new(
            "preview_unavailable",
            format!("The snapshot failed: {error}"),
        )),
    }
}

pub async fn call_js(
    app: &AppHandle,
    channel_id: &str,
    body: String,
    payload: String,
    page_world: bool,
    timeout: Duration,
) -> Result<Option<String>, PreviewError> {
    let channel = channel_id.to_string();
    type Reply = oneshot::Sender<Option<Result<Option<String>, String>>>;
    let result = on_main_with(app, timeout, move |_, reply: Reply| {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = reply.send(None);
            return;
        };
        let started = VIEWS.with(|views| {
            let Ok(views) = views.try_borrow() else {
                return Err(reply);
            };
            let Some(native) = views.get(&channel) else {
                return Err(reply);
            };
            let webview = native.view.webview();
            let world = if page_world {
                World::Page
            } else {
                World::Driver
            };
            webkit::call_async(mtm, &webview, &body, &payload, world, move |result| {
                let _ = reply.send(Some(result));
            });
            Ok(())
        });
        if let Err(reply) = started {
            let _ = reply.send(None);
        }
    })
    .await?;
    match result {
        None => Err(PreviewError::not_open()),
        Some(Ok(value)) => Ok(value),
        Some(Err(message)) => Err(PreviewError::new(
            "preview_eval_error",
            format!("The script threw: {message}"),
        )),
    }
}

pub async fn debug_state(app: &AppHandle, channel_id: &str) -> Result<Value, PreviewError> {
    let channel = channel_id.to_string();
    let native = on_main(app, move |app| {
        VIEWS.with(|views| {
            let views = views.try_borrow().ok()?;
            let native = views.get(&channel)?;
            let webview = native.view.webview();
            let scale = app
                .get_webview_window(&native.host)
                .and_then(|window| window.scale_factor().ok())
                .unwrap_or(1.0);
            let frame = native.view.bounds().ok().map(|bounds| {
                let position = bounds.position.to_logical::<f64>(scale);
                let size = bounds.size.to_logical::<f64>(scale);
                json!({ "x": position.x, "y": position.y, "width": size.width, "height": size.height })
            });
            Some(json!({
                "windowLabel": native.host,
                "frame": frame,
                "nativeHidden": webview.isHidden(),
                "url": webkit::current_url(&webview),
            }))
        })
    })
    .await?;
    let record = with_record(channel_id, |record| record.clone());
    Ok(json!({
        "native": native,
        "slot": record.slot,
        "slotWindow": record.slot_window,
        "popped": record.popped,
        "hidden": record.hidden,
        "occluded": record.occluded,
        "dataStore": record.data_store,
        "inset": geometry::SLOT_INSET,
    }))
}
