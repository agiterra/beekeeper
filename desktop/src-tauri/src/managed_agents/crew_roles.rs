//! Turning a folder of role packs into agents that carry their role.
//!
//! A **role pack** is a directory holding `.plugin/plugin.json` whose persona
//! declares `role:` in its frontmatter (`personas/roles/**` in this repo). Until
//! this module existed, nothing on this computer turned such a pack into
//! anything: `AgentDefinition::into_agent_record` writes `persona_team_dir:
//! None` / `persona_name_in_team: None`, so
//! [`crate::managed_agents::actor_seats::resolve_seat_pack`] resolved nothing
//! for every agent, and every seat staged **without** its pack.
//!
//! The installer here mints, for each role pack, one definition and one managed
//! agent whose `home_role` is the persona's declared role and whose
//! `persona_team_dir`/`persona_name_in_team` point at the pack — so the seat
//! that agent fills stages *with* its role skills. All of them join one team
//! named [`CREW_ROLES_TEAM_NAME`] carrying a crew block.
//!
//! Two things this module deliberately does **not** do:
//!
//! * It never guesses a `home_role` from a name, a slug, or a team. The role
//!   comes from the pack persona's frontmatter or the agent has none.
//! * The team's `source_dir` stays `None`. `delete_team_with_cascade` does
//!   `fs::remove_dir_all(source_dir)`, so pointing the team at the folder of
//!   packs would make "Delete team" delete the operator's checkout. The pack
//!   link lives on each agent instead.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::managed_agents::{
    team_events::{TeamCrew, TeamCrewSeat},
    types::{AgentDefinition, ManagedAgentRecord},
    TeamRecord,
};

#[path = "crew_roles_publish.rs"]
mod publish;
pub(crate) use publish::*;

#[path = "crew_roles_project.rs"]
mod project;
pub(crate) use project::install_role_packs_in_named_team;
pub(crate) use project::NameScope;
#[path = "crew_roles_runtime.rs"]
mod default_runtime;
use default_runtime::derive_harness_fields;
pub(crate) use default_runtime::pin_default_runtime;

/// Name of the one team every installed role pack joins. Also the dedupe key:
/// a second run updates this team rather than minting `Team roles (2)`.
pub const CREW_ROLES_TEAM_NAME: &str = "Team roles";

/// What this team was called before "crew" became "team" in every string a
/// person reads. Matched on install so a computer that already holds one is
/// renamed in place rather than given a second team beside it.
pub const LEGACY_CREW_ROLES_TEAM_NAME: &str = "Crew roles";

/// Roles seated by default, in launch order.
///
/// `poker`, `designer` and `verifier` packs are installed as agents but left
/// unseated: a poker drives the built app (screenshots, e2e), which a seated
/// coding-session execution cannot do; a designer works at brief time, before
/// a team exists; and a verifier cannot be seated *by this installer* at all —
/// every seat of one launch is created against the single
/// `providerInstanceRef` the dialog selected, so every seat runs on that
/// runtime's vendor, and contract D8 refuses a verifier sharing a builder's
/// vendor. Seating one here made the installed roster unlaunchable by
/// construction (SESSION_STATE item 77, F7). The pack is installed so a roster
/// launched across two providers can seat it.
pub const CREW_SEAT_ROSTER: [&str; 4] = ["lead", "architect", "builder", "runner"];

/// ACP driver slug a seat installed here declares.
///
/// The `claude-primary` coding-session runtime is the one this desktop always
/// offers (`session_provider::runtimes`), so it is the runtime a launch from
/// this computer uses unless the operator picks another — and a launch that
/// *does* pick another is refused by `checkCodingSessionCrewSeatModels`, which
/// compares the seat's declared vendor against the selected provider. The seat
/// therefore states a runtime rather than leaving the family rule with nothing
/// to read.
pub const DEFAULT_CREW_SEAT_DRIVER: &str = "claude-agent-acp";

