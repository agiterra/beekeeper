//! `bee device` — open, look at, and close this session's simulator, and
//! probe the shared device/preview stream as a second client (NIP-SDV,
//! NIP-SW; WIRE-C5 § 3, § 4, § 7).
//!
//! The provider owns the device. This command only publishes a kind 44254
//! command and waits for the provider's one terminal kind 44255 record that
//! carries its `sdv-cmd`. Host-local facts (UDID, daemon, screenshot paths)
//! never come from the relay: the PNG path and the quick-start text are read
//! from the provider's 0600 slot file on this machine, when this process can
//! see it, and are reported as unavailable (never failed) when it cannot.
//!
//! Exit codes: 0 ok; 1 input; 2 network; 3 auth; 4 the provider refused or
//! the device failed; 5 no answer inside the wait (`unconfirmed`: the
//! command may still be acted on).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use beekeeper_core::kind::{KIND_SESSION_DEVICE_RECORD, KIND_SURFACE_SNAPSHOT};
use beekeeper_core::preview_grant::PREVIEW_GRANT_ENV;
use beekeeper_core::session_device::{
    DeviceCommandPayload, DeviceOp, DevicePlatform, SessionDeviceCommand,
};
use beekeeper_core::surface_snapshot::{
    snapshot_evidence_token, validate_surface_snapshot_envelope,
};
use beekeeper_ws_client::{NostrWsConnection, RelayMessage, WsClientError};
use clap::{Args, Subcommand, ValueEnum};
use nostr::{Event, EventId};
use serde_json::{json, Value};

use crate::client::BeekeeperClient;
use crate::error::CliError;

#[path = "device_support.rs"]
pub mod support;
#[path = "device_watch.rs"]
pub mod watch;

use support::{
    command_id, default_open_slot, discover_session_dir, events_from_rows, list_output,
    match_terminal, parse_slot_flag, quick_start, read_local_slot, resolve_context, shot_png_path,
    terminal_result, AcceptedRecord, DeviceContext,
};

/// Env var naming this session's provider device directory (overrides the
/// PATH shim lookup).
pub const DEVICE_DIR_ENV: &str = "BEEKEEPER_DEVICE_DIR";

const SUBSCRIPTION_ID: &str = "bee-device";
const DEFAULT_WAIT_SECS: u64 = 120;

/// Which session a call addresses. Each flag overrides the matching claim of
/// `$BEEKEEPER_PREVIEW_GRANT`.
#[derive(Debug, Clone, Default, Args)]
pub struct SessionArgs {
    /// The session channel UUID.
    #[arg(long)]
    pub channel: Option<String>,
    /// The generation's cs-target key (`coding-session/v1|…`).
    #[arg(long)]
    pub target: Option<String>,
    /// The provider pubkey whose records to trust (default: the grant's issuer).
    #[arg(long)]
    pub provider: Option<String>,
}

/// Platforms `open` accepts.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PlatformArg {
    /// iOS Simulator.
    Ios,
    /// Android emulator.
    Android,
}

/// Surfaces `watch` probes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SurfaceArg {
    /// A device slot.
    Device,
    /// The session's shared Browser preview.
    Preview,
}

