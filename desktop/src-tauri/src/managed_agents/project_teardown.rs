//! What a project owns on *this computer*, and what tearing it down removes.
//!
//! Creating a project mints one managed-agent identity per role, a team named
//! `Project team <coordinate>`, and one `crew-role:` definition per role
//! ([`super::default_agents::install_default_agents`]). Deleting the project
//! on the relay touched none of it until this module existed, so every
//! deleted project left its identities behind for good.
//!
//! Everything here is pure — no `AppHandle`, no I/O — so the ordering rules
//! below can be unit-tested directly. The execution lives in
//! [`super::project_teardown_execute`].
//!
//! # Two facts decide the whole ordering
//!
//! **1. The team cannot go first.** [`super::teams::delete_team_with_cascade`]
//! only cascades definitions inside `if team.source_dir.is_some()`, and a
//! project team's `source_dir` is `None` by deliberate construction (see the
//! note at `crew_roles.rs`, "NEVER a path"). Deleting the team first would
//! therefore leave every `crew-role:` definition alive carrying
//! `source_team: Some(<id of a team that no longer exists>)` — which
//! `validate_persona_deletion` refuses forever, while the team path answers
//! "team not found". The definitions would become permanently undeletable.
//!
//! **2. The set that blocks the team delete is wider than the set that
//! carries the project.** `agents_referencing_team` refuses while any agent
//! matches `team_id == team.id` *or* `persona_team_dir.file_name() ==
//! team_persona_key(team)`, while a project association is written by
//! `associate_installation` **after** the installer returns and can fail. So
//! an agent can hold the team and not the project, be invisible to a
//! `project_ref` enumeration, and block the team delete permanently.
//!
//! [`teardown_agents`] therefore enumerates over the **union** of all three
//! arms — the same predicate that refuses — and [`enumeration_clears_the_team`]
//! asserts that invariant directly rather than trusting the reading.

use std::collections::BTreeSet;

use super::types::{AgentDefinition, ManagedAgentRecord, TeamRecord};
use super::BackendKind;

/// One agent a teardown would delete.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeardownAgent {
    pub pubkey: String,
    pub name: String,
}

/// One `crew-role:` definition a teardown would remove.
///
/// Carries `d_tag` because the kind:30175 tombstone names a coordinate built
/// from it, and the definition is gone from the store by the time the
/// tombstone is enqueued — the same reason `delete_team_with_cascade`
/// captures its cascaded d-tags before removal.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeardownDefinition {
    pub id: String,
    pub display_name: String,
    pub d_tag: String,
}

/// The team a teardown would delete, named for the confirmation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeardownTeam {
    pub id: String,
    pub name: String,
}

/// Find the project's team by the name the installer gives it.
///
/// Built-in teams are never a project team; matching one would mean a rename
/// had collided with the installer's format, and deleting it is refused
/// downstream anyway — so it is excluded here, where the reason can be
/// stated, rather than surfacing as a confusing refusal later.
pub fn teardown_team<'a>(teams: &'a [TeamRecord], team_name: &str) -> Option<&'a TeamRecord> {
    teams
        .iter()
        .find(|team| !team.is_builtin && team.name == team_name)
}

/// Every agent the teardown must remove, as the union of the three arms.
///
/// `project_ref` is already normalized; `team` is absent when the project
/// never got one, in which case only the project arm applies.
///
/// Returns records in a stable order (by pubkey) so a plan is identical
/// across runs and can be compared against the set execute re-enumerates.
pub fn teardown_agents<'a>(
    agents: &'a [ManagedAgentRecord],
    project_ref: &str,
    team: Option<&TeamRecord>,
) -> Vec<&'a ManagedAgentRecord> {
    let team_key = team.map(super::team_repair::team_persona_key);
    let mut matched: Vec<&ManagedAgentRecord> = agents
        .iter()
        .filter(|record| {
            let by_project = record
                .project_ref
                .as_deref()
                .and_then(super::project_agent_association::normalize_project_ref)
                .is_some_and(|value| value == project_ref);
            let by_team =
                team.is_some_and(|team| record.team_id.as_deref() == Some(team.id.as_str()));
            let by_dir = team_key.is_some_and(|key| {
                record
                    .persona_team_dir
                    .as_ref()
                    .and_then(|dir| dir.file_name())
                    .and_then(|name| name.to_str())
                    == Some(key)
            });
            by_project || by_team || by_dir
        })
        .collect();
    matched.sort_by(|left, right| left.pubkey.cmp(&right.pubkey));
    matched
}

