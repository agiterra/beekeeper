//! Host-local custody of an agent seat's key material.
//!
//! A coding-session execution can be *seated* by a managed agent: the signed
//! 44221 create names the agent's public key as its `actor`, and the provider
//! injects that agent's identity into the ACP child so the seat can speak on
//! the relay as itself. The secret half of that identity must never travel —
//! not in the create, not in any signed event, not through the relay at all.
//!
//! So it travels the same way a working directory already does
//! (`coding_sessions::workdir_store`): the desktop writes a host-local file
//! beside the provider's `projects.json`, keyed by the exact `commandId` of
//! the create it belongs to, and the provider consumes and deletes the entry
//! when it spawns the seat. Both processes run as the same user on the same
//! machine, so the file is the whole channel; it is written 0600 and holds
//! nothing but pending seats.
//!
//! The file's shape is the provider's read contract and is pinned by the
//! tests below:
//!
//! ```json
//! { "pending": { "<commandId>": {
//!     "pubkey": "<64-hex>", "nsec": "nsec1…",
//!     "authTag": "[\"…\"]" | null, "relayUrl": "wss://…" } } }
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::managed_agents::storage::{atomic_write_json_restricted, load_managed_agents};
use crate::relay::relay_ws_url_with_override;
use tauri::{AppHandle, State};

/// Filename of the pending-seat map, a sibling of `projects.json` inside the
/// provider's state dir. The provider discovers it exactly as it discovers the
/// projects file (`BUZZ_CSP_ACTOR_SEATS`, defaulting to this name beside it).
pub(crate) const ACTOR_SEATS_FILE_NAME: &str = "actor-seats.json";

/// Longest `commandId` this file will key an entry by. The 44221 command id is
/// a `csl-<uuid>`; the bound matches the lifecycle command's own identifier
/// limit so a malformed key is refused here rather than written to disk.
const MAX_COMMAND_ID_BYTES: usize = 256;

/// One seat's key material, held only until the provider spawns it.
///
/// Field names are the wire contract with the provider's reader; they are
/// camelCase on disk because the desktop's other host-local files are.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActorSeatEntry {
    /// The seat's public key. Must equal the create's `actor`; the provider
    /// refuses the create when it does not.
    pub pubkey: String,
    /// The seat's secret key, bech32 `nsec1…`. Never logged, never serialized
    /// into any event.
    pub nsec: String,
    /// The agent's NIP-OA auth tag, verbatim, or `null` when it has none.
    pub auth_tag: Option<String>,
    /// Relay the seat authenticates against.
    pub relay_url: String,
}

/// The whole file: pending seats keyed by the create's `commandId`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActorSeatsFile {
    pub pending: BTreeMap<String, ActorSeatEntry>,
}

/// Path of the pending-seat file inside a provider state dir.
pub(crate) fn actor_seats_path(state_dir: &Path) -> PathBuf {
    state_dir.join(ACTOR_SEATS_FILE_NAME)
}

/// Assemble one seat entry, refusing an agent whose secret is unavailable.
///
/// An empty `nsec` after the store's key hydration is not a keyless agent: it
/// is a keyring outage or a genuinely absent secret
/// ([`crate::managed_agents::storage`]). Staging it anyway would publish a
/// create naming an actor the provider can never impersonate, so the seat is
/// refused here — before anything is signed or published.
pub(crate) fn build_actor_seat_entry(
    pubkey: &str,
    nsec: &str,
    auth_tag: Option<&str>,
    relay_url: &str,
) -> Result<ActorSeatEntry, String> {
    if !crate::managed_agents::is_lowercase_hex_pubkey(pubkey) {
        return Err("an agent seat's pubkey must be 64-character lowercase hex".to_string());
    }
    if nsec.trim().is_empty() {
        return Err(format!(
            "agent {pubkey} has no private key available — the OS keyring may be unreachable. \
             Refusing to seat an agent without an identity; retry once the keyring is reachable."
        ));
    }
    if relay_url.trim().is_empty() {
        return Err("an agent seat needs a relay URL".to_string());
    }
    Ok(ActorSeatEntry {
        pubkey: pubkey.to_string(),
        nsec: nsec.to_string(),
        auth_tag: auth_tag.map(str::to_string),
        relay_url: relay_url.to_string(),
    })
}