/// Drive this session's simulator (provider-owned; NIP-SDV).
#[derive(Debug, Subcommand)]
pub enum DeviceCmd {
    /// Show what this machine's provider offers and each slot's newest state.
    List {
        #[command(flatten)]
        session: SessionArgs,
    },
    /// Boot (or reuse) a device and bind it to this session.
    Open {
        #[command(flatten)]
        session: SessionArgs,
        /// Platform.
        #[arg(long, value_enum, default_value = "ios")]
        platform: PlatformArg,
        /// Device model, e.g. `iPhone 17` (default: the provider's choice).
        #[arg(long)]
        model: Option<String>,
        /// Reuse a command id (a retry); default: a fresh random id.
        #[arg(long)]
        command_id: Option<String>,
        /// Seconds to wait for the provider's answer.
        #[arg(long, default_value_t = DEFAULT_WAIT_SECS)]
        wait: u64,
    },
    /// Publish a dated snapshot of the device everyone in the session sees.
    Screenshot {
        #[command(flatten)]
        session: SessionArgs,
        /// The slot (default: this session's newest open slot).
        #[arg(long)]
        slot: Option<String>,
        /// Copy the provider's PNG here too.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Reuse a command id (a retry); default: a fresh random id.
        #[arg(long)]
        command_id: Option<String>,
        /// Seconds to wait for the provider's answer.
        #[arg(long, default_value_t = DEFAULT_WAIT_SECS)]
        wait: u64,
    },
    /// Release the device.
    Close {
        #[command(flatten)]
        session: SessionArgs,
        /// The slot (default: this session's newest open slot).
        #[arg(long)]
        slot: Option<String>,
        /// Also shut the simulator down.
        #[arg(long)]
        shutdown: bool,
        /// Reuse a command id (a retry); default: a fresh random id.
        #[arg(long)]
        command_id: Option<String>,
        /// Seconds to wait for the provider's answer.
        #[arg(long, default_value_t = DEFAULT_WAIT_SECS)]
        wait: u64,
    },
    /// Watch a shared surface as a second client and print each frame
    /// (a probe: proves the stream reaches another identity).
    Watch {
        #[command(flatten)]
        session: SessionArgs,
        /// Which surface.
        #[arg(long, value_enum)]
        surface: SurfaceArg,
        /// Stop after this many frames.
        #[arg(long, default_value_t = 3)]
        count: u32,
        /// Device slot (default: this session's newest open slot).
        #[arg(long)]
        slot: Option<String>,
        /// Preview sessionRef (default: the one open preview of the session).
        #[arg(long)]
        session_ref: Option<String>,
        /// Give up after this many seconds.
        #[arg(long, default_value_t = 60)]
        timeout: u64,
        /// Also ask the producer for one snapshot and print it.
        #[arg(long)]
        snapshot: bool,
    },
}

/// Run one `bee device` subcommand.
pub async fn dispatch(cmd: &DeviceCmd, client: &BeekeeperClient) -> Result<(), CliError> {
    let grant = std::env::var(PREVIEW_GRANT_ENV).ok();
    let context = |session: &SessionArgs| {
        resolve_context(
            session.channel.as_deref(),
            session.target.as_deref(),
            session.provider.as_deref(),
            grant.as_deref(),
        )
    };
    match cmd {
        DeviceCmd::List { session } => cmd_list(client, &context(session)?).await,
        DeviceCmd::Open {
            session,
            platform,
            model,
            command_id: id,
            wait,
        } => {
            let ctx = context(session)?;
            let payload = DeviceCommandPayload {
                op: DeviceOp::Open,
                platform: Some(match platform {
                    PlatformArg::Ios => DevicePlatform::Ios,
                    PlatformArg::Android => DevicePlatform::Android,
                }),
                model: model.clone(),
                shutdown: None,
                action: None,
            };
            let outcome = run_command(
                client,
                &ctx,
                payload,
                None,
                command_id(id.as_deref())?,
                *wait,
            )
            .await?;
            let mut out = terminal_result(&outcome.terminal)?;
            add_quick_start(&mut out, &outcome.terminal);
            print_json(&out)
        }
        DeviceCmd::Close {
            session,
            slot,
            shutdown,
            command_id: id,
            wait,
        } => {
            let ctx = context(session)?;
            let payload = DeviceCommandPayload {
                op: DeviceOp::Close,
                platform: None,
                model: None,
                shutdown: Some(*shutdown),
                action: None,
            };
            let slot = slot.as_deref().map(parse_slot_flag).transpose()?;
            let outcome = run_command(
                client,
                &ctx,
                payload,
                slot,
                command_id(id.as_deref())?,
                *wait,
            )
            .await?;
            print_json(&terminal_result(&outcome.terminal)?)
        }
        DeviceCmd::Screenshot {
            session,
            slot,
            out,
            command_id: id,
            wait,
        } => {
            let ctx = context(session)?;
            let payload = DeviceCommandPayload {
                op: DeviceOp::Screenshot,
                platform: None,
                model: None,
                shutdown: None,
                action: None,
            };
            let slot = slot.as_deref().map(parse_slot_flag).transpose()?;
            let outcome = run_command(
                client,
                &ctx,
                payload,
                slot,
                command_id(id.as_deref())?,
                *wait,
            )
            .await?;
            let result = terminal_result(&outcome.terminal)?;
            let shot = screenshot_output(client, &outcome, result, out.as_deref()).await;
            print_json(&shot)
        }
        DeviceCmd::Watch {
            session,
            surface,
            count,
            slot,
            session_ref,
            timeout,
            snapshot,
        } => {
            let ctx = context(session)?;
            watch::cmd_watch(
                client,
                &ctx,
                watch::WatchRequest {
                    surface: *surface,
                    count: *count,
                    slot: slot.as_deref().map(parse_slot_flag).transpose()?,
                    session_ref: session_ref.clone(),
                    timeout: Duration::from_secs(*timeout),
                    snapshot: *snapshot,
                },
            )
            .await
        }
    }
}

