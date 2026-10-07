//! beekeeper-mirror-bridge — event-driven trigger for a git mirror sync.
//!
//! Subscribes to the relay's relay-signed kind:30618 NIP-34 ref-state events
//! (published on every ref-changing push) and runs a sync command whenever one
//! arrives, so a downstream mirror (e.g. the GitHub copy that Woodpecker
//! watches) follows the relay within seconds instead of a polling interval.
//!
//! 30618 is addressable/replaceable: the stored event replayed on every
//! (re)connect is indistinguishable from fresh work, so the bridge simply
//! syncs on it — the sync command is idempotent and cheap when nothing
//! changed. That replay is also what catches pushes made while the bridge was
//! down. Syncs are coalesced: events arriving while one sync runs mark it
//! dirty and one follow-up sync runs after it finishes.
//!
//! Deployed on the forge as `hive-mirror-bridge.service`; see
//! `scripts/forge/setup-hive-mirror.sh` and docs/INTEGRATION.md.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use beekeeper_core::kind::KIND_GIT_REPO_STATE;
use beekeeper_ws_client::{NostrWsConnection, RelayMessage, WsClientError};
use clap::Parser;
use nostr::Keys;
use serde_json::json;
use zeroize::Zeroize;

const SUBSCRIPTION_ID: &str = "mirror-bridge";
const MAX_BACKOFF_SECS: u64 = 300;
/// A connection that survived this long resets the reconnect backoff.
const STABLE_CONNECTION_SECS: u64 = 60;

#[derive(Parser)]
#[command(about, version)]
struct Args {
    /// Relay websocket URL, e.g. wss://hive.agiterra.org
    #[arg(long, env = "BEEKEEPER_RELAY_URL")]
    relay: String,

    /// Key file holding an nsec1... or 64-char hex secret (0600).
    /// $NOSTR_PRIVATE_KEY takes precedence and avoids the file.
    #[arg(long)]
    keyfile: Option<PathBuf>,

    /// Repo names (30618 `d` tags) to react to. Repeatable; when omitted,
    /// every repo's ref-state event triggers a sync.
    #[arg(long = "repo")]
    repos: Vec<String>,

    /// Command run (no arguments) to perform the sync.
    #[arg(long, default_value = "/usr/local/bin/git-mirror-update")]
    sync_command: String,

    /// Reconnect after this many seconds without any relay traffic, as a
    /// hedge against a silently dead TCP connection.
    #[arg(long, default_value_t = 900)]
    idle_reconnect_secs: u64,
}

fn load_keys(args: &Args) -> Result<Keys, String> {
    let mut raw = match std::env::var("NOSTR_PRIVATE_KEY") {
        Ok(val) if !val.is_empty() => val,
        _ => {
            let path = args.keyfile.as_ref().ok_or_else(|| {
                "no key configured: set $NOSTR_PRIVATE_KEY or pass --keyfile".to_string()
            })?;
            std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read keyfile {}: {e}", path.display()))?
        }
    };
    let keys = Keys::parse(raw.trim()).map_err(|e| format!("invalid key: {e}"));
    raw.zeroize();
    keys
}

/// The `d` tag of an addressable event, if present.
fn d_tag(event: &nostr::Event) -> Option<&str> {
    event
        .tags
        .iter()
        .find_map(|tag| {
            let parts = tag.as_slice();
            (parts.first().map(String::as_str) == Some("d")).then(|| parts.get(1))?
        })
        .map(String::as_str)
}

async fn run_sync(command: String) {
    println!("sync: running {command}");
    let started = Instant::now();
    match tokio::process::Command::new(&command).output().await {
        Ok(out) => {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                println!("sync: {line}");
            }
            for line in String::from_utf8_lossy(&out.stderr).lines() {
                eprintln!("sync: {line}");
            }
            let secs = started.elapsed().as_secs();
            if out.status.success() {
                println!("sync: done in {secs}s");
            } else {
                eprintln!("sync: {command} failed ({}) after {secs}s", out.status);
            }
        }
        Err(e) => eprintln!("sync: cannot run {command}: {e}"),
    }
}

