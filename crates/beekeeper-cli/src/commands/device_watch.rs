//! `bee device watch` — the second-client probe (WIRE-C5 § 3, § 4).
//!
//! Resolves the surface's producer authority the way the relay and the
//! desktop do, opens one authenticated connection, subscribes to live frames
//! from that authority only, keeps a watch alive on the desktop's timing
//! (watch every 15 s, `resync` when no frame arrived since the last beat),
//! and prints one JSON line per frame and per snapshot, then a summary.

use std::time::Duration;

use base64::Engine as _;
use beekeeper_core::kind::{
    KIND_SESSION_PREVIEW_ANNOUNCE, KIND_SURFACE_FRAME, KIND_SURFACE_SNAPSHOT,
};
use beekeeper_core::session_device::newest_device_state_record;
use beekeeper_core::surface_snapshot::{
    snapshot_evidence_token, validate_surface_snapshot_envelope,
};
use beekeeper_core::surface_watch::{
    validate_surface_frame_envelope, Surface, SurfaceFrameHeader, SurfaceFrameType, SurfaceWatch,
    SurfaceWatchAction, SURFACE_WATCH_KEEPALIVE_MS,
};
use beekeeper_ws_client::{NostrWsConnection, RelayMessage, WsClientError};
use nostr::{Event, PublicKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::support::{default_open_slot, events_from_rows, DeviceContext};
use super::{closed_error, query_device_records, ws_error, SurfaceArg};
use crate::client::BeekeeperClient;
use crate::commands::preview::share::{pick_session_ref, PickError};
use crate::error::CliError;

const SUBSCRIPTION_ID: &str = "bee-device-watch";

/// The honest limit of this probe, printed verbatim in every summary.
pub const SAME_MACHINE_NOTE: &str = "same machine, two identities — not cross-machine proof";

/// What to watch.
#[derive(Debug, Clone)]
pub struct WatchRequest {
    /// Device or preview.
    pub surface: SurfaceArg,
    /// Frames to collect before stopping.
    pub count: u32,
    /// Device slot override.
    pub slot: Option<String>,
    /// Preview sessionRef override.
    pub session_ref: Option<String>,
    /// Overall budget.
    pub timeout: Duration,
    /// Ask for one snapshot too.
    pub snapshot: bool,
}

/// The device frame authority: the signer of the newest valid state record
/// for `slot` in the channel (the relay additionally resolves its generation).
pub fn device_authority(events: &[Event], channel: Uuid, slot: &str) -> Option<PublicKey> {
    let verified: Vec<Event> = events
        .iter()
        .filter(|event| event.verify().is_ok())
        .cloned()
        .collect();
    newest_device_state_record(&verified, channel, slot).map(|(event, _)| event.pubkey)
}

/// The preview frame authority: the owner fold over signature-checked 30626s.
pub fn preview_authority(events: &[Event], channel: Uuid, session_ref: &str) -> Option<PublicKey> {
    let verified: Vec<Event> = events
        .iter()
        .filter(|event| event.verify().is_ok())
        .cloned()
        .collect();
    beekeeper_core::session_preview::resolve_preview_owner(&verified, channel, session_ref)
}

/// One frame as the probe prints it. `bytes`/`sha256` describe the decoded
/// JPEG (0 and null for `paused`/`end`).
pub fn frame_line(event: &Event, header: &SurfaceFrameHeader, authority: &PublicKey) -> Value {
    let jpeg = (header.frame_type == SurfaceFrameType::Frame)
        .then(|| {
            base64::engine::general_purpose::STANDARD
                .decode(event.content.as_bytes())
                .ok()
        })
        .flatten();
    json!({
        "type": "frame",
        "eventId": event.id.to_hex(),
        "author": event.pubkey.to_hex(),
        "authorityMatch": event.pubkey == *authority,
        "t": header.frame_type.as_str(),
        "seq": header.seq,
        "epoch": header.epoch,
        "capturedAt": header.captured_at_ms,
        "cadenceMs": header.cadence_ms,
        "dim": format!("{}x{}", header.width, header.height),
        "bytes": jpeg.as_ref().map_or(0, Vec::len),
        "sha256": jpeg.as_ref().map(|bytes| hex::encode(Sha256::digest(bytes))),
    })
}

/// One 44253 as the probe prints it, or `None` when it is not a valid
/// snapshot of this surface.
pub fn snapshot_line(event: &Event, channel: Uuid, key: &str) -> Option<Value> {
    if event.verify().is_err() {
        return None;
    }
    let shot = validate_surface_snapshot_envelope(event).ok()?;
    if shot.channel_id != channel || shot.key != key {
        return None;
    }
    Some(json!({
        "type": "snapshot",
        "id": event.id.to_hex(),
        "token": snapshot_evidence_token(&event.id),
        "author": event.pubkey.to_hex(),
        "x": shot.sha256,
        "url": shot.url,
        "dim": format!("{}x{}", shot.width, shot.height),
        "takenAt": shot.taken_at_ms,
        "requestedBy": shot.requested_by.map(|key| key.to_hex()),
    }))
}

/// The closing line.
pub fn summary_line(frames: u32, snapshots: u32, authority: &PublicKey, all_from: bool) -> Value {
    json!({
        "type": "summary",
        "frames": frames,
        "snapshots": snapshots,
        "authority": authority.to_hex(),
        "allFromAuthority": all_from,
        "note": SAME_MACHINE_NOTE,
    })
}

/// The beat action: `resync` when nothing arrived since the previous beat.
pub fn beat_action(frames_since_beat: u32) -> SurfaceWatchAction {
    if frames_since_beat == 0 {
        SurfaceWatchAction::Resync
    } else {
        SurfaceWatchAction::Watch
    }
}

async fn resolve_target(
    client: &BeekeeperClient,
    ctx: &DeviceContext,
    request: &WatchRequest,
) -> Result<(Surface, String, PublicKey), CliError> {
    match request.surface {
        SurfaceArg::Device => {
            let events = query_device_records(client, ctx).await?;
            let slot = match &request.slot {
                Some(slot) => slot.clone(),
                None => default_open_slot(&events, ctx)?,
            };
            let authority = device_authority(&events, ctx.channel, &slot).ok_or_else(|| {
                CliError::Other(format!("no device state record names slot {slot} here"))
            })?;
            Ok((Surface::Device, slot, authority))
        }
        SurfaceArg::Preview => {
            let filter = json!({
                "kinds": [KIND_SESSION_PREVIEW_ANNOUNCE],
                "#h": [ctx.channel.to_string()],
            });
            let events = events_from_rows(client.query_paginated(filter, 1000).await?);
            let session_ref = match &request.session_ref {
                Some(session_ref) => Uuid::parse_str(session_ref.trim())
                    .map_err(|_| {
                        CliError::Usage(format!("--session-ref is not a UUID: {session_ref}"))
                    })?
                    .to_string(),
                None => pick_session_ref(&events, ctx.channel, ctx.target_key.as_deref())
                    .map(|(session_ref, _)| session_ref)
                    .map_err(|error| match error {
                        PickError::None => {
                            CliError::Other("No preview is shared for this session.".into())
                        }
                        PickError::Ambiguous(refs) => CliError::Usage(format!(
                            "several previews are shared in this session; pass --session-ref \
                             (one of {})",
                            refs.join(", ")
                        )),
                    })?,
            };
            let authority = preview_authority(&events, ctx.channel, &session_ref)
                .ok_or_else(|| CliError::Other("No preview is shared for this session.".into()))?;
            Ok((Surface::Preview, session_ref, authority))
        }
    }
}

/// Run the probe.
pub async fn cmd_watch(
    client: &BeekeeperClient,
    ctx: &DeviceContext,
    request: WatchRequest,
) -> Result<(), CliError> {
    let (surface, key, authority) = resolve_target(client, ctx, &request).await?;
    if authority == client.keys().public_key() {
        return Err(CliError::Usage(
            "this identity is the producer; watch with a second identity".into(),
        ));
    }
    let channel = ctx.channel;
    let watch = |action| -> Result<Event, CliError> {
        let builder = beekeeper_sdk::surface::build_surface_watch(&SurfaceWatch {
            channel_id: channel,
            surface,
            key: key.clone(),
            producer: authority,
            action,
        })
        .map_err(|error| CliError::Other(format!("invalid surface watch: {error}")))?;
        client.sign_event_unchecked(builder)
    };

    let mut conn = NostrWsConnection::connect_authenticated(
        &client.ws_url(),
        client.keys(),
        client.auth_tag(),
    )
    .await
    .map_err(|error| ws_error(error, "watch connection"))?;
    let mut filters = vec![json!({
        "kinds": [KIND_SURFACE_FRAME],
        "#h": [channel.to_string()],
        "#d": [key],
        "authors": [authority.to_hex()],
    })];
    if request.snapshot {
        filters.push(json!({
            "kinds": [KIND_SURFACE_SNAPSHOT],
            "#h": [channel.to_string()],
            "#d": [key],
            "since": nostr::Timestamp::now().as_secs(),
        }));
    }
    let mut req = vec![json!("REQ"), json!(SUBSCRIPTION_ID)];
    req.extend(filters);
    conn.send_raw(&Value::Array(req))
        .await
        .map_err(|error| ws_error(error, "watch subscription"))?;

    send_watch(&mut conn, watch(SurfaceWatchAction::Watch)?).await?;
    if request.snapshot {
        send_watch(&mut conn, watch(SurfaceWatchAction::Snapshot)?).await?;
    }

    let start = tokio::time::Instant::now();
    let deadline = start + request.timeout;
    let keepalive = Duration::from_millis(SURFACE_WATCH_KEEPALIVE_MS);
    let mut next_beat = start + keepalive;
    let (mut frames, mut snapshots, mut since_beat) = (0u32, 0u32, 0u32);
    let mut all_from = true;
    while frames < request.count {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break;
        }
        if now >= next_beat {
            send_watch(&mut conn, watch(beat_action(since_beat))?).await?;
            since_beat = 0;
            next_beat = now + keepalive;
        }
        let wait = deadline.min(next_beat).saturating_duration_since(now);
        match conn.next_event(wait).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == SUBSCRIPTION_ID => {
                let kind = u32::from(event.kind.as_u16());
                if kind == KIND_SURFACE_FRAME {
                    if event.verify().is_err() {
                        continue;
                    }
                    let Ok(header) = validate_surface_frame_envelope(&event) else {
                        continue;
                    };
                    if header.channel_id != channel || header.key != key {
                        continue;
                    }
                    all_from &= event.pubkey == authority;
                    println!("{}", frame_line(&event, &header, &authority));
                    if header.frame_type == SurfaceFrameType::Frame {
                        frames += 1;
                        since_beat += 1;
                    }
                } else if kind == KIND_SURFACE_SNAPSHOT {
                    if let Some(line) = snapshot_line(&event, channel, &key) {
                        println!("{line}");
                        snapshots += 1;
                    }
                }
            }
            Ok(RelayMessage::Closed { message, .. }) => return Err(closed_error(&message)),
            Ok(RelayMessage::Notice { message }) => eprintln!("relay notice: {message}"),
            Ok(_) => {}
            Err(WsClientError::Timeout) => {}
            Err(error) => return Err(ws_error(error, "watch receive")),
        }
    }
    if let Ok(stop) = watch(SurfaceWatchAction::Stop) {
        let _ = conn.send_event(stop).await;
    }
    let _ = conn.disconnect().await;
    println!("{}", summary_line(frames, snapshots, &authority, all_from));
    if frames == 0 {
        return Err(CliError::Other(format!(
            "no frames in {}s",
            request.timeout.as_secs()
        )));
    }
    Ok(())
}

async fn send_watch(conn: &mut NostrWsConnection, event: Event) -> Result<(), CliError> {
    let ok = conn
        .send_event(event)
        .await
        .map_err(|error| ws_error(error, "surface watch publish"))?;
    if ok.accepted {
        Ok(())
    } else {
        Err(CliError::Relay {
            status: 400,
            body: ok.message,
        })
    }
}