fn print_json(value: &Value) -> Result<(), CliError> {
    println!("{value}");
    Ok(())
}

/// Every 44255 of the channel, read over HTTP.
pub(crate) async fn query_device_records(
    client: &BeekeeperClient,
    ctx: &DeviceContext,
) -> Result<Vec<Event>, CliError> {
    let filter = json!({"kinds": [KIND_SESSION_DEVICE_RECORD], "#h": [ctx.channel.to_string()]});
    Ok(events_from_rows(
        client.query_paginated(filter, 1000).await?,
    ))
}

async fn cmd_list(client: &BeekeeperClient, ctx: &DeviceContext) -> Result<(), CliError> {
    let events = query_device_records(client, ctx).await?;
    print_json(&list_output(&events, ctx))
}

/// Map a WebSocket failure into the CLI's error families.
pub(crate) fn ws_error(error: WsClientError, stage: &str) -> CliError {
    match error {
        WsClientError::AuthFailed(message) => CliError::Auth(format!("{stage}: {message}")),
        WsClientError::NoAuthChallenge => CliError::Auth(format!(
            "{stage}: relay did not provide the required NIP-42 challenge"
        )),
        other => CliError::Other(format!("{stage}: {other}")),
    }
}

/// What a device command came back with.
struct CommandOutcome {
    terminal: AcceptedRecord,
    /// 44253s seen on the connection, by id.
    snapshots: HashMap<EventId, Event>,
}