/// Model vendor each provider-locked ACP driver necessarily runs on.
///
/// Only drivers that can run exactly one vendor belong here: the Claude Code
/// adapter talks to Anthropic and the Codex adapter to OpenAI, whatever model
/// alias is chosen. Goose and `buzz-agent` take their provider from
/// configuration, so no vendor can be stated for them and none is.
const DRIVER_VENDORS: [(&str, &str); 3] = [
    ("claude-agent-acp", "anthropic"),
    ("claude-code-acp", "anthropic"),
    ("codex-acp", "openai"),
];

/// The one vendor `driver` can run, or `None` when it can run several.
pub fn driver_vendor(driver: &str) -> Option<&'static str> {
    DRIVER_VENDORS
        .iter()
        .find(|(slug, _)| *slug == driver)
        .map(|(_, vendor)| *vendor)
}

/// Install order for the packs themselves — the roster first, then the two
/// unseated roles, then any role a pack invents (alphabetically).
const ROLE_INSTALL_ORDER: [&str; 7] = [
    "lead",
    "architect",
    "builder",
    "verifier",
    "runner",
    "poker",
    "designer",
];

/// Reason reported for a child directory that is not a role pack. Rendered by
/// the dialog as `{path}: {reason}`.
pub const NO_ROLE_SKIP_REASON: &str = "no persona in this pack declares a role, so it was skipped.";

/// Prefix of every definition id the installer mints. Namespaced so a crew role
/// can never collide with a persona the user authored.
const CREW_ROLE_ID_PREFIX: &str = "crew-role:";

/// One persona, in one pack on this computer, that declares a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredRolePack {
    /// Absolute directory of the pack — what a seat stages from.
    pub dir: PathBuf,
    /// The persona's name inside the pack (`resolve_persona_by_name` key).
    pub persona_name: String,
    /// Human label for the minted agent.
    pub display_name: String,
    /// The persona's declared role. Never inferred.
    pub role: String,
    pub system_prompt: String,
    pub runtime: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub avatar_url: Option<String>,
}

/// A child of the chosen folder that produced no role, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedCrewRolePack {
    pub path: String,
    pub reason: String,
}

/// One installed role, as reported back to the dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledCrewRole {
    pub persona_id: String,
    pub persona_name: String,
    pub role: String,
    pub agent_pubkey: String,
    pub agent_name: String,
    pub pack_dir: String,
    /// `true` when an agent already installed from this pack was refreshed
    /// rather than minted.
    pub refreshed: bool,
    /// `true` when this run gave an already-installed identity a new name.
    ///
    /// Distinct from [`Self::refreshed`]: a refresh that changed nothing is
    /// not a rename, and the dialog owes an operator who renamed the designer
    /// a different sentence from the one it owes an operator who re-ran the
    /// installer over an untouched team.
    pub renamed: bool,
    /// `true` when this role is in the crew's default seat roster.
    pub seated: bool,
}

/// One row of the installer's "Name your team" list.
///
/// Ledger 84: the installer asked for one name — the lead's — so every other
/// identity was called after its role on this computer *and* on the relay. A
/// name is asked per pack the scan found, and the default is the name that
/// identity already carries here, so re-running the installer over a named
/// team offers `Keystone` rather than proposing to rename it back to `lead`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrewRoleNameChoice {
    /// The role this pack's persona declares. The map key of the install.
    pub role: String,
    /// The persona's name inside the pack.
    pub persona_name: String,
    /// Absolute directory of the pack, for a row that has to say where it
    /// came from.
    pub pack_dir: String,
    /// What the field starts on: the installed identity's current name, or
    /// the pack's own display name when nothing is installed from it yet.
    pub default_name: String,
    /// `true` when an identity is already installed from this pack, so a
    /// changed name here is a rename rather than a first naming.
    pub installed: bool,
}

/// What one scan of the chosen folder found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RolePackScan {
    pub packs: Vec<DiscoveredRolePack>,
    pub skipped: Vec<SkippedCrewRolePack>,
}

/// Key material for one freshly minted agent, supplied by the command layer.
///
/// The installer never generates keys itself — it asks for them — so there is
/// exactly one key-minting path in the desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedCrewIdentity {
    pub pubkey: String,
    pub private_key_nsec: String,
    pub auth_tag: Option<String>,
}

