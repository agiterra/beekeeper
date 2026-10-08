//! `bee preview` on the session broker: the `preview` op (WIRE-C4 §2).
//!
//! The broker socket is owner-only, but that only says the caller runs as
//! this user. *Which session* a call speaks for comes from the preview grant
//! the provider minted for the calling execution, verified here against the
//! provider keys this app already trusts for transcripts
//! (`GlobalAgentConfig::allowed_bridge_pubkeys`) and addressed to this app's
//! identity. The grant names a channel and a session; the preview is found by
//! the channel, and may be driven only by the session it is bound to
//! ([`authorize`]).

use std::time::Duration;

use beekeeper_core_pkg::preview_grant::{
    preview_grant_audience, verify_preview_grant, PreviewGrantError, PreviewSessionBinding,
    VerifiedPreviewGrant,
};
use nostr::PublicKey;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter, Manager};

use super::driver::{self, OP_TIMEOUT, WAIT_FOR_DEFAULT, WAIT_FOR_MAX};
use super::policy::{self, PNG_CAP_BYTES};
use super::view::{self, HistoryOp, ImageEncoding};
use super::{
    emit_state, ports, refused_origin, unix_now, with_record, Binding, Driving, PreviewError,
    PreviewStatus, ACTIVITY_EVENT, DRIVING_EVENT, DRIVING_LINGER_SECS, OPEN_REQUESTED_EVENT,
};
use crate::session_broker::protocol::BrokerResponse;

/// Largest request line a preview op may arrive in.
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// One `bee preview` verb (WIRE-C4 §2 ops table).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "verb",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PreviewAction {
    /// The preview's state; works with nothing open.
    Status,
    /// Open a URL or a local port.
    Open {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        port: Option<u16>,
    },
    /// Go to a URL, or back/forward/reload.
    Navigate {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        back: bool,
        #[serde(default)]
        forward: bool,
        #[serde(default)]
        reload: bool,
    },
    /// Aria text and, by default, a PNG.
    Snapshot {
        #[serde(default)]
        image: Option<bool>,
    },
    /// Click a target.
    Click {
        target: Value,
        #[serde(default)]
        button: Option<String>,
        #[serde(default)]
        click_count: Option<u32>,
    },
    /// Type into a target.
    Type {
        target: Value,
        text: String,
        #[serde(default)]
        clear: bool,
    },
    /// Press a key, optionally on a target.
    Press {
        key: String,
        #[serde(default)]
        target: Option<Value>,
    },
    /// Scroll the page or a target.
    Scroll {
        #[serde(default)]
        target: Option<Value>,
        #[serde(default)]
        dx: Option<f64>,
        #[serde(default)]
        dy: Option<f64>,
        #[serde(default)]
        to: Option<String>,
    },
    /// Evaluate an expression.
    Eval {
        expression: String,
        #[serde(default)]
        world: Option<String>,
    },
    /// Wait for a condition.
    WaitFor {
        #[serde(default)]
        target: Option<Value>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        url_includes: Option<String>,
        #[serde(default)]
        state: Option<String>,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    /// This machine's listening loopback HTTP servers.
    Servers,
    /// Close the preview.
    Close,
    /// Native frame/visibility facts, for automated placement checks.
    DebugState,
}

impl PreviewAction {
    /// The verb, as the CLI names it.
    pub fn verb(&self) -> &'static str {
        match self {
            PreviewAction::Status => "status",
            PreviewAction::Open { .. } => "open",
            PreviewAction::Navigate { .. } => "navigate",
            PreviewAction::Snapshot { .. } => "snapshot",
            PreviewAction::Click { .. } => "click",
            PreviewAction::Type { .. } => "type",
            PreviewAction::Press { .. } => "press",
            PreviewAction::Scroll { .. } => "scroll",
            PreviewAction::Eval { .. } => "eval",
            PreviewAction::WaitFor { .. } => "wait_for",
            PreviewAction::Servers => "servers",
            PreviewAction::Close => "close",
            PreviewAction::DebugState => "debug_state",
        }
    }
}

/// What a grant is asking to do with a preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Open (or reopen) it.
    Open,
    /// Drive an open one.
    Drive,
}