/// Subscribe, publish one 44254 (unless the replay already holds its answer:
/// a retry with the same `--command-id`), and wait for its terminal 44255.
async fn run_command(
    client: &BeekeeperClient,
    ctx: &DeviceContext,
    payload: DeviceCommandPayload,
    slot: Option<String>,
    command_id: String,
    wait_secs: u64,
) -> Result<CommandOutcome, CliError> {
    let target = ctx.target()?.to_owned();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(wait_secs);
    let unanswered = || {
        CliError::Unconfirmed(format!(
            "no answer from the provider after {wait_secs}s (command {command_id}; it may still \
             be acted on — retry with --command-id {command_id})"
        ))
    };
    let mut conn = NostrWsConnection::connect_authenticated(
        &client.ws_url(),
        client.keys(),
        client.auth_tag(),
    )
    .await
    .map_err(|error| ws_error(error, "device connection"))?;
    let channel = ctx.channel.to_string();
    let since = nostr::Timestamp::now().as_secs().saturating_sub(300);
    conn.send_raw(&json!([
        "REQ",
        SUBSCRIPTION_ID,
        {"kinds": [KIND_SESSION_DEVICE_RECORD], "#h": [channel], "limit": 1000},
        {"kinds": [KIND_SURFACE_SNAPSHOT], "#h": [channel], "since": since}
    ]))
    .await
    .map_err(|error| ws_error(error, "device subscription"))?;

    let mut replay = Vec::new();
    let mut snapshots = HashMap::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match conn.next_event(remaining).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == SUBSCRIPTION_ID => {
                if u32::from(event.kind.as_u16()) == KIND_SURFACE_SNAPSHOT {
                    snapshots.insert(event.id, *event);
                } else {
                    replay.push(*event);
                }
            }
            Ok(RelayMessage::Eose { subscription_id }) if subscription_id == SUBSCRIPTION_ID => {
                break
            }
            Ok(RelayMessage::Closed { message, .. }) => {
                return Err(closed_error(&message));
            }
            Ok(_) => {}
            Err(WsClientError::Timeout) => return Err(unanswered()),
            Err(error) => return Err(ws_error(error, "device subscription")),
        }
    }

    if let Some(terminal) = replay
        .iter()
        .find_map(|event| match_terminal(event, ctx, &command_id))
    {
        return Ok(CommandOutcome {
            terminal,
            snapshots,
        });
    }

    let slot = match payload.op {
        DeviceOp::Open => None,
        _ => Some(match slot {
            Some(slot) => slot,
            None => default_open_slot(&replay, ctx)?,
        }),
    };
    let command = SessionDeviceCommand {
        channel_id: ctx.channel,
        target_key: target,
        command_id: command_id.clone(),
        slot,
        payload,
    };
    let builder = beekeeper_sdk::surface::build_session_device_command(&command)
        .map_err(|error| CliError::Usage(format!("invalid device command: {error}")))?;
    // Exact tag order, nothing else: `sign_event` would add a NIP-OA `auth`
    // tag the 44254 layout refuses.
    let event = client.sign_event_unchecked(builder)?;
    let ok = conn
        .send_event(event)
        .await
        .map_err(|error| ws_error(error, "device command publish"))?;
    if !ok.accepted {
        return Err(CliError::Relay {
            status: 400,
            body: ok.message,
        });
    }

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match conn.next_event(remaining).await {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id == SUBSCRIPTION_ID => {
                if u32::from(event.kind.as_u16()) == KIND_SURFACE_SNAPSHOT {
                    snapshots.insert(event.id, *event);
                } else if let Some(terminal) = match_terminal(&event, ctx, &command_id) {
                    let _ = conn.disconnect().await;
                    return Ok(CommandOutcome {
                        terminal,
                        snapshots,
                    });
                }
            }
            Ok(RelayMessage::Closed { message, .. }) => return Err(closed_error(&message)),
            Ok(_) => {}
            Err(WsClientError::Timeout) => return Err(unanswered()),
            Err(error) => return Err(ws_error(error, "device wait")),
        }
    }
}

pub(crate) fn closed_error(message: &str) -> CliError {
    let lower = message.to_ascii_lowercase();
    if lower.contains("auth") || lower.contains("restricted") || lower.contains("forbidden") {
        CliError::Auth(format!("subscription closed: {message}"))
    } else {
        CliError::Relay {
            status: 400,
            body: format!("subscription closed: {message}"),
        }
    }
}