/// The whole result of an install: the stores to write and the rows to report.
#[derive(Debug, Clone)]
pub struct CrewRoleInstall {
    pub team: TeamRecord,
    /// Every definition, existing ones included — write this list wholesale.
    pub definitions: Vec<AgentDefinition>,
    /// Every agent, existing ones included — write this list wholesale.
    pub agents: Vec<ManagedAgentRecord>,
    pub installed: Vec<InstalledCrewRole>,
    /// The roles the crew that was written actually seats, in seat order.
    ///
    /// Read off the crew block, never off [`CREW_SEAT_ROSTER`]: a partial
    /// install seats fewer roles than the roster names, and a screen printing
    /// the roster would claim seats nothing holds.
    pub seated: Vec<String>,
    /// Roster roles with no installed pack, dropped from the crew's seats.
    ///
    /// Empty is the ordinary case. A non-empty list is a disclosure the
    /// installer owes: the operator asked for a crew and got a smaller one.
    pub dropped: Vec<String>,
}

/// Which stage of an install failed.
///
/// Carried instead of a prefixed sentence so the dialog can name the real
/// cause. Every failure used to reach the operator wrapped in "That folder
/// could not be read:", which sent someone with a locked keychain to look at
/// their folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrewRoleInstallFailure {
    /// The chosen folder is not a readable directory.
    Folder,
    /// The keychain was unavailable, or a new agent key could not be minted.
    Keys,
    /// A store — personas, managed agents, or teams — could not be saved.
    Store,
    /// The active community no longer matches the relay the caller pinned.
    Relay,
}

/// A failed install: the stage that failed, and the cause verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrewRoleInstallError {
    pub failure: CrewRoleInstallFailure,
    /// The underlying cause, with no sentence wrapped around it. The dialog
    /// supplies the sentence that matches `failure`.
    pub detail: String,
}

impl CrewRoleInstallError {
    /// The chosen folder could not be read.
    pub fn folder(detail: impl Into<String>) -> Self {
        Self {
            failure: CrewRoleInstallFailure::Folder,
            detail: detail.into(),
        }
    }

    /// A key could not be minted, or the keychain could not be reached.
    pub fn keys(detail: impl Into<String>) -> Self {
        Self {
            failure: CrewRoleInstallFailure::Keys,
            detail: detail.into(),
        }
    }

    /// A store could not be read or written.
    pub fn store(detail: impl Into<String>) -> Self {
        Self {
            failure: CrewRoleInstallFailure::Store,
            detail: detail.into(),
        }
    }

