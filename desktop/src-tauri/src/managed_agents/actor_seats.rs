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
use crate::managed_agents::project_agent_association::{new_seat_refusal, refused_seat_preview};
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
/// The seat's role picks the pack, and the pack's own persona has the last
/// word. **Home roles order the search; the declared role decides.** An
/// agent's record says what it was installed as; its pack says what it is
/// today, read through `persona_declared_role` from the persona's frontmatter.
/// A candidate is staged only when that declared role is the seat's role (or,
/// on the last step, when the persona declares none). Before this rule, a
/// `verifier`-labelled agent whose pack now declared `builder` staged that
/// pack into a verifier seat on the strength of its label alone
/// (finding 94, `review-2026-09-01/LIVE-RUN-TeamRolesV1.md`).
///
/// Identity is who signs, role is what the seat is for, and a `builder`
/// identity seated as `architect` is an architect for that execution. Staging
/// the builder's pack there handed the seat the wrong craft while every
/// screen said `architect` — the staging bug this function exists to close
/// (`docs/CREW_FRONT_DOOR.md`, *Rules from the lead*).
///
/// Resolution, in order, and each step is a fact rather than a guess:
///
/// 1. The actor's own pack, when the actor's home role is the seat's role
///    **and** its persona declares that role.
/// 2. Any pack installed on this computer whose agent's home role is the
///    seat's role, whose agent has the actor's project association (never
///    another project's pack), **and** whose persona declares it — the crew-role installer
///    mints one agent per role pack, so a machine that has the roles has the
///    packs.
/// 3. The actor's own pack when that pack claims no role at all. A persona
///    with no `role:` makes no claim this could contradict; a persona that
///    declares a *different* role is refused, because staging it would be the
///    original bug by another route.
///
/// A candidate that fails the declared-role check on step 1 or 2 falls
/// through to the next step; it is never returned on its label.
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
        if let Some(pack) = resolve_pack_declaring_role(record, teams, role) {
            return Some(pack);
        }
    }
    // Only another agent of the same project association (or another agent of
    // no project, for an unassociated actor): a seat never stages a different
    // project's role pack on the strength of a shared role name.
    let project = record
        .project_ref
        .as_deref()
        .and_then(crate::managed_agents::project_agent_association::normalize_project_ref);
    if let Some(pack) =
        records
            .iter()
            .filter(|other| other.home_role.as_deref().map(str::trim) == Some(role))
            .filter(|other| {
                other.project_ref.as_deref().and_then(
                    crate::managed_agents::project_agent_association::normalize_project_ref,
                ) == project
            })
            .find_map(|other| resolve_pack_declaring_role(other, teams, role))
    {
        return Some(pack);
    }
    let (dir, persona) = resolve_seat_pack(record, teams)?;
    match persona_declared_role(&dir, &persona) {
        // The pack claims a different role: not this seat's pack.
        Some(declared) if declared != role => {
            refuse_another_roles_pack(&dir, &persona, &declared, role);
            None
        }
        _ => Some((dir, persona)),
    }
}

/// `resolve_seat_pack`, accepted only when the persona **declares** `role`.
///
/// The home-role steps of `resolve_local_seat_pack` go through here, so a
/// label on an agent's record is never enough on its own: a readable pack
/// whose persona declares another role is refused with the same debug line
/// the final step uses, and one whose persona declares no role is left for
/// that final step to judge.
fn resolve_pack_declaring_role(
    record: &crate::managed_agents::types::ManagedAgentRecord,
    teams: &[crate::managed_agents::types::TeamRecord],
    role: &str,
) -> Option<(PathBuf, String)> {
    let (dir, persona) = resolve_seat_pack(record, teams)?;
    match persona_declared_role(&dir, &persona) {
        Some(declared) if declared == role => Some((dir, persona)),
        Some(declared) => {
            refuse_another_roles_pack(&dir, &persona, &declared, role);
            None
        }
        None => {
            tracing::debug!(
                pack = %dir.display(),
                persona = %persona,
                seat_role = %role,
                "home role alone stages no role pack: the persona declares no role"
            );
            None
        }
    }
}