/// Whether removing `doomed` would leave nothing blocking the team delete.
///
/// This is `agents_referencing_team`'s predicate, restated so the invariant
/// can be asserted in a test instead of inferred: if this is ever false, the
/// teardown deletes agents and then refuses its own team delete, leaving the
/// definitions stranded exactly as the module doc describes.
pub fn enumeration_clears_the_team(
    agents: &[ManagedAgentRecord],
    doomed: &BTreeSet<String>,
    team: &TeamRecord,
) -> bool {
    let key = super::team_repair::team_persona_key(team);
    !agents
        .iter()
        .filter(|record| !doomed.contains(&record.pubkey))
        .any(|record| {
            record.team_id.as_deref() == Some(team.id.as_str())
                || record
                    .persona_team_dir
                    .as_ref()
                    .and_then(|dir| dir.file_name())
                    .and_then(|name| name.to_str())
                    == Some(key)
        })
}

/// Every definition the teardown must remove.
///
/// Two arms, and the second one matters more than it looks:
///
/// * definitions the team sourced (`source_team == team.id`), which is what
///   the installer writes; and
/// * definitions referenced by a doomed agent that **no surviving agent**
///   references, which reclaims rows already orphaned by an earlier attempt
///   that died between removing the agents and removing the definitions.
///
/// Built-ins are never included: a project has no business deleting Fizz.
pub fn teardown_definitions<'a>(
    definitions: &'a [AgentDefinition],
    agents: &[ManagedAgentRecord],
    doomed: &BTreeSet<String>,
    team: Option<&TeamRecord>,
) -> Vec<&'a AgentDefinition> {
    let doomed_persona_ids: BTreeSet<&str> = agents
        .iter()
        .filter(|record| doomed.contains(&record.pubkey))
        .filter_map(|record| record.persona_id.as_deref())
        .collect();
    let surviving_persona_ids: BTreeSet<&str> = agents
        .iter()
        .filter(|record| !doomed.contains(&record.pubkey))
        .filter_map(|record| record.persona_id.as_deref())
        .collect();
    let mut matched: Vec<&AgentDefinition> = definitions
        .iter()
        .filter(|definition| !definition.is_builtin)
        .filter(|definition| {
            let by_team = team
                .is_some_and(|team| definition.source_team.as_deref() == Some(team.id.as_str()));
            let by_orphan = doomed_persona_ids.contains(definition.id.as_str())
                && !surviving_persona_ids.contains(definition.id.as_str());
            by_team || by_orphan
        })
        .collect();
    matched.sort_by(|left, right| left.id.cmp(&right.id));
    matched
}

/// Doomed agents that are provider-deployed, which refuse the whole teardown.
///
/// Deleting the local record of a live remote deployment orphans it — the
/// invariant `delete_managed_agent` enforces with `force_remote_delete` and
/// `delete_persona` enforces with its own pre-flight. A multi-agent teardown
/// has no honest way to ask per agent, so it refuses and names them.
pub fn remote_deployed_agents<'a>(
    agents: &'a [ManagedAgentRecord],
    doomed: &BTreeSet<String>,
) -> Vec<&'a ManagedAgentRecord> {
    agents
        .iter()
        .filter(|record| {
            doomed.contains(&record.pubkey)
                && record.backend != BackendKind::Local
                && record.backend_agent_id.is_some()
        })
        .collect()
}

#[cfg(test)]
#[path = "project_teardown_tests.rs"]
mod tests;