/// Decide whether `grant` may act on a preview with this `status` and
/// `binding`, and what the binding becomes. `Ok(None)` = unchanged.
///
/// - An absent preview belongs to nobody: `open` creates it bound to the
///   grant's session; driving it is `preview_not_open` (the caller's check).
/// - Unbound (opened where there was no session): undriveable until an
///   agent `open`s, which binds it to that agent.
/// - Bound: core `check_binding` — same driver, instance and session, same
///   or newer generation. A sibling session in the same channel is refused
///   `preview_wrong_session`; a newer generation rebinds.
pub fn authorize(
    status: PreviewStatus,
    binding: &Binding,
    grant: &VerifiedPreviewGrant,
    access: Access,
) -> Result<Option<Binding>, PreviewGrantError> {
    let claims = grant.claims();
    let agent_binding = || Binding::Agent {
        target: claims.target.clone(),
        execution_id: claims.execution_id.clone(),
    };
    if status == PreviewStatus::Absent {
        return Ok(match access {
            Access::Open => Some(agent_binding()),
            Access::Drive => None,
        });
    }
    let Some(bound) = binding.target() else {
        return match access {
            Access::Open => Ok(Some(agent_binding())),
            Access::Drive => Err(PreviewGrantError::WrongSession(
                "the preview was opened where there was no session; it can be driven only \
                 after an agent opens its own"
                    .into(),
            )),
        };
    };
    grant.check_binding(&PreviewSessionBinding {
        channel_id: claims.channel_id,
        target: Some(bound.clone()),
    })?;
    if claims.target.generation > bound.generation {
        return Ok(Some(match binding {
            Binding::Person { .. } => Binding::Person {
                target: claims.target.clone(),
            },
            _ => agent_binding(),
        }));
    }
    Ok(None)
}

fn refusal(error: &PreviewError) -> BrokerResponse {
    let mut response = BrokerResponse::err(error.message.clone());
    response.code = Some(error.code.clone());
    response
}

fn grant_refusal(error: &PreviewGrantError) -> PreviewError {
    PreviewError::new(error.code(), grant_sentence(error))
}

/// The sentence for a grant refusal: what happened, in the agent's terms.
pub fn grant_sentence(error: &PreviewGrantError) -> String {
    match error {
        PreviewGrantError::Missing => "This execution has no preview grant \
             ($BEEKEEPER_PREVIEW_GRANT is unset), so it cannot drive a Browser preview."
            .to_string(),
        PreviewGrantError::WrongSession(why) => {
            format!("That preview belongs to a different session: {why}.")
        }
        other => format!("The preview grant was refused: {other}."),
    }
}

fn verify(app: &AppHandle, grant: Option<&str>) -> Result<VerifiedPreviewGrant, PreviewError> {
    let keys = app
        .state::<crate::app_state::AppState>()
        .signing_keys()
        .map_err(|_| {
            PreviewError::new(
                "preview_unavailable",
                "The Beekeeper app has no identity signed in, so it cannot check preview grants.",
            )
        })?;
    let audience = preview_grant_audience(&keys.public_key());
    let trusted: Vec<PublicKey> = crate::managed_agents::load_global_agent_config(app)
        .map(|config| {
            config
                .allowed_bridge_pubkeys
                .iter()
                .filter_map(|entry| PublicKey::from_hex(entry.pubkey.trim()).ok())
                .collect()
        })
        .unwrap_or_default();
    verify_preview_grant(grant, &trusted, &audience, unix_now()).map_err(|e| grant_refusal(&e))
}

/// Handle one `preview` request: verify, authorize, run, answer.
pub async fn handle(app: &AppHandle, grant: Option<String>, action: Value) -> BrokerResponse {
    let action: PreviewAction = match serde_json::from_value(action) {
        Ok(action) => action,
        Err(error) => return refusal(&PreviewError::bad_request(format!("{error}"))),
    };
    let verb = action.verb();
    let verified = match verify(app, grant.as_deref()) {
        Ok(verified) => verified,
        Err(error) => {
            eprintln!(
                "session-broker: op=preview verb={verb} refused={}",
                error.code
            );
            return refusal(&error);
        }
    };
    let claims = verified.claims();
    let channel_id = claims.channel_id.hyphenated().to_string();
    eprintln!(
        "session-broker: op=preview verb={verb} channel={channel_id} execution={}",
        claims.execution_id
    );
    match run(app, &verified, &channel_id, action).await {
        Ok(result) => BrokerResponse::ok(Value::Object(result)),
        Err(error) => refusal(&error),
    }
}

