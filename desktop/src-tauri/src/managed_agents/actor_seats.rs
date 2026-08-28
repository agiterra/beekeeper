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
//!     "authTag": "[\"…\"]" | null, "relayUrl": "wss://…",
//!     "displayName": "Levain" | absent,
//!     "packDir": "/…/teams/roles" | absent,
//!     "personaId": "builder" | absent } } }
//! ```
//!
//! `packDir`/`personaId` are the seat's role pack (contract D8-A). They travel
//! by this file for the same reason the nsec does — not because they are
//! secret, but because a *path on this machine* is host-local. They are
//! resolved here, in Rust, from the agent's own provenance; the webview never
//! sends a path, so a compromised or merely wrong renderer cannot point the
//! provider's skill materialization at a directory of its choosing.

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
    /// The agent's display name, when it has one.
    ///
    /// The provider uses it as the seat's `git` author and committer name, so
    /// a seated execution's commits are attributed to the agent rather than to
    /// whoever owns the home directory (ledger 77, *Fence* (b)). Absent when
    /// the record carries no display name; the provider then falls back to the
    /// seat's role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Host-local directory of the role pack this seat's persona came from.
    ///
    /// Absent when this computer has no pack behind the agent — the seat still
    /// runs, it simply materializes no role skills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_dir: Option<PathBuf>,
    /// The persona's name inside [`Self::pack_dir`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
}

/// What one staging call did, for a caller that has to be honest about it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StagedActorSeat {
    /// Whether a role pack was staged with the seat. `false` means the seat
    /// will run with its prompt alone: no `.agents/skills` will appear in its
    /// working directory, and any copy claiming otherwise is wrong.
    pub pack_staged: bool,
}

