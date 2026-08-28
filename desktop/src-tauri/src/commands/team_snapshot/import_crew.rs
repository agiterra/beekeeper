//! Binding an imported snapshot's crew to the ids the import minted.
//!
//! Lifted out of `team_snapshot.rs` so the file stays inside the repo's
//! size ratchet; the logic is unchanged.

use super::TeamSnapshot;

/// The disclosure shown when a snapshot's crew cannot be bound to its members.
pub(crate) const CREW_UNMATCHED_NOTE: &str =
    "This snapshot's crew could not be matched to its members, so it was imported as an ordinary team.";

/// Bind the snapshot's crew, if it has one, to the ids this import minted.
///
/// `None` covers two different facts — no crew, and a crew whose seats do not
/// all name a member — and the caller distinguishes them so the second is
/// disclosed rather than silently swallowed.
pub(crate) fn remap_import_crew(
    snapshot: &TeamSnapshot,
    persona_ids: &[String],
) -> Option<crate::managed_agents::TeamCrew> {
    let crew = snapshot.team.crew.as_ref()?;
    let member_names: Vec<String> = snapshot
        .members
        .iter()
        .map(|member| member.definition.name.clone())
        .collect();
    crate::managed_agents::team_snapshot::remap_snapshot_crew(crew, &member_names, persona_ids)
}

/// `true` when the snapshot declares a crew this import could not bind.
pub(crate) fn import_crew_is_unmatched(snapshot: &TeamSnapshot, persona_ids: &[String]) -> bool {
    snapshot.team.crew.is_some() && remap_import_crew(snapshot, persona_ids).is_none()
}