async fn run(
    app: &AppHandle,
    grant: &VerifiedPreviewGrant,
    channel_id: &str,
    action: PreviewAction,
) -> Result<Map<String, Value>, PreviewError> {
    let verb = action.verb();
    match action {
        PreviewAction::Status => to_map(json!(super::state_of(channel_id))),
        PreviewAction::Servers => {
            let refused_port = refused_origin(app).map(|origin| origin.port);
            let servers = ports::servers(refused_port)
                .await
                .map_err(|e| PreviewError::new("preview_unavailable", e))?;
            to_map(json!({ "servers": servers }))
        }
        PreviewAction::DebugState => to_map(view::debug_state(app, channel_id).await?),
        PreviewAction::Open { url, port } => {
            let url = open_target(app, url, port).await?;
            authorize_and_bind(channel_id, grant, Access::Open)?;
            let opened = async {
                let before = with_record(channel_id, |record| record.generation);
                let state = view::open(app, channel_id, &url, true).await?;
                await_commit(channel_id, before).await?;
                let mut out = common(channel_id);
                out.insert("opened".into(), json!(true));
                out.insert("placement".into(), json!(state.placement));
                Ok(out)
            };
            noting_success(opened, || note_activity(app, channel_id, grant, verb)).await
        }
        PreviewAction::Close => {
            authorize_and_bind(channel_id, grant, Access::Drive)?;
            ensure_drivable(channel_id)?;
            view::close(app, channel_id, false).await?;
            to_map(json!({ "closed": true }))
        }
        action => {
            authorize_and_bind(channel_id, grant, Access::Drive)?;
            ensure_drivable(channel_id)?;
            ensure_visible(app, channel_id).await?;
            noting_success(drive(app, channel_id, action), || {
                note_activity(app, channel_id, grant, verb)
            })
            .await
        }
    }
}

/// Await an agent op and record its activity only once it has succeeded.
///
/// A refused click or type must not light "Agent is driving" for the linger
/// window, so the note runs after the op, never before it.
async fn noting_success<T, F>(op: F, note: impl FnOnce()) -> Result<T, PreviewError>
where
    F: std::future::Future<Output = Result<T, PreviewError>>,
{
    let result = op.await;
    if result.is_ok() {
        note();
    }
    result
}

fn to_map(value: Value) -> Result<Map<String, Value>, PreviewError> {
    match value {
        Value::Object(map) => Ok(map),
        other => {
            let mut map = Map::new();
            map.insert("value".into(), other);
            Ok(map)
        }
    }
}

fn authorize_and_bind(
    channel_id: &str,
    grant: &VerifiedPreviewGrant,
    access: Access,
) -> Result<(), PreviewError> {
    with_record(channel_id, |record| {
        let rebind = authorize(record.status, &record.binding, grant, access)
            .map_err(|e| grant_refusal(&e))?;
        if let Some(binding) = rebind {
            record.binding = binding;
        }
        Ok(())
    })
}

fn ensure_drivable(channel_id: &str) -> Result<(), PreviewError> {
    with_record(channel_id, |record| match record.status {
        PreviewStatus::Absent => Err(PreviewError::not_open()),
        PreviewStatus::ClosedByPerson => Err(PreviewError::closed_by_person()),
        PreviewStatus::Unavailable => Err(match &record.unavailable {
            Some(reason) => PreviewError::unavailable(reason),
            None => PreviewError::not_open(),
        }),
        PreviewStatus::Loading | PreviewStatus::Ready if !record.has_view => {
            Err(PreviewError::not_open())
        }
        PreviewStatus::Loading | PreviewStatus::Ready => Ok(()),
    })
}

/// Visible only: an agent op on a preview drawn nowhere pops it out first.
async fn ensure_visible(app: &AppHandle, channel_id: &str) -> Result<(), PreviewError> {
    let (unplaced, url) = with_record(channel_id, |record| {
        (
            record.placement() == super::PreviewPlacement::None,
            record.url.clone(),
        )
    });
    if unplaced {
        view::set_popped(app, channel_id, true).await?;
        with_record(channel_id, |record| record.auto_popped = true);
        let _ = app.emit(
            OPEN_REQUESTED_EVENT,
            json!({ "channelId": channel_id, "url": url }),
        );
    }
    Ok(())
}

async fn open_target(
    app: &AppHandle,
    url: Option<String>,
    port: Option<u16>,
) -> Result<url::Url, PreviewError> {
    let refused = refused_origin(app);
    let raw = match (url, port) {
        (Some(url), None) => url,
        (None, Some(port)) => {
            let known = ports::servers(refused.as_ref().map(|origin| origin.port))
                .await
                .unwrap_or_default()
                .into_iter()
                .find(|server| server.port == port);
            known
                .map(|server| server.url)
                .unwrap_or_else(|| format!("http://localhost:{port}/"))
        }
        _ => {
            return Err(PreviewError::bad_request(
                "open takes exactly one of url and port.",
            ))
        }
    };
    Ok(policy::check_preview_url(&raw, refused.as_ref())?)
}

