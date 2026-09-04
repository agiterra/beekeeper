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
//!     "personaId": "builder" | absent,
//!     "packRef": { "repo": "30617:…", "sha": "<40-hex>",
//!                  "role": "builder", "path": "personas/roles/builder" }
//!                | absent } } }
//! ```
//!
//! `packDir`/`personaId` are the seat's role pack (contract D8-A). They travel
//! by this file for the same reason the nsec does — not because they are
//! secret, but because a *path on this machine* is host-local. They are
//! resolved here, in Rust, from the seat's **role**; the webview never sends a
//! path, so a compromised or merely wrong renderer cannot point the provider's
//! skill materialization at a directory of its choosing.
//!
//! `packRef` is the wire's account of that pack — repository, exact commit,
//! role and path — present only when the pack came from the project's packs
//! repository (`managed_agents::packs_cache`). The provider republishes it
//! verbatim on the seat's kind:44223, so a reader can answer "which pack ran"
//! from the wire rather than from a claim.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::managed_agents::packs_cache;
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
    /// The wire's account of the pack, when it came from a project's packs
    /// repository rather than from this computer's own installation.
    ///
    /// Absent for a locally-installed pack and for a packless seat: the
    /// provider publishes this verbatim as the seat's kind:44223 `packRef`,
    /// and a `packRef` naming a repository the pack did not come from would be
    /// a proof of the wrong thing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_ref: Option<crate::managed_agents::packs_cache::PackRef>,
}

/// What one staging call did, for a caller that has to be honest about it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StagedActorSeat {
    /// Whether a role pack was staged with the seat. `false` means the seat
    /// will run with its prompt alone: no `.agents/skills` will appear in its
    /// working directory, and any copy claiming otherwise is wrong.
    pub pack_staged: bool,
    /// The wire's account of the pack, when it came from a project's packs
    /// repository. `None` for a pack installed on this computer and for a
    /// packless seat — the screen then has no repository to name and must not
    /// invent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_ref: Option<packs_cache::PackRef>,
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

/// The role a pack's persona declares inside `dir`, if it declares one.
///
/// Read from the persona's own frontmatter through the resolver the provider
/// will use, never from a directory name or an agent's record.
fn persona_declared_role(dir: &Path, persona: &str) -> Option<String> {
    buzz_persona_pkg::resolve::resolve_persona_by_name(dir, persona)
        .ok()?
        .role
        .map(|role| role.trim().to_owned())
        .filter(|role| !role.is_empty())
}

/// The pack this computer would stage for a seat **created with `seat_role`**.
///
/// The seat's role picks the pack. The actor's home role is not consulted:
/// identity is who signs, role is what the seat is for, and a `builder`
/// identity seated as `architect` is an architect for that execution. Staging
/// the builder's pack there handed the seat the wrong craft while every screen
/// said `architect` — the staging bug this function exists to close
/// (`docs/CREW_FRONT_DOOR.md`, *Rules from the lead*).
///
/// Resolution, in order, and each step is a fact rather than a guess:
///
/// 1. The actor's own pack, when the actor's home role **is** the seat's role.
/// 2. Any pack installed on this computer whose agent declares the seat's
///    role — the crew-role installer mints one agent per role pack, so a
///    machine that has the roles has the packs.
/// 3. The actor's own pack when that pack claims no role at all. A persona
///    with no `role:` makes no claim this could contradict; a persona that
///    declares a *different* role is refused, because staging it would be the
///    original bug by another route.
///
/// `None` — never another role's pack — when none of those hold. A seat with
/// no pack is a legal seat that carries no role skills, and the screen says so.
pub(crate) fn resolve_local_seat_pack(
    record: &crate::managed_agents::types::ManagedAgentRecord,
    records: &[crate::managed_agents::types::ManagedAgentRecord],
    teams: &[crate::managed_agents::types::TeamRecord],
    seat_role: Option<&str>,
) -> Option<(PathBuf, String)> {
    let Some(role) = seat_role.map(str::trim).filter(|role| !role.is_empty()) else {
        // An unseated-by-role create keeps the behaviour it has always had.
        return resolve_seat_pack(record, teams);
    };
    if record.home_role.as_deref().map(str::trim) == Some(role) {
        if let Some(pack) = resolve_seat_pack(record, teams) {
            return Some(pack);
        }
    }
    if let Some(pack) = records
        .iter()
        .filter(|other| other.home_role.as_deref().map(str::trim) == Some(role))
        .find_map(|other| resolve_seat_pack(other, teams))
    {
        return Some(pack);
    }
    let (dir, persona) = resolve_seat_pack(record, teams)?;
    match persona_declared_role(&dir, &persona) {
        // The pack claims a different role: not this seat's pack.
        Some(declared) if declared != role => {
            tracing::debug!(
                pack = %dir.display(),
                persona = %persona,
                %declared,
                seat_role = %role,
                "seat stages no role pack: the agent's pack is another role's"
            );
            None
        }
        _ => Some((dir, persona)),
    }
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
    pack_ref: Option<crate::managed_agents::packs_cache::PackRef>,
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
    let pack_present = pack_dir.is_some();
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
        // A pack that was not staged cannot be described. `packRef` rides
        // exactly the pack it names or it does not ride at all.
        pack_ref: pack_ref.filter(|_| pack_present),
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

/// A project's kind:30624 pack source, as the webview hands it over.
///
/// The renderer reads the signed record off the relay — it is an ordinary
/// addressable event it already subscribes to — and passes the *decoded tags*
/// here. Not a path: every field is validated in Rust
/// ([`packs_cache::parse_repo_coordinate`], [`packs_cache::validate_pack_path`])
/// before it can reach `git`, and the checkout is confined to this host's own
/// packs cache. The security property the module docs state — the webview
/// never names a directory on this machine — is unchanged.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPackSourceInput {
    /// `30617:<owner-hex>:<id>`.
    pub repo: String,
    /// `refs/heads/main`, when the source follows a branch.
    #[serde(default)]
    pub git_ref: Option<String>,
    /// A pinned commit, when the source pins one.
    #[serde(default)]
    pub sha: Option<String>,
    /// Sub-path holding the role directories; the default when absent.
    #[serde(default)]
    pub path: Option<String>,
}