    /// The caller's relay scope changed before the install completed.
    pub fn relay(detail: impl Into<String>) -> Self {
        Self {
            failure: CrewRoleInstallFailure::Relay,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for CrewRoleInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

/// Sort key placing a role in install order: roster first, then poker and
/// designer, then anything a pack invents, alphabetically.
fn role_order(role: &str) -> (usize, String) {
    match ROLE_INSTALL_ORDER.iter().position(|known| *known == role) {
        Some(index) => (index, role.to_string()),
        None => (ROLE_INSTALL_ORDER.len(), role.to_string()),
    }
}

/// Whether a role is seated by default.
pub fn is_seated_role(role: &str) -> bool {
    CREW_SEAT_ROSTER.contains(&role)
}

/// The "Name your team" field list for a scan, in the order the dialog renders
/// it — the lead first, then the rest of the roster, then the unseated roles.
///
/// `agents` is this computer's current agent list: a pack that already has an
/// identity here defaults to *that identity's* name, never the pack's, so an
/// operator who leaves every field alone renames nobody.
pub fn role_name_choices(
    scan: &RolePackScan,
    agents: &[ManagedAgentRecord],
) -> Vec<CrewRoleNameChoice> {
    scan.packs
        .iter()
        .map(|pack| {
            let existing = existing_agent_for(agents, pack);
            CrewRoleNameChoice {
                role: pack.role.clone(),
                persona_name: pack.persona_name.clone(),
                pack_dir: pack.dir.display().to_string(),
                default_name: existing
                    .map(|record| record.name.clone())
                    .unwrap_or_else(|| pack.display_name.clone()),
                installed: existing.is_some(),
            }
        })
        .collect()
}

/// What one read-only look at a project's `personas/roles` folder found.
///
/// Three answers, kept apart on purpose: the folder is not there, the folder
/// is there and holds nothing, or the folder scans to packs. An empty `packs`
/// list alone cannot tell the first two apart, and the dialog has to say
/// which — "set a checkout" and "that checkout has no packs in it" send an
/// operator to different places.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRolePacksScan {
    /// The folder this scan looked in, named whether or not it exists.
    pub directory: String,
    /// `true` when that folder is there and is a directory.
    pub exists: bool,
    /// The same rows the picker path renders, from the same scan.
    pub packs: Vec<CrewRoleNameChoice>,
    /// Children that produced no role, and why.
    pub skipped: Vec<SkippedCrewRolePack>,
}

/// Where a project keeps its role packs: `personas/roles` under its checkout.
///
/// One function rather than a string joined at each call site, because the
/// dialog shows this path to the operator and the scan reads it — the two
/// must never be able to disagree about which folder was meant.
pub fn project_role_packs_dir(checkout: &Path) -> PathBuf {
    checkout.join("personas").join("roles")
}

/// Look at `<checkout>/personas/roles` without writing anything.
///
/// A folder that is not there is reported (`exists: false`), never returned as
/// an error: a project whose checkout has no role packs is an ordinary state
/// the dialog explains, not a failure to blame the operator for. `Err` is kept
/// for a folder that exists and still cannot be read.
///
/// `agents` is this computer's agent list, so a pack already installed here
/// defaults to that identity's own name — the same rule
/// [`role_name_choices`] applies on the picker path, because it *is* that
/// call.
pub fn scan_project_role_packs(
    checkout: &Path,
    agents: &[ManagedAgentRecord],
) -> Result<ProjectRolePacksScan, String> {
    let directory = project_role_packs_dir(checkout);
    let display = directory.display().to_string();
    if !directory.is_dir() {
        return Ok(ProjectRolePacksScan {
            directory: display,
            exists: false,
            packs: Vec::new(),
            skipped: Vec::new(),
        });
    }
    let scan = scan_role_packs(&directory)?;
    Ok(ProjectRolePacksScan {
        directory: display,
        exists: true,
        packs: role_name_choices(&scan, agents),
        skipped: scan.skipped,
    })
}

/// Scan the **immediate children** of `directory` for role packs.
///
/// No recursion, by decision: a nested pack belongs to whichever folder the
/// operator points at, and walking the tree would install packs they did not
/// choose. Every child directory that is not a role pack is reported in
/// [`RolePackScan::skipped`] rather than dropped silently. Non-directory
/// children cannot hold a `.plugin/plugin.json` and are not reported.
///
/// Returns `Err` only when the folder itself cannot be read — the caller turns
/// that into the "That folder could not be read" disclosure.
pub fn scan_role_packs(directory: &Path) -> Result<RolePackScan, String> {
    let entries =
        std::fs::read_dir(directory).map_err(|error| format!("{}", DisplayIoError(error)))?;

    let mut scan = RolePackScan::default();
    let mut children: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}", DisplayIoError(error)))?;
        children.push(entry.path());
    }
    children.sort();

    for child in children {
        if !child.is_dir() {
            continue;
        }
        let display = child.display().to_string();
        if !child.join(".plugin").join("plugin.json").is_file() {
            scan.skipped.push(SkippedCrewRolePack {
                path: display,
                reason: NO_ROLE_SKIP_REASON.to_string(),
            });
            continue;
        }
        let resolved = match buzz_persona_pkg::resolve::resolve_pack(&child) {
            Ok(resolved) => resolved,
            Err(error) => {
                // A pack that cannot be read is a different fact from a pack
                // with no role. Saying "no persona declares a role" here would
                // be a comfortable guess about a folder we never parsed.
                scan.skipped.push(SkippedCrewRolePack {
                    path: display,
                    reason: format!("this pack could not be read: {error}"),
                });
                continue;
            }
        };
        let dir = std::fs::canonicalize(&child).unwrap_or(child.clone());
        let mut found = false;
        for persona in resolved.personas {
            let Some(role) = persona.role.as_ref().map(|role| role.trim().to_string()) else {
                continue;
            };
            if role.is_empty() {
                continue;
            }
            found = true;
            scan.packs.push(DiscoveredRolePack {
                dir: dir.clone(),
                persona_name: persona.name.clone(),
                display_name: if persona.display_name.trim().is_empty() {
                    persona.name.clone()
                } else {
                    persona.display_name.clone()
                },
                role,
                system_prompt: persona.system_prompt.clone(),
                runtime: persona.runtime.clone(),
                model: persona.model.clone(),
                provider: persona.llm_provider.clone(),
                avatar_url: persona.avatar.clone(),
            });
        }
        if !found {
            scan.skipped.push(SkippedCrewRolePack {
                path: display,
                reason: NO_ROLE_SKIP_REASON.to_string(),
            });
        }
    }

