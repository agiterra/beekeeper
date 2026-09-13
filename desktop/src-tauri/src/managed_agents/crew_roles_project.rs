//! Source-scoped project-team installer entrypoints.
//!
//! Kept beside the general crew installer so project identity boundaries do
//! not make the shared role-pack policy file exceed its size gate.

use super::*;

/// The project installer keys a durable team before looking at the local cache
/// path. Two projects may intentionally use the same repository and revision;
/// that must never cause one project's lead identity to be adopted by another.
pub(super) fn existing_agent_for_team<'a>(
    agents: &'a [ManagedAgentRecord],
    pack: &DiscoveredRolePack,
    team_id: &str,
) -> Option<&'a ManagedAgentRecord> {
    agents.iter().find(|record| {
        record.team_id.as_deref() == Some(team_id)
            && record.persona_team_dir.as_deref() == Some(pack.dir.as_path())
            && record.persona_name_in_team.as_deref() == Some(pack.persona_name.as_str())
    })
}

pub(super) fn mint_agent_name(
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

/// Install a source-scoped project team. The caller persists `team_id` with
/// its adopted source before retrying, making identity minting idempotent per
/// project rather than per cache directory.
#[allow(clippy::too_many_arguments)]
pub(crate) fn install_role_packs_in_named_team(
    scan: &RolePackScan,
    definitions: Vec<AgentDefinition>,
    agents: Vec<ManagedAgentRecord>,
    teams: &[TeamRecord],
    now: &str,
    names: &HashMap<String, String>,
    mint: &mut dyn FnMut() -> Result<MintedCrewIdentity, String>,
    team_id: String,
    team_name: &str,
) -> Result<CrewRoleInstall, CrewRoleInstallError> {
    let existing_team = teams.iter().find(|team| team.id == team_id);
    install_role_packs_for_team(
        scan,
        definitions,
        agents,
        now,
        names,
        mint,
        team_id,
        team_name,
        existing_team,
    )
}
