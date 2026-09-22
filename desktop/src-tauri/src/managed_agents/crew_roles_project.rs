//! Source-scoped project-team installer entrypoints.
//!
//! Kept beside the general crew installer so project identity boundaries do
//! not make the shared role-pack policy file exceed its size gate.

use super::*;

use std::collections::HashSet;

/// The project installer keys a durable team before looking at the local cache
/// path. Two projects may intentionally use the same repository and revision;
/// that must never cause one project's lead identity to be adopted by another.
pub(super) fn existing_agent_for_team<'a>(
    agents: &'a [ManagedAgentRecord],
    pack: &DiscoveredRolePack,
    team_id: &str,
) -> Option<&'a ManagedAgentRecord> {
    let exact = agents.iter().find(|record| {
        record.team_id.as_deref() == Some(team_id)
            && record.persona_team_dir.as_deref() == Some(pack.dir.as_path())
            && record.persona_name_in_team.as_deref() == Some(pack.persona_name.as_str())
    });
    if exact.is_some() {
        return exact;
    }
    // A project whose roles moved — from a packs repository into its own
    // agents repository — installs from a *different* directory than the
    // one its identities were minted under, so the exact key above matches
    // nothing and every role would be minted a second identity beside the
    // one the operator already knows. The team is already project-scoped,
    // so the role inside it identifies the same seat.
    //
    // Only when exactly one identity in this team fills the role: two
    // builders are an operator's deliberate arrangement, and silently
    // adopting one of them would be a guess.
    let mut by_role = agents.iter().filter(|record| {
        record.team_id.as_deref() == Some(team_id)
            && record.home_role.as_deref() == Some(pack.role.as_str())
    });
    match (by_role.next(), by_role.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// The namespace a minted agent name has to be unique inside.
///
/// This says nothing about who may hire the agent — only which other names
/// this one must differ from. `Unscoped` is the *no-project bucket*, never
/// "every project": a record naming no project reserves nothing outside that
/// bucket. The reserved every-project namespace is the explicit
/// [`ManagedAgentRecord::reserves_name_globally`] flag and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NameScope {
    /// Every agent associated with this normalized `30621:<owner>:<dtag>`.
    Project(String),
    /// Every agent on this computer that belongs to no project.
    Unscoped,
}

/// The namespace `record` already lives in, read from its durable association.
///
/// A blank or unparseable `project_ref` is `Unscoped` — never a third bucket,
/// so a malformed coordinate cannot open a private namespace of its own.
pub(crate) fn record_name_scope(record: &ManagedAgentRecord) -> NameScope {
    record
        .project_ref
        .as_deref()
        .and_then(crate::managed_agents::project_agent_association::normalize_project_ref)
        .map(NameScope::Project)
        .unwrap_or(NameScope::Unscoped)
}

/// Mint a name unique inside `scope`, never colliding with a name reserved in
/// the every-project namespace.
///
/// Until 2026-09-22 this scanned every managed agent on the computer, so the
/// second project to install a `builder` role got `Builder 2` and the third
/// `Builder 3` — a serial number recording nothing but the order in which
/// projects were created on one laptop (ledger 207, fixed by 246). A project
/// is the namespace now.
///
/// `agents` is the instance list. `load_managed_agents` already drops key-less
/// definitions (`storage.rs`), so a definition named `Lead` has never blocked
/// a mint and still does not — keeping that shape matters, because widening it
/// here would start suffixing names that were fine before.
///
/// `minted_here` carries the pubkeys this install run has already minted.
/// Their `project_ref` is written by `associate_installation` *after* the
/// installer returns, so without this they would read as `Unscoped` and two
/// roles of one project could be named past each other.
///
/// `keep` is the pubkey of an identity being renamed in place; its own current
/// name never blocks it.
///
/// Case is folded on both sides. Every other name resolver in this app folds
/// it — `workflow_sink::resolve_mention_pubkeys`, `appendUniqueName`,
/// `useAgentManagement` — so `builder` and `Builder` are one name, not two.
pub(super) fn mint_agent_name(
    display_name: &str,
    agents: &[ManagedAgentRecord],
    scope: &NameScope,
    minted_here: &HashSet<String>,
    keep: Option<&str>,
) -> String {
    let in_scope = |record: &ManagedAgentRecord| -> bool {
        // A record minted moments ago in this same run belongs to the scope
        // this run is installing into, whatever its not-yet-written field says.
        minted_here.contains(&record.pubkey) || record_name_scope(record) == *scope
    };
    let taken = |candidate: &str| {
        agents.iter().any(|record| {
            Some(record.pubkey.as_str()) != keep
                && record.name.trim().eq_ignore_ascii_case(candidate.trim())
                && (record.reserves_name_globally || in_scope(record))
        })
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
    scope: NameScope,
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
        scope,
        existing_team,
    )
}