/// Wait until a navigation started after `before` has committed.
async fn await_commit(channel_id: &str, before: u64) -> Result<(), PreviewError> {
    let deadline = tokio::time::Instant::now() + OP_TIMEOUT;
    loop {
        let (generation, status) =
            with_record(channel_id, |record| (record.generation, record.status));
        if generation > before {
            return Ok(());
        }
        if matches!(
            status,
            PreviewStatus::Absent | PreviewStatus::ClosedByPerson | PreviewStatus::Unavailable
        ) {
            return Err(PreviewError::not_open());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(PreviewError::timeout());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// `input`, `url`, `generation`: on every driving result.
fn common(channel_id: &str) -> Map<String, Value> {
    let (url, generation) =
        with_record(channel_id, |record| (record.url.clone(), record.generation));
    let mut out = Map::new();
    out.insert("input".into(), json!("synthetic"));
    out.insert("url".into(), json!(url));
    out.insert("generation".into(), json!(generation));
    out
}

fn op_map(verb: &str, fields: Value) -> Map<String, Value> {
    let mut map = match fields {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    map.insert("verb".into(), json!(verb));
    map.retain(|_, value| !value.is_null());
    map
}

fn require_target(target: &Value) -> Result<(), PreviewError> {
    if target.is_object() {
        Ok(())
    } else {
        Err(PreviewError::bad_request(
            "target must be a locator object.",
        ))
    }
}

async fn drive(
    app: &AppHandle,
    channel_id: &str,
    action: PreviewAction,
) -> Result<Map<String, Value>, PreviewError> {
    let result = match action {
        PreviewAction::Navigate {
            url,
            back,
            forward,
            reload,
        } => {
            let chosen = [url.is_some(), back, forward, reload]
                .iter()
                .filter(|chosen| **chosen)
                .count();
            if chosen != 1 {
                return Err(PreviewError::bad_request(
                    "navigate takes exactly one of url, back, forward and reload.",
                ));
            }
            let (before, can_back, can_forward) = with_record(channel_id, |record| {
                (record.generation, record.can_go_back, record.can_go_forward)
            });
            if let Some(url) = url {
                let url = policy::check_preview_url(&url, refused_origin(app).as_ref())?;
                view::open(app, channel_id, &url, true).await?;
            } else if back {
                if !can_back {
                    return Err(PreviewError::bad_request("There is no page to go back to."));
                }
                view::history(app, channel_id, HistoryOp::Back).await?;
            } else if forward {
                if !can_forward {
                    return Err(PreviewError::bad_request(
                        "There is no page to go forward to.",
                    ));
                }
                view::history(app, channel_id, HistoryOp::Forward).await?;
            } else {
                view::history(app, channel_id, HistoryOp::Reload).await?;
            }
            await_commit(channel_id, before).await?;
            Map::new()
        }
        PreviewAction::Snapshot { image } => {
            snapshot(app, channel_id, image.unwrap_or(true)).await?
        }
        PreviewAction::Click {
            target,
            button,
            click_count,
        } => {
            require_target(&target)?;
            let op = op_map(
                "click",
                json!({ "target": target, "button": button, "clickCount": click_count }),
            );
            driver::run_op(app, channel_id, op, OP_TIMEOUT).await?
        }
        PreviewAction::Type {
            target,
            text,
            clear,
        } => {
            require_target(&target)?;
            let op = op_map(
                "type",
                json!({ "target": target, "text": text, "clear": clear }),
            );
            driver::run_op(app, channel_id, op, OP_TIMEOUT).await?
        }
        PreviewAction::Press { key, target } => {
            if let Some(target) = &target {
                require_target(target)?;
            }
            let op = op_map("press", json!({ "key": key, "target": target }));
            driver::run_op(app, channel_id, op, OP_TIMEOUT).await?
        }
        PreviewAction::Scroll { target, dx, dy, to } => {
            if let Some(target) = &target {
                require_target(target)?;
            }
            let op = op_map(
                "scroll",
                json!({ "target": target, "dx": dx, "dy": dy, "to": to }),
            );
            driver::run_op(app, channel_id, op, OP_TIMEOUT).await?
        }
        PreviewAction::Eval { expression, world } => {
            let page_world = match world.as_deref() {
                None | Some("driver") => false,
                Some("page") => true,
                Some(other) => {
                    return Err(PreviewError::bad_request(format!(
                        "Unknown world {other}; use driver or page."
                    )))
                }
            };
            driver::eval(app, channel_id, &expression, page_world).await?
        }
        PreviewAction::WaitFor {
            target,
            text,
            url_includes,
            state,
            timeout_ms,
        } => {
            if let Some(target) = &target {
                require_target(target)?;
            }
            if let Some(state) = state.as_deref() {
                if !matches!(state, "visible" | "hidden" | "attached" | "detached") {
                    return Err(PreviewError::bad_request(format!("Unknown state {state}.")));
                }
            }
            let timeout = timeout_ms
                .map(Duration::from_millis)
                .unwrap_or(WAIT_FOR_DEFAULT)
                .min(WAIT_FOR_MAX);
            let op = op_map(
                "check",
                json!({ "target": target, "text": text, "urlIncludes": url_includes, "state": state }),
            );
            driver::wait_for(app, channel_id, op, timeout).await?
        }
        PreviewAction::Status
        | PreviewAction::Servers
        | PreviewAction::DebugState
        | PreviewAction::Open { .. }
        | PreviewAction::Close => Map::new(),
    };
    let mut out = common(channel_id);
    out.extend(result);
    Ok(out)
}

async fn snapshot(
    app: &AppHandle,
    channel_id: &str,
    image: bool,
) -> Result<Map<String, Value>, PreviewError> {
    let mut aria =
        driver::run_op(app, channel_id, op_map("snapshot", json!({})), OP_TIMEOUT).await?;
    let text = aria
        .remove("aria")
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    let (text, truncated) = driver::cap_aria(&text);
    let mut out = Map::new();
    out.insert("title".into(), aria.remove("title").unwrap_or(Value::Null));
    out.insert("ariaBytes".into(), json!(text.len()));
    out.insert("aria".into(), json!(text));
    out.insert("ariaTruncated".into(), json!(truncated));
    let png = if image {
        let picture = view::snapshot(app, channel_id, ImageEncoding::Png).await?;
        if picture.bytes.len() > PNG_CAP_BYTES {
            out.insert("pngOmitted".into(), json!("too_large"));
            Value::Null
        } else {
            use base64::Engine as _;
            json!({
                "base64": base64::engine::general_purpose::STANDARD.encode(&picture.bytes),
                "width": picture.width,
                "height": picture.height,
                "bytes": picture.bytes.len(),
            })
        }
    } else {
        Value::Null
    };
    out.insert("png".into(), png);
    Ok(out)
}

/// Record an agent op: activity event, driving state, and the timer that
/// ends "driving" [`DRIVING_LINGER_SECS`] after the last op.
fn note_activity(app: &AppHandle, channel_id: &str, grant: &VerifiedPreviewGrant, op: &str) {
    let claims = grant.claims();
    let at = unix_now();
    let (started, seq) = with_record(channel_id, |record| {
        let started = record.driving.is_none();
        record.driving_seq += 1;
        record.driving = Some(Driving {
            execution_id: claims.execution_id.clone(),
            session_id: claims.target.session_id.clone(),
            last_op: op.to_string(),
            at,
        });
        (started, record.driving_seq)
    });
    let who = json!({
        "channelId": channel_id,
        "executionId": claims.execution_id,
        "sessionId": claims.target.session_id,
        "at": at,
    });
    let mut activity = who.clone();
    activity["op"] = json!(op);
    let _ = app.emit(ACTIVITY_EVENT, activity);
    if started {
        let mut driving = who.clone();
        driving["driving"] = json!(true);
        let _ = app.emit(DRIVING_EVENT, driving);
    }
    emit_state(app, channel_id);
    let app = app.clone();
    let channel = channel_id.to_string();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(DRIVING_LINGER_SECS)).await;
        let stopped = with_record(&channel, |record| {
            if record.driving_seq == seq && record.driving.is_some() {
                record.driving = None;
                true
            } else {
                false
            }
        });
        if stopped {
            let mut driving = who;
            driving["driving"] = json!(false);
            driving["at"] = json!(unix_now());
            let _ = app.emit(DRIVING_EVENT, driving);
            emit_state(&app, &channel);
        }
    });
}

#[cfg(test)]
#[path = "broker_tests.rs"]
mod tests;