    scan.packs.sort_by(|left, right| {
        role_order(&left.role)
            .cmp(&role_order(&right.role))
            .then_with(|| left.persona_name.cmp(&right.persona_name))
    });
    Ok(scan)
}

/// `std::io::Error` rendered without the "os error N" noise the dialog would
/// otherwise put in front of the operator.
struct DisplayIoError(std::io::Error);

impl std::fmt::Display for DisplayIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Build the crew block from the roles that actually got installed.
///
/// A roster role with no installed pack is **dropped** from the seats — never
/// seated with a persona id that does not exist, which would make every team
/// launch fail on a seat nobody fills. `primary` is the lead's persona, or the
/// first remaining seat when no lead was installed. `None` when the roster
/// produced no seats at all: that is an ordinary team, not a team with a hole.
///
/// Every seat states the runtime it will launch on and the vendor that runtime
/// can only be. Leaving both unset is what made the installed roster
/// unlaunchable: the family rule had nothing to read, so it refused every seat
/// as "vendor not declared" (SESSION_STATE item 77, F7).
pub fn build_crew(installed: &[(String, String)]) -> Option<TeamCrew> {
    let mut seats: Vec<TeamCrewSeat> = Vec::new();
    for role in CREW_SEAT_ROSTER {
        let Some((_, persona_id)) = installed.iter().find(|(candidate, _)| candidate == role)
        else {
            continue;
        };
        seats.push(TeamCrewSeat {
            persona_id: persona_id.clone(),
            role: role.to_string(),
            driver: Some(DEFAULT_CREW_SEAT_DRIVER.to_string()),
            model: None,
            vendor: driver_vendor(DEFAULT_CREW_SEAT_DRIVER).map(str::to_string),
        });
    }
    let primary = seats
        .iter()
        .find(|seat| seat.role == "lead")
        .or_else(|| seats.first())?
        .persona_id
        .clone();
    Some(TeamCrew { primary, seats })
}

/// The agent already installed from this exact pack persona, if any.
///
/// The idempotency key is the pack link the installer itself writes
/// (`persona_team_dir` + `persona_name_in_team`), not a display name a user may
/// have edited.
fn existing_agent_for<'a>(
    agents: &'a [ManagedAgentRecord],
    pack: &DiscoveredRolePack,
) -> Option<&'a ManagedAgentRecord> {
    agents.iter().find(|record| {
        record.persona_team_dir.as_deref() == Some(pack.dir.as_path())
            && record.persona_name_in_team.as_deref() == Some(pack.persona_name.as_str())
    })
}