/// Read the pending-seat file. A missing file is an empty one; an unreadable
/// or malformed one is an error, because silently starting from empty would
/// drop another create's staged seat on the next write.
pub(crate) fn read_actor_seats(path: &Path) -> Result<ActorSeatsFile, String> {
    if !path.exists() {
        return Ok(ActorSeatsFile::default());
    }
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(ActorSeatsFile::default());
    }
    serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

/// Write the pending-seat file atomically, owner-only.
pub(crate) fn write_actor_seats(path: &Path, file: &ActorSeatsFile) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(file)
        .map_err(|error| format!("failed to serialize agent seats: {error}"))?;
    atomic_write_json_restricted(path, &payload)
}

/// Stage one seat under its create's `commandId`, replacing any prior entry.
pub(crate) fn stage_actor_seat(
    file: &mut ActorSeatsFile,
    command_id: &str,
    entry: ActorSeatEntry,
) -> Result<(), String> {
    let key = command_id.trim();
    if key.is_empty() {
        return Err("an agent seat needs the create's commandId".to_string());
    }
    if key.len() > MAX_COMMAND_ID_BYTES {
        return Err(format!("commandId exceeds {MAX_COMMAND_ID_BYTES} bytes"));
    }
    file.pending.insert(key.to_string(), entry);
    Ok(())
}

/// Drop a staged seat. Returns whether anything was actually removed, so a
/// caller can tell "cleaned up" from "the provider already consumed it".
pub(crate) fn clear_actor_seat(file: &mut ActorSeatsFile, command_id: &str) -> bool {
    file.pending.remove(command_id.trim()).is_some()
}

// ── The Tauri commands ────────────────────────────────────────────────────
//
// A seated execution's key material reaches the provider host-locally, never
// on the wire (`managed_agents::actor_seats`). These two commands are the
// desktop half of that channel: stage the seat under the create's exact
// `commandId` *before* the 44221 is published, and drop it again if the
// create never goes out. The provider deletes the entry itself once it has
// spawned the seat, so `clear` reports success either way.

/// Resolve the provider state dir the seat file lives in.
///
/// `Ok(None)` means no provider has been provisioned for this relay yet —
/// there is nowhere to stage a seat, and the caller must refuse rather than
/// publish a create no provider can honour.
fn actor_seats_file_path(
    app: &AppHandle,
    state: &AppState,
) -> Result<Option<std::path::PathBuf>, String> {
    let relay_url = relay_ws_url_with_override(state);
    let store = crate::session_provider::store::load_provider_store(app)?;
    let Some(record) = store.get(&relay_url) else {
        return Ok(None);
    };
    let state_dir = crate::session_provider::provider_state_dir(app, &record.provider_pubkey)?;
    Ok(Some(actor_seats_path(&state_dir)))
}

/// Stage a managed agent's identity for one exact coding-session create.
///
/// Refuses when the agent is unknown or its secret is unavailable (a keyring
/// outage), so a create naming an actor the provider could never impersonate
/// is never published.
#[tauri::command]
pub async fn stage_coding_session_actor_seat(
    app: AppHandle,
    state: State<'_, AppState>,
    command_id: String,
    agent_pubkey: String,
) -> Result<(), String> {
    let relay_url = relay_ws_url_with_override(&state);
    let Some(path) = actor_seats_file_path(&app, &state)? else {
        return Err(
            "This computer has no coding-session provider yet, so an agent cannot be seated."
                .to_string(),
        );
    };
    let pubkey = agent_pubkey.trim().to_string();
    let entry = {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        let records = load_managed_agents(&app)?;
        let record = records
            .iter()
            .find(|record| record.pubkey == pubkey)
            .ok_or_else(|| format!("agent {pubkey} is not a managed agent on this computer"))?;
        build_actor_seat_entry(
            &record.pubkey,
            &record.private_key_nsec,
            record.auth_tag.as_deref(),
            &relay_url,
        )?
    };
    let mut file = read_actor_seats(&path)?;
    stage_actor_seat(&mut file, &command_id, entry)?;
    write_actor_seats(&path, &file)
}