/// The one debug line every refusal of another role's pack emits.
fn refuse_another_roles_pack(dir: &Path, persona: &str, declared: &str, role: &str) {
    tracing::debug!(
        pack = %dir.display(),
        persona = %persona,
        %declared,
        seat_role = %role,
        "seat stages no role pack: the agent's pack is another role's"
    );
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

/// Mutate the latest custody map under the lock shared with the provider.
///
/// Lock a stable sibling, never the atomically replaced JSON inode. Hold it
/// only for local read/mutate/write; resolve packs and keyring data beforehand.
pub(crate) fn mutate_actor_seats_file<T>(
    path: &Path,
    update: impl FnOnce(&mut ActorSeatsFile) -> Result<T, String>,
) -> Result<T, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options
        .open(path.with_extension("lock"))
        .map_err(|error| format!("cannot open seat custody lock: {error}"))?;
    lock.lock()
        .map_err(|error| format!("cannot lock seat custody: {error}"))?;
    let mut file = read_actor_seats(path)?;
    let result = update(&mut file)?;
    write_actor_seats(path, &file)?;
    Ok(result)
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
    /// What the composer wanted said while staging — a deprecated template,
    /// a `![[` that was not on its own line. Never a refusal.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// The staged composition's `sha256:…` digest, or `null` when nothing
    /// was staged or the seat runs an uncomposed pack (a seat with no role).
    #[serde(default)]
    pub compose_digest: Option<String>,
    /// Where the composed bytes came from, in the composer's vocabulary:
    /// `repository` (the project's pinned source), `branch-override` (the
    /// seat's own branch changed this role — spec § 4.9), `shipped`, or
    /// `local`. `null` when nothing was staged.
    #[serde(default)]
    pub source_kind: Option<String>,
    /// Whether a seat in this role may see the `beekeeper/` directory in its
    /// worktree (`team.yml` `workspace.roles_visible`, spec § 4.10); the
    /// worktree cut reads it to decide `hide_roles`. `false` when nothing
    /// was staged or the source has no manifest.
    #[serde(default)]
    pub roles_visible: bool,
}

// The staging rule itself lives in `seat_pack_plan.rs`; re-exported so every
// caller keeps naming it here, beside the seat types it produces.
pub(crate) use crate::managed_agents::seat_pack_plan::plan_seat_pack;

/// Name an installed pack on the wire, when this build can vouch for it.
///
/// An installed pack that *is* one of this build's shipped packs is named as
/// such: the installer points `persona_team_dir` at the folder the operator
/// chose, and on a development machine that folder is the checkout's own
/// `personas/roles` — the very bytes `tauri-build` copied into the target
/// directory that [`packs_cache::shipped_packs_dir`] answers with. A seat
/// staged from it published `packRef: null` (finding 72: every lead 44223 on
/// runs 6 and 7, both machines), which said no one could vouch for its pack
/// while the app's own version could. Recognition, not a guess:
/// [`packs_cache::shipped_pack_ref_for_dir`] answers `Some` only for
/// `<shipped>/<role>` itself, by path or by bytes. Anything else is
/// [`SeatPackOrigin::Installed`] with nothing on the wire to name it.
///
/// The seat file carries whatever this returns, verbatim, and the provider
/// republishes it on every 44223 of the generation — so this is the one place
/// the lead's `packRef` is decided, and the status is never re-derived from
/// it.
pub(crate) fn installed_seat_pack_ref(
    shipped_root: Option<&Path>,
    shipped_version: &str,
    dir: &Path,
    role: Option<&str>,
) -> (SeatPackOrigin, Option<packs_cache::PackRef>) {
    let pack_ref = role.and_then(|role| {
        packs_cache::shipped_pack_ref_for_dir(shipped_root, dir, role, shipped_version)
    });
    let origin = if pack_ref.is_some() {
        SeatPackOrigin::Shipped
    } else {
        SeatPackOrigin::Installed
    };
    (origin, pack_ref)
}