impl From<ProjectPackSourceInput> for packs_cache::ProjectPackSource {
    fn from(input: ProjectPackSourceInput) -> Self {
        Self {
            repo: input.repo,
            git_ref: input.git_ref,
            sha: input.sha,
            path: input
                .path
                .unwrap_or_else(|| packs_cache::DEFAULT_PACK_PATH.to_string()),
        }
    }
}

/// Where a seat's pack comes from, as one word the screen can render.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SeatPackOrigin {
    /// The project's packs repository, at the commit `packRef` names.
    Project,
    /// The session's own checkout, `<checkout>/personas/roles/<role>`.
    Checkout,
    /// A pack installed on this computer for the seat's role.
    Installed,
    /// The packs bundled into this build of the app — the last fallback, so a
    /// person who installs Beekeeper on a second machine and hires an
    /// architect gets an architect without reading a runbook.
    Shipped,
    /// No pack — the seat runs on its persona prompt alone.
    None,
}

/// What this computer would stage for one seat, without staging it.
///
/// The honest answer to "which pack will this seat run with", for the hire and
/// launch dialogs to show *before* anything is signed. Every field is a fact
/// this host can produce on its own; nothing is asked of an agent.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatPackPreview {
    /// Whether a pack would be staged at all.
    pub pack_staged: bool,
    /// Where it would come from.
    pub origin: SeatPackOrigin,
    /// The seat role this preview was computed for, or `null` when the caller
    /// named none (in which case the actor's own pack is the answer).
    pub role: Option<String>,
    /// Absolute directory that would be staged, or `null`.
    pub pack_dir: Option<String>,
    /// The persona inside it, or `null`.
    pub persona_id: Option<String>,
    /// The wire's `packRef` for this seat, or `null` when the pack is local to
    /// this computer and no repository can vouch for it.
    pub pack_ref: Option<packs_cache::PackRef>,
    /// The sentence a hire would be refused with, or `null` when it would go
    /// ahead. Present exactly when the project names a packs source this
    /// computer could not stage.
    pub refusal: Option<String>,
    /// The underlying reason behind `refusal`, for a log line or a details
    /// disclosure. `null` when there is no refusal.
    pub reason: Option<String>,
}