/// One connection's lifetime: authenticate, subscribe, react until the
/// connection dies. Always returns the reason it ended.
async fn run_connection(args: &Args, keys: &Keys) -> String {
    let mut conn = match NostrWsConnection::connect_authenticated(&args.relay, keys, None).await {
        Ok(conn) => conn,
        Err(e) => return format!("connect: {e}"),
    };

    let mut filter = json!({ "kinds": [KIND_GIT_REPO_STATE] });
    if !args.repos.is_empty() {
        filter["#d"] = json!(args.repos);
    }
    if let Err(e) = conn
        .send_raw(&json!(["REQ", SUBSCRIPTION_ID, filter]))
        .await
    {
        return format!("subscribe: {e}");
    }
    println!("subscribed to kind:{KIND_GIT_REPO_STATE} at {}", args.relay);

    let idle = Duration::from_secs(args.idle_reconnect_secs);
    let mut sync_needed = false;
    let mut running: Option<tokio::task::JoinHandle<()>> = None;

    let reason = loop {
        if sync_needed && running.is_none() {
            sync_needed = false;
            running = Some(tokio::spawn(run_sync(args.sync_command.clone())));
        }
        tokio::select! {
            msg = conn.next_event(idle) => match msg {
                Ok(RelayMessage::Event { subscription_id, event })
                    if subscription_id == SUBSCRIPTION_ID =>
                {
                    let repo = d_tag(&event).unwrap_or("<no d tag>");
                    println!("ref-state update for {repo}");
                    sync_needed = true;
                }
                // Reconcile once even when the replay was empty — proves the
                // pipe works and needs no special-casing of the first run.
                Ok(RelayMessage::Eose { .. }) => sync_needed = true,
                Ok(RelayMessage::Closed { subscription_id, message })
                    if subscription_id == SUBSCRIPTION_ID =>
                {
                    break format!("subscription closed by relay: {message}");
                }
                Ok(RelayMessage::Notice { message }) => println!("relay notice: {message}"),
                Ok(_) => {}
                // Quiet is normal while a sync runs; otherwise treat a long
                // silence as a possibly-dead connection.
                Err(WsClientError::Timeout) if running.is_some() => {}
                Err(WsClientError::Timeout) => {
                    break format!("no relay traffic for {}s", args.idle_reconnect_secs);
                }
                Err(e) => break format!("connection lost: {e}"),
            },
            // `running` is Some here, so the unwrap cannot panic; the task
            // itself never panics (run_sync handles its own errors).
            result = async { running.as_mut().unwrap().await }, if running.is_some() => {
                running = None;
                if let Err(e) = result {
                    eprintln!("sync task failed: {e}");
                }
            }
        }
    };

    // Never leave a sync running into the next connection's lifetime — two
    // concurrent runs would race on the same repos.
    if let Some(handle) = running {
        let _ = handle.await;
    }
    reason
}

fn main() -> ExitCode {
    // Before clap, tokio or any thread: read BUZZ_* as BEEKEEPER_*.
    beekeeper_core::env_compat::adopt_legacy_env("beekeeper-mirror-bridge");
    async_main()
}

#[tokio::main]
async fn async_main() -> ExitCode {
    // The workspace compiles both aws-lc-rs and ring into rustls
    // transitively, so it cannot auto-select a provider and panics on the
    // first TLS connection without this. Mirrors beekeeper-admin's main().
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        eprintln!("failed to install rustls crypto provider");
        return ExitCode::from(1);
    }

    let args = Args::parse();
    let keys = match load_keys(&args) {
        Ok(keys) => keys,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    println!("bridge identity: {}", keys.public_key().to_hex());

    let mut backoff = 1u64;
    loop {
        let connected_at = Instant::now();
        let reason = run_connection(&args, &keys).await;
        if connected_at.elapsed() >= Duration::from_secs(STABLE_CONNECTION_SECS) {
            backoff = 1;
        }
        eprintln!("{reason}; reconnecting in {backoff}s");
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Kind, Tag};

    fn event_with_tags(tags: Vec<Tag>) -> nostr::Event {
        let keys = Keys::generate();
        EventBuilder::new(Kind::Custom(KIND_GIT_REPO_STATE as u16), "")
            .tags(tags)
            .sign_with_keys(&keys)
            .unwrap()
    }

    #[test]
    fn d_tag_extracts_repo_name() {
        let event = event_with_tags(vec![
            Tag::parse(["t", "noise"]).unwrap(),
            Tag::parse(["d", "agiterra-beekeeper"]).unwrap(),
        ]);
        assert_eq!(d_tag(&event), Some("agiterra-beekeeper"));
    }

    #[test]
    fn d_tag_missing_or_bare_is_none() {
        assert_eq!(d_tag(&event_with_tags(vec![])), None);
        // A "d" tag with no value must not panic or match.
        let event = event_with_tags(vec![Tag::parse(["d"]).unwrap()]);
        assert_eq!(d_tag(&event), None);
    }
}