/// Where a seat's role pack lives on this computer, as `(pack dir, persona)`.
///
/// Two sources, in order:
///
/// 1. The instance-side link (`persona_team_dir` + `persona_name_in_team`),
///    for records that carry one.
/// 2. The definition's provenance: the team directory the persona was
///    installed from, plus its slug inside that pack. This is the live path —
///    the instance-side pair is `None` on records built today
///    (`AgentDefinition::into_agent_record`).
///
/// Returns `None` — never a guess — when neither resolves to a pack that
/// actually holds that persona. **A directory that exists is not the test.**
/// The provenance arm pairs a slug with whatever directory the agent's team
/// names, and those two facts can disagree: an agent whose definition arrived
/// from another device carries the kind:30175 `d` tag as its slug, a persona
/// may be renamed or dropped from its pack, and a team directory may be
/// something else entirely. Staging such a pair is not a harmless guess — the
/// provider's `materialize_seat_skills` turns an unreadable pack into
/// `CreateFailure{PROVIDER_UNAVAILABLE}`, so a guess here refuses a create
/// that used to work. A seat with no pack is a legal seat that carries no role
/// skills; that is the honest answer when the persona does not resolve.
pub(crate) fn resolve_seat_pack(
    record: &crate::managed_agents::types::ManagedAgentRecord,
    teams: &[crate::managed_agents::types::TeamRecord],
) -> Option<(PathBuf, String)> {
    let (dir, persona) = match (
        record.persona_team_dir.as_ref(),
        record.persona_name_in_team.as_ref(),
    ) {
        (Some(dir), Some(persona)) => (dir.clone(), persona.clone()),
        _ => {
            let persona = record.source_team_persona_slug.as_ref()?;
            let team_id = record
                .source_team
                .as_deref()
                .or(record.team_id.as_deref())?;
            let dir = teams
                .iter()
                .find(|team| team.id == team_id)?
                .source_dir
                .clone()?;
            (dir, persona.clone())
        }
    };
    if persona.trim().is_empty() || !dir.is_dir() {
        return None;
    }
    // The only question that matters: can the provider read this persona out
    // of this pack? Ask the same resolver the provider will.
    if let Err(error) = buzz_persona_pkg::resolve::resolve_persona_by_name(&dir, &persona) {
        tracing::debug!(
            pack = %dir.display(),
            persona = %persona,
            %error,
            "seat stages no role pack: this computer has no such persona in that pack"
        );
        return None;
    }
    Some((dir, persona))
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
    display_name: Option<&str>,
    pack: Option<(PathBuf, String)>,
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
    let (pack_dir, persona_id) = match pack {
        Some((dir, persona)) => (Some(dir), Some(persona)),
        None => (None, None),
    };
    Ok(ActorSeatEntry {
        pubkey: pubkey.to_string(),
        nsec: nsec.to_string(),
        auth_tag: auth_tag.map(str::to_string),
        relay_url: relay_url.to_string(),
        // Blank is absent: a name made of spaces is not a `git` author.
        display_name: display_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        pack_dir,
        persona_id,
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

/// Stage a managed agent's identity — and its role pack — for one exact
/// coding-session create.
///
/// Refuses when the agent is unknown or its secret is unavailable (a keyring
/// outage), so a create naming an actor the provider could never impersonate
/// is never published.
///
/// The role pack is resolved here rather than passed in: the caller names an
/// agent, and this computer decides which directory that agent's skills come
/// from. The returned [`StagedActorSeat`] says whether one was found, because
/// a seat launched with no pack carries no role skills and the screen has to
/// be able to say so.
#[tauri::command]
pub async fn stage_coding_session_actor_seat(
    app: AppHandle,
    state: State<'_, AppState>,
    command_id: String,
    agent_pubkey: String,
) -> Result<StagedActorSeat, String> {
    let relay_url = relay_ws_url_with_override(&state);
    let Some(path) = actor_seats_file_path(&app, &state)? else {
        return Err(
            "This computer has no coding-session provider yet, so an agent cannot be seated."
                .to_string(),
        );
    };
    let pubkey = agent_pubkey.trim().to_string();
    let teams = crate::managed_agents::teams::load_teams(&app).unwrap_or_default();
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
            record
                .display_name
                .as_deref()
                .or(Some(record.name.as_str())),
            resolve_seat_pack(record, &teams),
        )?
    };
    let staged = StagedActorSeat {
        pack_staged: entry.pack_dir.is_some(),
    };
    let mut file = read_actor_seats(&path)?;
    stage_actor_seat(&mut file, &command_id, entry)?;
    write_actor_seats(&path, &file)?;
    Ok(staged)
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

    fn agent_record(
        source_team: Option<&str>,
        slug: Option<&str>,
    ) -> crate::managed_agents::types::ManagedAgentRecord {
        crate::managed_agents::types::AgentDefinition {
            id: "def".into(),
            display_name: "Builder".into(),
            avatar_url: None,
            system_prompt: String::new(),
            runtime: None,
            model: None,
            provider: None,
            name_pool: vec![],
            is_builtin: false,
            is_active: true,
            shared: false,
            source_team: source_team.map(str::to_owned),
            source_team_persona_slug: slug.map(str::to_owned),
            catalog_source: None,
            env_vars: Default::default(),
            respond_to: None,
            respond_to_allowlist: vec![],
            parallelism: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
        .into_agent_record()
    }

    fn team_record(
        id: &str,
        source_dir: Option<PathBuf>,
    ) -> crate::managed_agents::types::TeamRecord {
        crate::managed_agents::types::TeamRecord {
            id: id.into(),
            name: id.into(),
            description: None,
            instructions: None,
            persona_ids: vec![],
            crew: None,
            is_builtin: false,
            source_dir,
            is_symlink: false,
            symlink_target: None,
            version: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// A directory that really is a role pack, with one persona in it.
    fn role_pack(root: &Path, persona: &str) -> PathBuf {
        let pack = root.join("pack");
        std::fs::create_dir_all(pack.join(".plugin")).expect("plugin dir");
        std::fs::create_dir_all(pack.join("personas")).expect("personas dir");
        std::fs::write(
            pack.join(".plugin/plugin.json"),
            format!(
                r#"{{"id":"com.test.roles","name":"Roles","version":"0.1.0","personas":["personas/{persona}.persona.md"]}}"#
            ),
        )
        .expect("plugin.json");
        std::fs::write(
            pack.join(format!("personas/{persona}.persona.md")),
            format!(
                "---\nname: {persona}\ndisplay_name: {persona}\ndescription: Builds.\nrole: builder\n---\nYou build.\n"
            ),
        )
        .expect("persona");
        pack
    }

    #[test]
    fn a_seat_carries_the_pack_its_agent_was_installed_from() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let dir = role_pack(tmp.path(), "builder");
        let record = agent_record(Some("team-1"), Some("builder"));
        let teams = vec![team_record("team-1", Some(dir.clone()))];
        let pack =
            resolve_seat_pack(&record, &teams).expect("the pack is host-local, not on the wire");
        assert_eq!(pack.0, dir);
        assert_eq!(pack.1, "builder");

        let entry = build_actor_seat_entry(
            PUBKEY,
            "nsec1secret",
            None,
            "wss://relay.example",
            None,
            Some(pack),
        )
        .expect("seat");
        let json = serde_json::to_value(&entry).expect("serialize");
        assert_eq!(
            json.get("packDir").and_then(|v| v.as_str()),
            Some(dir.to_string_lossy().as_ref()),
            "the provider reads packDir/personaId off this entry"
        );
        assert_eq!(
            json.get("personaId").and_then(|v| v.as_str()),
            Some("builder")
        );
    }

    #[test]
    fn a_pack_that_does_not_hold_the_persona_stages_no_pack() {
        // The provenance fallback used to pair the agent's slug with whatever
        // directory its team names, checking only that the directory exists.
        // An agent whose definition arrived from another device carries the
        // 30175 d-tag uuid as its slug (persona_events.rs), and no pack has a
        // persona by that name — so the seat was staged with a pack the
        // provider cannot read, and the provider's materialize_seat_skills
        // turns that into CreateFailure{PROVIDER_UNAVAILABLE}: a create that
        // worked before role packs existed, refused afterwards. A seat with no
        // readable pack is a packless seat, not a refused create.
        let tmp = tempfile::tempdir().expect("temp dir");
        let dir = role_pack(tmp.path(), "builder");
        let teams = vec![team_record("team-1", Some(dir.clone()))];

        assert!(
            resolve_seat_pack(
                &agent_record(Some("team-1"), Some("9f2c8d1e-inbound-uuid")),
                &teams,
            )
            .is_none(),
            "a slug the pack has no persona for is not a pack"
        );
        // Same for a renamed or removed persona reached by the instance-side
        // link rather than the provenance fallback.
        let mut linked = agent_record(None, None);
        linked.persona_team_dir = Some(dir.clone());
        linked.persona_name_in_team = Some("architect".into());
        assert!(
            resolve_seat_pack(&linked, &teams).is_none(),
            "a persona no longer in the pack is not a pack"
        );
        // A directory that is not a pack at all.
        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(&plain).expect("plain dir");
        assert!(
            resolve_seat_pack(
                &agent_record(Some("team-2"), Some("builder")),
                &[team_record("team-2", Some(plain))],
            )
            .is_none(),
            "an ordinary directory is not a pack"
        );
        // The persona the pack really holds still resolves.
        assert_eq!(
            resolve_seat_pack(&agent_record(Some("team-1"), Some("builder")), &teams),
            Some((dir, "builder".to_owned())),
        );
    }

    #[test]
    fn an_agent_with_no_pack_on_this_computer_stages_no_pack() {
        // No provenance at all.
        assert!(resolve_seat_pack(&agent_record(None, None), &[]).is_none());
        // A slug whose team is JSON-only: there is no directory to read.
        assert!(resolve_seat_pack(
            &agent_record(Some("team-1"), Some("builder")),
            &[team_record("team-1", None)],
        )
        .is_none());
        // A team directory that no longer exists is not a pack either.
        assert!(resolve_seat_pack(
            &agent_record(Some("team-1"), Some("builder")),
            &[team_record("team-1", Some(PathBuf::from("/nope/not/here")))],
        )
        .is_none());

        let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r", None, None)
            .expect("seat");
        let json = serde_json::to_value(&entry).expect("serialize");
        assert!(
            json.get("packDir").is_none() && json.get("personaId").is_none(),
            "a packless seat writes no pack keys: {json}"
        );
    }

    #[test]
    fn a_seat_without_a_key_in_the_keyring_is_refused() {
        let error = build_actor_seat_entry(PUBKEY, "", None, "wss://relay.example", None, None)
            .expect_err("an empty nsec must refuse the seat");
        assert!(error.contains("keyring"), "unexpected refusal: {error}");
        assert!(
            error.contains(PUBKEY),
            "refusal must name the agent: {error}"
        );
        // Whitespace is not a key either.
        assert!(
            build_actor_seat_entry(PUBKEY, "   ", None, "wss://relay.example", None, None).is_err()
        );
    }

    #[test]
    fn a_seat_pubkey_must_be_lowercase_hex() {
        assert!(
            build_actor_seat_entry("not-a-pubkey", "nsec1x", None, "wss://r", None, None).is_err()
        );
        assert!(build_actor_seat_entry(
            &PUBKEY.to_uppercase(),
            "nsec1x",
            None,
            "wss://r",
            None,
            None
        )
        .is_err());
    }

    #[test]
    fn the_file_shape_is_the_providers_read_contract() {
        let mut file = ActorSeatsFile::default();
        let entry = build_actor_seat_entry(
            PUBKEY,
            "nsec1secret",
            None,
            "wss://relay.example",
            None,
            None,
        )
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

    /// Ledger 77 (*Fence*, b): the provider turns this into the seat's `git`
    /// author and committer, so a seated execution's commits are attributed to
    /// the agent instead of to the operator whose home directory it runs in.
    #[test]
    fn a_seat_carries_the_agents_display_name_for_its_git_identity() {
        let entry = build_actor_seat_entry(
            PUBKEY,
            "nsec1secret",
            None,
            "wss://relay.example",
            Some("Levain"),
            None,
        )
        .expect("seat");
        let json = serde_json::to_value(&entry).expect("serialize");
        assert_eq!(
            json.get("displayName").and_then(|v| v.as_str()),
            Some("Levain"),
            "the provider reads displayName off this entry: {json}"
        );

        // Blank is absent, not a name made of spaces: an empty `git` author is
        // worse than falling back to the seat's role.
        let blank = build_actor_seat_entry(
            PUBKEY,
            "nsec1secret",
            None,
            "wss://relay.example",
            Some("   "),
            None,
        )
        .expect("seat");
        assert_eq!(blank.display_name, None);
        assert!(
            serde_json::to_value(&blank)
                .expect("serialize")
                .get("displayName")
                .is_none(),
            "a nameless seat writes no displayName key"
        );
    }

    #[test]
    fn an_auth_tag_travels_verbatim() {
        let entry = build_actor_seat_entry(
            PUBKEY,
            "nsec1secret",
            Some("[\"tag\",\"value\"]"),
            "wss://relay.example",
            None,
            None,
        )
        .expect("seat");
        assert_eq!(entry.auth_tag.as_deref(), Some("[\"tag\",\"value\"]"));
    }

    #[test]
    fn staging_needs_a_command_id() {
        let mut file = ActorSeatsFile::default();
        let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r", None, None)
            .expect("seat");
        assert!(stage_actor_seat(&mut file, "  ", entry.clone()).is_err());
        assert!(stage_actor_seat(&mut file, &"c".repeat(257), entry).is_err());
        assert!(file.pending.is_empty());
    }

    #[test]
    fn clearing_reports_whether_the_provider_beat_us_to_it() {
        let mut file = ActorSeatsFile::default();
        let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r", None, None)
            .expect("seat");
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
        let entry = build_actor_seat_entry(PUBKEY, "nsec1secret", None, "wss://r", None, None)
            .expect("seat");
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