/// Resolve the seat pack for `agent_pubkey` at `role`, project source first.
///
/// The one place the staging rule lives, shared by the preview command and the
/// staging command so the dialog cannot promise a pack the create then fails
/// to stage.
fn plan_seat_pack(
    app: &AppHandle,
    state: &AppState,
    records: &[crate::managed_agents::types::ManagedAgentRecord],
    record: &crate::managed_agents::types::ManagedAgentRecord,
    role: Option<&str>,
    pack_source: Option<packs_cache::ProjectPackSource>,
    checkout: Option<&Path>,
) -> SeatPackPreview {
    let role = role
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .map(str::to_owned);
    if let Some(source) = pack_source {
        let Some(role) = role.clone() else {
            return SeatPackPreview {
                pack_staged: false,
                origin: SeatPackOrigin::None,
                role: None,
                pack_dir: None,
                persona_id: None,
                pack_ref: None,
                refusal: Some(packs_cache::HIRE_PACK_UNAVAILABLE.to_string()),
                reason: Some(
                    "this project stages packs by role, and the seat was given none".to_string(),
                ),
            };
        };
        let staged = packs_cache::packs_root(app).and_then(|root| {
            let auth = crate::commands::project_git_exec::build_git_auth_config(state)?;
            let relay_http = crate::relay::relay_http_base_url(&relay_ws_url_with_override(state));
            packs_cache::stage_project_role_pack(&root, &relay_http, &source, &role, &auth)
        });
        return match staged {
            Ok(pack) => SeatPackPreview {
                pack_staged: true,
                origin: SeatPackOrigin::Project,
                role: Some(role),
                pack_dir: Some(pack.dir.to_string_lossy().into_owned()),
                persona_id: Some(pack.persona),
                pack_ref: Some(pack.pack_ref),
                refusal: None,
                reason: None,
            },
            Err(reason) => SeatPackPreview {
                pack_staged: false,
                origin: SeatPackOrigin::None,
                role: Some(role),
                pack_dir: None,
                persona_id: None,
                pack_ref: None,
                refusal: Some(packs_cache::HIRE_PACK_UNAVAILABLE.to_string()),
                reason: Some(reason),
            },
        };
    }
    // No project record. Three fallbacks, in the order the addendum fixes:
    // the session's own checkout, then a pack installed on this computer, then
    // the packs this build ships. Each is a fact; none is a guess.
    if let Some((dir, persona)) = role
        .as_deref()
        .zip(checkout)
        .and_then(|(role, checkout)| packs_cache::checkout_role_pack(checkout, role))
    {
        return SeatPackPreview {
            pack_staged: true,
            origin: SeatPackOrigin::Checkout,
            role,
            pack_dir: Some(dir.to_string_lossy().into_owned()),
            persona_id: Some(persona),
            pack_ref: None,
            refusal: None,
            reason: None,
        };
    }
    let teams = crate::managed_agents::teams::load_teams(app).unwrap_or_default();
    match resolve_local_seat_pack(record, records, &teams, role.as_deref()).map_or_else(
        || {
            // The shipped packs, named on the wire as what they are: no
            // repository announces them, and the app's own version is what
            // pins them.
            let role = role.as_deref()?;
            let dir = packs_cache::shipped_packs_dir(app)?;
            let (dir, persona) = packs_cache::role_pack_in_checkout(&dir, "", role)?;
            Some((
                dir,
                persona,
                SeatPackOrigin::Shipped,
                Some(packs_cache::PackRef {
                    repo: packs_cache::PACK_REF_SHIPPED_REPO.to_string(),
                    sha: packs_cache::shipped_packs_version(app),
                    role: role.to_string(),
                    path: format!("{}/{role}", packs_cache::DEFAULT_PACK_PATH),
                }),
            ))
        },
        |(dir, persona)| {
            // An installed pack that *is* one of this build's shipped packs is
            // named as such on the wire. The installer points
            // `persona_team_dir` at the folder the operator chose, and on a
            // development build that folder is usually the checkout's own
            // `personas/roles` — the shipped directory under another name. A
            // seat staged from it published `packRef: null`, which said no one
            // could vouch for its pack while the app's own version could.
            // Recognition, not a guess: `shipped_pack_ref_for_dir` answers
            // `Some` only for `<shipped>/<role>` itself.
            let pack_ref = role.as_deref().and_then(|role| {
                packs_cache::shipped_pack_ref_for_dir(
                    packs_cache::shipped_packs_dir(app).as_deref(),
                    &dir,
                    role,
                    &packs_cache::shipped_packs_version(app),
                )
            });
            let origin = if pack_ref.is_some() {
                SeatPackOrigin::Shipped
            } else {
                SeatPackOrigin::Installed
            };
            Some((dir, persona, origin, pack_ref))
        },
    ) {
        Some((dir, persona, origin, pack_ref)) => SeatPackPreview {
            pack_staged: true,
            origin,
            role,
            pack_dir: Some(dir.to_string_lossy().into_owned()),
            persona_id: Some(persona),
            pack_ref,
            refusal: None,
            reason: None,
        },
        None => SeatPackPreview {
            pack_staged: false,
            origin: SeatPackOrigin::None,
            role,
            pack_dir: None,
            persona_id: None,
            pack_ref: None,
            refusal: None,
            reason: None,
        },
    }
}

