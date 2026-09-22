//! What the role-pack installer owes the relay, and what it reports back.
//!
//! Split out of `crew_roles.rs` when that file reached the repository's
//! 1000-line ceiling. The seam is the boundary between *installing* — minting
//! identities, naming them, writing the team — and *telling everyone else*: the
//! kind:0 publishes a rename owes, the response the dialog renders, and the
//! crew projection a team snapshot carries.

use super::*;

/// One kind:0 profile publish the installer owes the relay.
///
/// Ledger 80 (e): the session header read "Fizz · Lead" over an identity this
/// installer had just named `Keystone`, because the seat's name is read from
/// the identity's relay profile and the installer never republished it. A name
/// that exists only in `managed-agents.json` is a name nobody else can see.
///
/// Carries no key material: the command layer holds the records and reads the
/// `nsec` from the one it is about to publish for. A struct that could be
/// logged must not be a struct that could log a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrewRoleProfilePublish {
    /// The identity whose profile is republished.
    pub pubkey: String,
    /// The name the profile must now carry — the identity's, not the pack's.
    pub display_name: String,
    /// The name this identity carried before the install, when it existed
    /// here at all. `None` means a freshly minted identity, which has no
    /// profile on the relay yet.
    pub previous_name: Option<String>,
}

/// Every kind:0 profile publish an install owes, one per installed role.
///
/// Deliberately unconditional rather than "only when the name changed": a
/// freshly minted identity has no profile at all, and a refreshed one may
/// carry a stale profile from an install whose publish failed. The relay's
/// kind:0 is replaceable, so republishing a name that is already correct
/// costs one event and removes a whole class of "the store and the relay
/// disagree" state. [`CrewRoleProfilePublish::previous_name`] is what lets a
/// caller describe a rename without making the publish conditional on one.
///
/// `previous` is the agent list as it was **before** the install.
pub fn role_profile_publishes(
    previous: &[ManagedAgentRecord],
    install: &CrewRoleInstall,
) -> Vec<CrewRoleProfilePublish> {
    install
        .installed
        .iter()
        .filter_map(|row| {
            let record = install
                .agents
                .iter()
                .find(|record| record.pubkey == row.agent_pubkey)?;
            Some(CrewRoleProfilePublish {
                pubkey: record.pubkey.clone(),
                display_name: record.name.clone(),
                previous_name: previous
                    .iter()
                    .find(|earlier| earlier.pubkey == record.pubkey)
                    .map(|earlier| earlier.name.clone()),
            })
        })
        .collect()
}

/// Response of the `install_crew_role_packs` command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCrewRolePacksResponse {
    pub team_id: String,
    pub team_name: String,
    pub installed: Vec<InstalledCrewRole>,
    pub skipped: Vec<SkippedCrewRolePack>,
    /// The roles this install actually seated, in seat order. The dialog
    /// renders this, never the roster constant.
    pub seated: Vec<String>,
    /// Roster roles whose pack was not installed, so they hold no seat.
    pub dropped: Vec<String>,
    /// What went wrong republishing the installed identities' relay profiles,
    /// or `None` when every one landed.
    ///
    /// The install itself already succeeded when this is set — the stores are
    /// written and the team exists. What is not true is that the relay knows
    /// these identities by the names this computer now uses, which is exactly
    /// the state ledger 80 (e) found: a session header reading the old name.
    pub profile_sync_error: Option<String>,
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
