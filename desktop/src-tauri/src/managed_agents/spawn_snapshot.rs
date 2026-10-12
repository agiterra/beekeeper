//! Wire observation shape and current deployment team projection.
//! Historical desktop-process snapshot regressions are test fixtures only.
use super::types::{ManagedAgentRecord, TeamRecord};
#[cfg(test)]
use super::{AcpSessionPolicy, AgentDefinition, GlobalAgentConfig};
pub(crate) mod diff;
pub(crate) use diff::RestartDiffEntry;
#[cfg(test)]
mod fixture_projection;
#[cfg(test)]
pub(crate) use fixture_projection::*;
/// Resolve the current instructions for this instance's deployment-time team binding.
/// A deleted team deliberately degrades to no team section.
pub(crate) fn effective_team_instructions(
    record: &ManagedAgentRecord,
    teams: &[TeamRecord],
) -> Option<String> {
    teams
        .iter()
        .find(|team| Some(team.id.as_str()) == record.team_id.as_deref())
        .and_then(|team| team.instructions.as_deref())
        .map(str::trim)
        .filter(|instructions| !instructions.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests;