/// What would be staged for this agent, at this role, in this project.
///
/// Read-only: it syncs the project's packs cache (so the answer is the answer,
/// not a hope) but writes no seat and publishes nothing. The hire and launch
/// dialogs call it to show the pack a seat will run with — and, when the
/// project's packs cannot be read, the sentence the hire will be refused with,
/// *before* the operator commits to it. `require_project_ref` (a new
/// selection's project) answers a [`SEAT_NOT_PROJECT_AGENT`](crate::managed_agents::project_agent_association::SEAT_NOT_PROJECT_AGENT) refusal for an
/// agent that is not that project's. `new_selection: Some(true)` applies the
/// full new-seat rule ([`new_seat_refusal`]): a projectless session takes no
/// project's agent, and the seat role must be the agent's primary role.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri state handles plus the fixed IPC shape.
pub async fn preview_coding_session_seat_pack(
    app: AppHandle,
    state: State<'_, AppState>,
    agent_pubkey: String,
    role: Option<String>,
    pack_source: Option<ProjectPackSourceInput>,
    checkout: Option<String>,
    require_project_ref: Option<String>,
    new_selection: Option<bool>,
    worktree: Option<String>,
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
    if let Some(refusal) = new_seat_refusal(
        record,
        role.as_deref(),
        require_project_ref.as_deref(),
        new_selection,
    ) {
        return Ok(refused_seat_preview(role.as_deref(), &refusal));
    }
    let checkout = checkout
        .as_deref()
        .map(str::trim)
        .filter(|checkout| !checkout.is_empty())
        .map(Path::new);
    let worktree = worktree
        .as_deref()
        .map(str::trim)
        .filter(|worktree| !worktree.is_empty())
        .map(Path::new);
    Ok(plan_seat_pack(
        &app,
        &state,
        &records,
        record,
        role.as_deref(),
        pack_source.map(Into::into),
        checkout,
        worktree,
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
/// the screen has to be able to say so. A new selection passes its project as
/// `require_project_ref`; an agent that is not that project's is refused with
/// [`SEAT_NOT_PROJECT_AGENT`](crate::managed_agents::project_agent_association::SEAT_NOT_PROJECT_AGENT), and passes `new_selection: Some(true)` for the
/// full new-seat rule ([`new_seat_refusal`]). Resume and restage pass neither.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri state handles plus the fixed IPC shape.
pub async fn stage_coding_session_actor_seat(
    app: AppHandle,
    state: State<'_, AppState>,
    command_id: String,
    agent_pubkey: String,
    role: Option<String>,
    pack_source: Option<ProjectPackSourceInput>,
    checkout: Option<String>,
    require_project_ref: Option<String>,
    new_selection: Option<bool>,
    worktree: Option<String>,
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
        if crate::managed_agents::project_team_setup::actor::restage::is_setup_actor(record) {
            return Err(
                crate::managed_agents::project_team_setup::actor::restage::SCOPED_STAGE_REQUIRED
                    .to_string(),
            );
        }
        if let Some(refusal) = new_seat_refusal(
            record,
            role.as_deref(),
            require_project_ref.as_deref(),
            new_selection,
        ) {
            return Err(refusal);
        }
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
            worktree
                .as_deref()
                .map(str::trim)
                .filter(|worktree| !worktree.is_empty())
                .map(Path::new),
        );
        seat_entry_for_plan(record, &relay_url, plan)?
    };
    let staged = StagedActorSeat::of(&entry);
    mutate_actor_seats_file(&path, |file| stage_actor_seat(file, &command_id, entry))?;
    Ok(staged)
}

impl StagedActorSeat {
    /// What the webview is told about `entry`: the same `packRef` the
    /// provider will read off the custody file, never a second opinion.
    pub(crate) fn of(entry: &ActorSeatEntry) -> Self {
        Self {
            pack_staged: entry.pack_dir.is_some(),
            pack_ref: entry.pack_ref.clone(),
        }
    }
}

/// Turn one seat plan into the custody entry [`stage_actor_seat`] files, or
/// the refusal the plan carries.
///
/// This is the boundary between "what this computer decided to stage"
/// ([`plan_seat_pack`], project source first) and "what the provider will
/// read": the plan's `pack_dir`/`persona_id` become the entry's pack and its
/// `pack_ref` rides the entry verbatim, so a seat staged from the project's
/// repository is stamped with that repository's commit and nothing else.
pub(crate) fn seat_entry_for_plan(
    record: &crate::managed_agents::types::ManagedAgentRecord,
    relay_url: &str,
    plan: SeatPackPreview,
) -> Result<ActorSeatEntry, String> {
    if let Some(refusal) = plan.refusal {
        tracing::warn!(
            agent = %record.pubkey,
            role = plan.role.as_deref().unwrap_or("<none>"),
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
        relay_url,
        record
            .display_name
            .as_deref()
            .or(Some(record.name.as_str())),
        plan.pack_dir.map(PathBuf::from).zip(plan.persona_id),
        plan.pack_ref,
    )
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
    mutate_actor_seats_file(&path, |file| {
        clear_actor_seat(file, &command_id);
        Ok(())
    })
}

#[cfg(test)]
#[path = "actor_seats_tests.rs"]
mod tests;