/// What would be staged for this agent, at this role, in this project.
///
/// Read-only: it syncs the project's packs cache (so the answer is the answer,
/// not a hope) but writes no seat and publishes nothing. The hire and launch
/// dialogs call it to show the pack a seat will run with — and, when the
/// project's packs cannot be read, the sentence the hire will be refused with,
/// *before* the operator commits to it.
#[tauri::command]
pub async fn preview_coding_session_seat_pack(
    app: AppHandle,
    state: State<'_, AppState>,
    agent_pubkey: String,
    role: Option<String>,
    pack_source: Option<ProjectPackSourceInput>,
    checkout: Option<String>,
) -> Result<SeatPackPreview, String> {
    let pubkey = agent_pubkey.trim().to_string();
    let records = {
        let _store_guard = state
            .managed_agents_store_lock
            .lock()
            .map_err(|error| error.to_string())?;
        load_managed_agents(&app)?
    };
    let record = records
        .iter()
        .find(|record| record.pubkey == pubkey)
        .ok_or_else(|| format!("agent {pubkey} is not a managed agent on this computer"))?;
    let checkout = checkout
        .as_deref()
        .map(str::trim)
        .filter(|checkout| !checkout.is_empty())
        .map(Path::new);
    Ok(plan_seat_pack(
        &app,
        &state,
        &records,
        record,
        role.as_deref(),
        pack_source.map(Into::into),
        checkout,
    ))
}

/// Stage a managed agent's identity — and its role pack — for one exact
/// coding-session create.
///
/// Refuses when the agent is unknown or its secret is unavailable (a keyring
/// outage), so a create naming an actor the provider could never impersonate
/// is never published. Refuses with [`packs_cache::HIRE_PACK_UNAVAILABLE`]
/// when the project names a packs repository this computer cannot read the
/// seat's role out of: a seat started on a bare persona would look like a
/// working hire and behave like an agent that forgot its craft.
///
/// The role pack is resolved here rather than passed in: the caller names an
/// agent and a role, and this computer decides which directory that seat's
/// skills come from. **The seat's role picks the pack** — the actor's home
/// role is never consulted. The returned [`StagedActorSeat`] says whether one
/// was found, because a seat launched with no pack carries no role skills and
/// the screen has to be able to say so.
#[tauri::command]
pub async fn stage_coding_session_actor_seat(
    app: AppHandle,
    state: State<'_, AppState>,
    command_id: String,
    agent_pubkey: String,
    role: Option<String>,
    pack_source: Option<ProjectPackSourceInput>,
    checkout: Option<String>,
) -> Result<StagedActorSeat, String> {
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
        let plan = plan_seat_pack(
            &app,
            &state,
            &records,
            record,
            role.as_deref(),
            pack_source.map(Into::into),
            checkout
                .as_deref()
                .map(str::trim)
                .filter(|checkout| !checkout.is_empty())
                .map(Path::new),
        );
        if let Some(refusal) = plan.refusal {
            tracing::warn!(
                agent = %pubkey,
                role = role.as_deref().unwrap_or("<none>"),
                reason = plan.reason.as_deref().unwrap_or("<none>"),
                "refusing to seat an agent without the pack its project promised"
            );
            return Err(match plan.reason {
                Some(reason) => format!("{refusal} ({reason})"),
                None => refusal,
            });
        }
        build_actor_seat_entry(
            &record.pubkey,
            &record.private_key_nsec,
            record.auth_tag.as_deref(),
            &relay_url,
            record
                .display_name
                .as_deref()
                .or(Some(record.name.as_str())),
            plan.pack_dir
                .map(PathBuf::from)
                .zip(plan.persona_id.clone()),
            plan.pack_ref,
        )?
    };
    let staged = StagedActorSeat {
        pack_staged: entry.pack_dir.is_some(),
        pack_ref: entry.pack_ref.clone(),
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
#[path = "actor_seats_tests.rs"]
mod tests;