/// Compute the exact drift basis an installed identity should carry for this
/// pack and its host-owned display name, without mutating either store.
pub(crate) fn role_pack_source_version(pack: &DiscoveredRolePack, display_name: &str) -> String {
    let definition = AgentDefinition {
        id: String::new(),
        display_name: display_name.to_string(),
        avatar_url: pack.avatar_url.clone(),
        system_prompt: pack.system_prompt.clone(),
        runtime: pack.runtime.clone(),
        model: pack.model.clone(),
        provider: pack.provider.clone(),
        name_pool: Vec::new(),
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        env_vars: Default::default(),
        respond_to: None,
        respond_to_allowlist: Vec::new(),
        parallelism: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    super::persona_events::persona_snapshot(&definition).source_version
}

/// A definition id that is free, derived from the persona name.
fn mint_definition_id(persona_name: &str, definitions: &[AgentDefinition]) -> String {
    let base = format!("{CREW_ROLE_ID_PREFIX}{persona_name}");
    if !definitions.iter().any(|def| def.id == base) {
        return base;
    }
    let mut suffix = 2usize;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !definitions.iter().any(|def| def.id == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

/// Keep a host-owned value across a role-pack reinstall.
///
/// `slot` starts out as whatever the pack projection produced. A non-blank
/// `previous` — the value the operator seated on this host — replaces it;
/// a blank or absent one leaves the pack's value standing so a pack that newly
/// declares something can still be picked up.
fn carry_over_host_value(slot: &mut Option<String>, previous: Option<&str>) {
    if let Some(value) = previous.map(str::trim).filter(|value| !value.is_empty()) {
        *slot = Some(value.to_string());
    }
}

/// Install every discovered role pack into the stores handed in.
///
/// Pure over the stores: it takes the current definitions, agents and teams and
/// returns the lists to write, so the whole policy (idempotent refresh, crew
/// composition, the `source_dir: None` rule) is testable without an
/// `AppHandle`. `mint` is called exactly once per **new** agent; a refresh
/// mints nothing.
///
/// # Errors
///
/// Propagates whatever `mint` returns as a [`CrewRoleInstallFailure::Keys`]
/// failure — a keyring or key-generation failure must abort the install rather
/// than write a keyless agent.
///
/// `names` maps a **role** to the name the operator gave that identity (D11: an
/// identity is something a person names once, not a role label). A role with no
/// entry — or an entry that is blank — keeps its pack's own display name. A
/// name that differs from what an already-installed identity carries renames
/// *that* identity in place, record and persona card together; it never mints a
/// second one, which is the whole of D11's "minted once" rule.
pub fn install_role_packs(
    scan: &RolePackScan,
    definitions: Vec<AgentDefinition>,
    agents: Vec<ManagedAgentRecord>,
    teams: &[TeamRecord],
    now: &str,
    names: &HashMap<String, String>,
    mint: &mut dyn FnMut() -> Result<MintedCrewIdentity, String>,
) -> Result<CrewRoleInstall, CrewRoleInstallError> {
    let existing_team = teams.iter().find(|team| {
        !team.is_builtin
            && (team.name == CREW_ROLES_TEAM_NAME || team.name == LEGACY_CREW_ROLES_TEAM_NAME)
    });
    let team_id = existing_team
        .map(|team| team.id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    install_role_packs_for_team(
        scan,
        definitions,
        agents,
        now,
        names,
        mint,
        team_id,
        CREW_ROLES_TEAM_NAME,
        // The folder picker installs into the one machine-wide team, which
        // belongs to no project. It takes no scope parameter on purpose: there
        // is no way for a caller to hand the machine-wide installer a project
        // namespace by accident.
        project::NameScope::Unscoped,
        existing_team,
    )
}

#[allow(clippy::too_many_arguments)]
fn install_role_packs_for_team(
    scan: &RolePackScan,
    mut definitions: Vec<AgentDefinition>,
    mut agents: Vec<ManagedAgentRecord>,
    now: &str,
    names: &HashMap<String, String>,
    mint: &mut dyn FnMut() -> Result<MintedCrewIdentity, String>,
    team_id: String,
    team_name: &str,
    // The namespace names minted by THIS install must be unique inside.
    // Passed in rather than read off the records being written: on a first
    // install `project_ref` is still `None` at mint time, because
    // `associate_installation` writes it after the installer returns.
    scope: NameScope,
    existing_team: Option<&TeamRecord>,
) -> Result<CrewRoleInstall, CrewRoleInstallError> {
    let mut installed: Vec<InstalledCrewRole> = Vec::new();
    let mut persona_ids: Vec<String> = Vec::new();
    let mut roles_to_personas: Vec<(String, String)> = Vec::new();
    // Identities minted by this run. Their `project_ref` is written by
    // `associate_installation` after the installer returns, so naming has to
    // be told they belong to `scope` rather than reading a field that is still
    // `None` — otherwise two roles of one project are named past each other.
    let mut minted_here: HashSet<String> = HashSet::new();

    for pack in &scan.packs {
        let existing = project::existing_agent_for_team(&agents, pack, &team_id).cloned();
        let persona_id = match existing
            .as_ref()
            .and_then(|record| record.persona_id.clone())
        {
            Some(id) if definitions.iter().any(|def| def.id == id) => id,
            _ => mint_definition_id(&pack.persona_name, &definitions),
        };

        // D11: every one of these is an identity the operator names. A role
        // the operator left alone keeps the name its pack declares.
        let wanted_name = names
            .get(&pack.role)
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .unwrap_or(pack.display_name.as_str());
        // An identity this install adopts keeps the namespace it already lives
        // in. The folder-picker install mints under `Unscoped`, but a record an
        // operator later associated with a project has to be named against
        // THAT project — otherwise this install could mint a second `Lead`
        // inside a project it never looked at.
        let pack_scope = existing
            .as_ref()
            .map(project::record_name_scope)
            .unwrap_or_else(|| scope.clone());
        let agent_name = match existing.as_ref() {
            // A refresh keeps the handle the operator already knows unless the
            // pack renamed the persona; either way the name stays unique inside
            // its namespace.
            Some(record) => project::mint_agent_name(
                wanted_name,
                &agents,
                &pack_scope,
                &minted_here,
                Some(&record.pubkey),
            ),
            None => project::mint_agent_name(wanted_name, &agents, &pack_scope, &minted_here, None),
        };
        // A rename is only a rename when an identity that already existed here
        // ends the run under a different name. A fresh mint is a naming.
        let renamed = existing
            .as_ref()
            .is_some_and(|record| record.name != agent_name);

        let definition = AgentDefinition {
            id: persona_id.clone(),
            // The card is the identity's card. Titling it after the pack left
            // the Agents grid reading "Lead" over an agent named Keystone —
            // a card pointing at an agent that does not exist (item 79a).
            display_name: agent_name.clone(),
            avatar_url: pack.avatar_url.clone(),
            system_prompt: pack.system_prompt.clone(),
            runtime: pack.runtime.clone(),
            model: pack.model.clone(),
            provider: pack.provider.clone(),
            name_pool: Vec::new(),
            is_builtin: false,
            is_active: true,
            shared: false,
            source_team: Some(team_id.clone()),
            source_team_persona_slug: Some(pack.persona_name.clone()),
            catalog_source: None,
            env_vars: Default::default(),
            respond_to: None,
            respond_to_allowlist: Vec::new(),
            parallelism: None,
            created_at: existing
                .as_ref()
                .map(|record| record.created_at.clone())
                .unwrap_or_else(|| now.to_string()),
            updated_at: now.to_string(),
        };
        match definitions.iter_mut().find(|def| def.id == persona_id) {
            Some(slot) => *slot = definition,
            None => definitions.push(definition),
        }

        let (pubkey, nsec, auth_tag, refreshed, created_at) = match existing.as_ref() {
            Some(record) => (
                record.pubkey.clone(),
                record.private_key_nsec.clone(),
                record.auth_tag.clone(),
                true,
                record.created_at.clone(),
            ),
            None => {
                let minted = mint().map_err(CrewRoleInstallError::keys)?;
                (
                    minted.pubkey,
                    minted.private_key_nsec,
                    minted.auth_tag,
                    false,
                    now.to_string(),
                )
            }
        };
        if !refreshed {
            minted_here.insert(pubkey.clone());
        }

        // Build the instance off the definition projection rather than a second
        // hand-written literal, then set the instance-side fields.
        let mut record = definitions
            .iter()
            .find(|def| def.id == persona_id)
            .cloned()
            .ok_or_else(|| {
                CrewRoleInstallError::store(format!(
                    "definition {persona_id} disappeared during install"
                ))
            })?
            .into_agent_record();
        record.pubkey = pubkey.clone();
        record.name = agent_name.clone();
        record.display_name = None;
        record.slug = None;
        record.persona_id = Some(persona_id.clone());
        record.team_id = Some(team_id.clone());
        record.private_key_nsec = nsec;
        record.auth_tag = auth_tag;
        record.relay_url = existing
            .as_ref()
            .map(|record| record.relay_url.clone())
            .unwrap_or_default();
        record.acp_command = crate::managed_agents::DEFAULT_ACP_COMMAND.to_string();
        record.home_role = Some(pack.role.clone());
        record.persona_team_dir = Some(pack.dir.clone());
        record.persona_name_in_team = Some(pack.persona_name.clone());
        record.created_at = created_at;
        record.updated_at = now.to_string();
        if let Some(previous) = existing.as_ref() {
            record.runtime_pid = previous.runtime_pid;
            record.backend = previous.backend.clone();
            record.backend_agent_id = previous.backend_agent_id.clone();
            record.env_vars = previous.env_vars.clone();
            record.last_started_at = previous.last_started_at.clone();
            record.last_stopped_at = previous.last_stopped_at.clone();
            // Model, provider, runtime and avatar are host-owned install facts
            // — what THIS computer runs the identity on, and what it looks like
            // — while the pack owns role, persona, skills and the display-name
            // default (D11–D13; ledger 77 "roles are team artifacts, installs
            // are per host"). The instance above is rebuilt off the pack
            // definition, and packs are model-agnostic by design, so without
            // this carry-over a refresh (or a rename-in-place) silently wipes
            // the seat the operator chose.
            //
            // Only a non-blank previous value carries over: a refresh must
            // still be able to pick up a value the pack newly declares for an
            // identity that never had one.
            carry_over_host_value(&mut record.model, previous.model.as_deref());
            carry_over_host_value(&mut record.provider, previous.provider.as_deref());
            carry_over_host_value(&mut record.runtime, previous.runtime.as_deref());
            carry_over_host_value(&mut record.avatar_url, previous.avatar_url.as_deref());
            record.agent_command_override = previous.agent_command_override.clone();
            // The project association is durable and never moved by a
            // reinstall; `associate_installation` must see it to refuse one.
            record.project_ref = previous.project_ref.clone();
        }
        // Re-derive the stored harness from the record we just assembled, so a
        // carried-over runtime or pin is not contradicted by a command line
        // computed from the pack alone.
        derive_harness_fields(&mut record, &definitions);

        match agents
            .iter_mut()
            .find(|candidate| candidate.pubkey == pubkey)
        {
            Some(slot) => *slot = record,
            None => agents.push(record),
        }

        persona_ids.push(persona_id.clone());
        roles_to_personas.push((pack.role.clone(), persona_id.clone()));
        installed.push(InstalledCrewRole {
            persona_id,
            persona_name: pack.persona_name.clone(),
            role: pack.role.clone(),
            agent_pubkey: pubkey,
            agent_name,
            pack_dir: pack.dir.display().to_string(),
            refreshed,
            renamed,
            seated: is_seated_role(&pack.role),
        });
    }

    let crew = build_crew(&roles_to_personas);
    // Both lists come off the crew that was actually built, so they cannot
    // drift from the seats the launch will read.
    let seated: Vec<String> = crew
        .as_ref()
        .map(|crew| crew.seats.iter().map(|seat| seat.role.clone()).collect())
        .unwrap_or_default();
    let dropped: Vec<String> = CREW_SEAT_ROSTER
        .iter()
        .filter(|role| !seated.iter().any(|seat| seat == *role))
        .map(|role| (*role).to_string())
        .collect();
    let team = TeamRecord {
        id: team_id,
        name: team_name.to_string(),
        description: existing_team.and_then(|team| team.description.clone()),
        instructions: existing_team.and_then(|team| team.instructions.clone()),
        persona_ids,
        crew,
        is_builtin: false,
        // NEVER a path. `delete_team_with_cascade` removes `source_dir`
        // recursively; a crew-roles team pointed at the operator's checkout
        // would delete it on "Delete team".
        source_dir: None,
        is_symlink: false,
        symlink_target: None,
        version: None,
        created_at: existing_team
            .map(|team| team.created_at.clone())
            .unwrap_or_else(|| now.to_string()),
        updated_at: now.to_string(),
    };

    Ok(CrewRoleInstall {
        team,
        definitions,
        agents,
        installed,
        seated,
        dropped,
    })
}

#[cfg(test)]
#[path = "crew_roles_tests.rs"]
mod tests;