/// Drop a staged seat. Succeeds when the provider already consumed it.
#[tauri::command]
pub async fn clear_coding_session_actor_seat(
    app: AppHandle,
    state: State<'_, AppState>,
    command_id: String,
) -> Result<(), String> {
    let Some(path) = actor_seats_file_path(&app, &state)? else {
        return Ok(());
    };
    let mut file = read_actor_seats(&path)?;
    if !clear_actor_seat(&mut file, &command_id) {
        return Ok(());
    }
    write_actor_seats(&path, &file)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBKEY: &str = "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66";

    #[test]
    fn a_seat_without_a_key_in_the_keyring_is_refused() {
        let error = build_actor_seat_entry(PUBKEY, "", None, "wss://relay.example")
            .expect_err("an empty nsec must refuse the seat");
        assert!(error.contains("keyring"), "unexpected refusal: {error}");
        assert!(
            error.contains(PUBKEY),
            "refusal must name the agent: {error}"
        );
        // Whitespace is not a key either.
        assert!(build_actor_seat_entry(PUBKEY, "   ", None, "wss://relay.example").is_err());
    }

    #[test]
    fn a_seat_pubkey_must_be_lowercase_hex() {
        assert!(build_actor_seat_entry("not-a-pubkey", "nsec1x", None, "wss://r").is_err());
        assert!(build_actor_seat_entry(&PUBKEY.to_uppercase(), "nsec1x", None, "wss://r").is_err());
    }

    #[test]
    fn the_file_shape_is_the_providers_read_contract() {
        let mut file = ActorSeatsFile::default();
        let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://relay.example")
            .expect("a hydrated key seats an agent");
        stage_actor_seat(&mut file, "csl-1234", entry).expect("stage");
        let json: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&file).expect("serialize"))
                .expect("parse back");
        let pending = json.get("pending").expect("pending key");
        let seat = pending.get("csl-1234").expect("keyed by commandId");
        let keys: Vec<&str> = seat
            .as_object()
            .expect("seat object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, vec!["authTag", "nsec", "pubkey", "relayUrl"]);
        assert_eq!(seat.get("pubkey").and_then(|v| v.as_str()), Some(PUBKEY));
        assert_eq!(
            seat.get("nsec").and_then(|v| v.as_str()),
            Some("nsec1secret")
        );
        assert!(seat.get("authTag").expect("authTag present").is_null());
        assert_eq!(
            seat.get("relayUrl").and_then(|v| v.as_str()),
            Some("wss://relay.example")
        );
    }

    #[test]
    fn an_auth_tag_travels_verbatim() {
        let entry = build_actor_seat_entry(
            PUBKEY,
            "nsec1secret",
            Some("[\"tag\",\"value\"]"),
            "wss://relay.example",
        )
        .expect("seat");
        assert_eq!(entry.auth_tag.as_deref(), Some("[\"tag\",\"value\"]"));
    }

    #[test]
    fn staging_needs_a_command_id() {
        let mut file = ActorSeatsFile::default();
        let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r").expect("seat");
        assert!(stage_actor_seat(&mut file, "  ", entry.clone()).is_err());
        assert!(stage_actor_seat(&mut file, &"c".repeat(257), entry).is_err());
        assert!(file.pending.is_empty());
    }

    #[test]
    fn clearing_reports_whether_the_provider_beat_us_to_it() {
        let mut file = ActorSeatsFile::default();
        let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r").expect("seat");
        stage_actor_seat(&mut file, "csl-9", entry).expect("stage");
        assert!(clear_actor_seat(&mut file, "csl-9"));
        assert!(!clear_actor_seat(&mut file, "csl-9"));
    }

    #[test]
    fn the_file_round_trips_through_disk_owner_only() {
        let dir = std::env::temp_dir().join(format!(
            "buzz-actor-seats-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = actor_seats_path(&dir);
        assert_eq!(
            read_actor_seats(&path).expect("missing is empty"),
            ActorSeatsFile::default()
        );
        let mut file = ActorSeatsFile::default();
        let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r").expect("seat");
        stage_actor_seat(&mut file, "csl-7", entry).expect("stage");
        write_actor_seats(&path, &file).expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "the seat file must be owner-only");
        }
        assert_eq!(read_actor_seats(&path).expect("read back"), file);
        std::fs::remove_dir_all(&dir).ok();
    }
}
