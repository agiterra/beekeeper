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

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::managed_agents::{
    team_events::{TeamCrew, TeamCrewSeat},
    types::{AgentDefinition, ManagedAgentRecord},
    TeamRecord,
};

/// Name of the one team every installed role pack joins. Also the dedupe key:
/// a second run updates this team rather than minting `Crew roles (2)`.
pub const CREW_ROLES_TEAM_NAME: &str = "Crew roles";

/// Roles seated by default, in launch order.
///
/// `poker` and `designer` packs are installed as agents but left unseated: a
/// poker drives the built app (screenshots, e2e), which a seated coding-session
/// execution cannot do, and a designer works at brief time, before a crew
/// exists.
pub const CREW_SEAT_ROSTER: [&str; 5] = ["lead", "architect", "builder", "verifier", "runner"];

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
    /// `true` when this role is in the crew's default seat roster.
    pub seated: bool,
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
/// seated with a persona id that does not exist, which would make every crew
/// launch fail on a seat nobody fills. `primary` is the lead's persona, or the
/// first remaining seat when no lead was installed. `None` when the roster
/// produced no seats at all: that is an ordinary team, not a crew with a hole.
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
            driver: None,
            model: None,
            vendor: None,
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

/// An agent handle that is free among every agent except `keep`.
fn mint_agent_name(
    display_name: &str,
    agents: &[ManagedAgentRecord],
    keep: Option<&str>,
) -> String {
    let taken = |candidate: &str| {
        agents
            .iter()
            .any(|record| record.name == candidate && Some(record.pubkey.as_str()) != keep)
    };
    if !taken(display_name) {
        return display_name.to_string();
    }
    let mut suffix = 2usize;
    loop {
        let candidate = format!("{display_name} {suffix}");
        if !taken(&candidate) {
            return candidate;
        }
        suffix += 1;
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
/// Propagates whatever `mint` returns as an error — a keyring or key-generation
/// failure must abort the install rather than write a keyless agent.
pub fn install_role_packs(
    scan: &RolePackScan,
    mut definitions: Vec<AgentDefinition>,
    mut agents: Vec<ManagedAgentRecord>,
    teams: &[TeamRecord],
    now: &str,
    mint: &mut dyn FnMut() -> Result<MintedCrewIdentity, String>,
) -> Result<CrewRoleInstall, String> {
    let existing_team = teams
        .iter()
        .find(|team| !team.is_builtin && team.name == CREW_ROLES_TEAM_NAME);
    let team_id = existing_team
        .map(|team| team.id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut installed: Vec<InstalledCrewRole> = Vec::new();
    let mut persona_ids: Vec<String> = Vec::new();
    let mut roles_to_personas: Vec<(String, String)> = Vec::new();

    for pack in &scan.packs {
        let existing = existing_agent_for(&agents, pack).cloned();
        let persona_id = match existing
            .as_ref()
            .and_then(|record| record.persona_id.clone())
        {
            Some(id) if definitions.iter().any(|def| def.id == id) => id,
            _ => mint_definition_id(&pack.persona_name, &definitions),
        };

        let definition = AgentDefinition {
            id: persona_id.clone(),
            display_name: pack.display_name.clone(),
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

        let agent_command =
            crate::managed_agents::effective_agent_command(Some(&persona_id), &definitions, None);
        let agent_args = crate::managed_agents::normalize_agent_args(&agent_command, Vec::new());
        let mcp_command = crate::managed_agents::known_acp_runtime(&agent_command)
            .and_then(|runtime| runtime.mcp_command)
            .unwrap_or("")
            .to_string();

        let (pubkey, nsec, auth_tag, refreshed, created_at) = match existing.as_ref() {
            Some(record) => (
                record.pubkey.clone(),
                record.private_key_nsec.clone(),
                record.auth_tag.clone(),
                true,
                record.created_at.clone(),
            ),
            None => {
                let minted = mint()?;
                (
                    minted.pubkey,
                    minted.private_key_nsec,
                    minted.auth_tag,
                    false,
                    now.to_string(),
                )
            }
        };

        let agent_name = match existing.as_ref() {
            // A refresh keeps the handle the operator already knows unless the
            // pack renamed the persona; either way the name stays unique.
            Some(record) => mint_agent_name(&pack.display_name, &agents, Some(&record.pubkey)),
            None => mint_agent_name(&pack.display_name, &agents, None),
        };

        // Build the instance off the definition projection rather than a second
        // hand-written literal, then set the instance-side fields.
        let mut record = definitions
            .iter()
            .find(|def| def.id == persona_id)
            .cloned()
            .ok_or_else(|| format!("definition {persona_id} disappeared during install"))?
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
        record.agent_command = agent_command;
        record.agent_args = agent_args;
        record.mcp_command = mcp_command;
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
        }

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
            seated: is_seated_role(&pack.role),
        });
    }

    let crew = build_crew(&roles_to_personas);
    let team = TeamRecord {
        id: team_id,
        name: CREW_ROLES_TEAM_NAME.to_string(),
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
    })
}

/// Response of the `install_crew_role_packs` command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCrewRolePacksResponse {
    pub team_id: String,
    pub team_name: String,
    pub installed: Vec<InstalledCrewRole>,
    pub skipped: Vec<SkippedCrewRolePack>,
}

/// Crew composition carried by a team snapshot, keyed by **member name**.
///
/// Never by `personaId`: import mints fresh definition ids, so an id from
/// another computer names nothing here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TeamSnapshotCrew {
    /// `AgentSnapshotDefinition.name` of the seat that takes the first turn.
    pub primary_member_name: String,
    pub seats: Vec<TeamSnapshotCrewSeat>,
}

/// One seat of a snapshot's crew.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TeamSnapshotCrewSeat {
    pub member_name: String,
    pub role: String,
}

#[cfg(test)]
#[path = "crew_roles_tests.rs"]
mod tests;