/// Add the provider's quick-start text to an `open` result when this
/// process can read the slot file, else say why not.
fn add_quick_start(out: &mut Value, terminal: &AcceptedRecord) {
    let Value::Object(map) = out else { return };
    let Some(slot) = terminal.record.slot.as_deref() else {
        return;
    };
    let local = discover_session_dir(
        std::env::var_os(DEVICE_DIR_ENV).as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
    .and_then(|dir| read_local_slot(&dir, slot));
    match local {
        Ok(local) => {
            map.insert(
                "quickStart".into(),
                json!(quick_start(slot, &local.model, &local.os_version)),
            );
        }
        Err(why) => {
            map.insert("quickStart".into(), Value::Null);
            map.insert("quickStartNote".into(), json!(why));
        }
    }
}

/// The screenshot's output: the 44253 it points at and the local PNG.
async fn screenshot_output(
    client: &BeekeeperClient,
    outcome: &CommandOutcome,
    mut result: Value,
    out: Option<&std::path::Path>,
) -> Value {
    let terminal = &outcome.terminal;
    let snapshot_id = terminal.record.snapshot;
    let mut snapshot = snapshot_id.and_then(|id| outcome.snapshots.get(&id).cloned());
    if snapshot.is_none() {
        if let Some(id) = snapshot_id {
            let filter = json!({"ids": [id.to_hex()], "kinds": [KIND_SURFACE_SNAPSHOT]});
            if let Ok(rows) = client.query_paginated(filter, 1).await {
                snapshot = events_from_rows(rows).into_iter().next();
            }
        }
    }
    let parsed = snapshot
        .as_ref()
        .filter(|event| event.verify().is_ok() && event.pubkey == terminal.author)
        .and_then(|event| validate_surface_snapshot_envelope(event).ok());
    let Value::Object(map) = &mut result else {
        return result;
    };
    map.insert(
        "token".into(),
        json!(snapshot_id.map(|id| snapshot_evidence_token(&id))),
    );
    let sha256 = match &parsed {
        Some(shot) => {
            map.insert("url".into(), json!(shot.url));
            map.insert("sha256".into(), json!(shot.sha256));
            map.insert(
                "dim".into(),
                json!(format!("{}x{}", shot.width, shot.height)),
            );
            map.insert("mime".into(), json!(shot.mime));
            map.insert("takenAt".into(), json!(shot.taken_at_ms));
            Some(shot.sha256.clone())
        }
        None => {
            for key in ["url", "sha256", "dim"] {
                map.insert(key.into(), Value::Null);
            }
            map.insert(
                "snapshotNote".into(),
                json!("the snapshot record the provider named could not be read from the relay"),
            );
            None
        }
    };
    let png = terminal
        .record
        .slot
        .as_deref()
        .ok_or_else(|| "the provider's record names no slot".to_owned())
        .and_then(|slot| {
            let dir = discover_session_dir(
                std::env::var_os(DEVICE_DIR_ENV).as_deref(),
                std::env::var_os("PATH").as_deref(),
            )?;
            read_local_slot(&dir, slot)
        })
        .and_then(|local| {
            let sha = sha256
                .as_deref()
                .ok_or_else(|| "the snapshot's sha256 is unknown".to_owned())?;
            let path = shot_png_path(&local, sha)
                .ok_or_else(|| "the snapshot's sha256 is not a hash".to_owned())?;
            if path.is_file() {
                Ok(path)
            } else {
                Err("the provider's PNG for this snapshot is not on this machine".to_owned())
            }
        });
    match png {
        Ok(path) => {
            map.insert("pngPath".into(), json!(path.display().to_string()));
            if let Some(out) = out {
                match copy_png(&path, out) {
                    Ok(dest) => {
                        map.insert("outPath".into(), json!(dest.display().to_string()));
                    }
                    Err(why) => {
                        map.insert("outPath".into(), Value::Null);
                        map.insert("outNote".into(), json!(why));
                    }
                }
            }
        }
        Err(why) => {
            map.insert("pngPath".into(), Value::Null);
            map.insert("pngNote".into(), json!(why));
        }
    }
    result
}

fn copy_png(from: &std::path::Path, to: &std::path::Path) -> Result<PathBuf, String> {
    let dest = if to.is_absolute() {
        to.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("cannot read the working directory: {error}"))?
            .join(to)
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    std::fs::copy(from, &dest).map_err(|error| format!("cannot copy the PNG: {error}"))?;
    Ok(dest)
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
