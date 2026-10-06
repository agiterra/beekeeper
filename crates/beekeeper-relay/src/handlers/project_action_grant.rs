//! Admission for a delegated project action (NIP-CSAT, ledger 186).
//!
//! # The defect
//!
//! Publishing a project's kind:30620 action definition, and starting a manual
//! kind:46020 run of one, are admitted for the channel owner or admin plus the
//! project's creator, a roster owner or an endorsed repository's founder. A
//! team session's lead seat holds none of those. On 2026-09-20 a session was
//! given the goal "build kettle with tests and a verify action, land it on
//! main", wrote the action, and was refused its publication; the lead opened a
//! ruling on the founder (finding 178(f)). The system had accepted a routine
//! goal it lacked the standing authority to finish.
//!
//! # The rule this module adds
//!
//! One additional way in, and it is deliberately the narrowest one that
//! finishes that goal: the caller holds a **live `grant-project-actions`
//! delegation** for this exact project, in this channel's authority chain,
//! and
//!
//! 1. the capability asked for is one a delegation can carry at all
//!    ([`ProjectActionCapability::is_delegable`] — approving a host step,
//!    kind:46030, never is);
//! 2. the pubkey that signed the delegation is, **right now**, admitted to
//!    write this project ([`project_write_admitted`] — the same check the
//!    undelegated path uses), so a granter who has since lost ownership
//!    leaves no capability behind;
//! 3. the grantee still holds the session's active `lead` seat, because what
//!    the founder delegated is a lead's ability to carry out the project's
//!    work, not a permanent capability attached to an identity.
//!
//! Every pre-existing admission path is untouched: this is consulted only
//! after the standing checks have already refused.
//!
//! # Fail closed, and say which fact was missing
//!
//! Anything this module cannot establish — an unreadable roster, a chain too
//! long to read whole — returns [`ProjectActionDelegation::Refused`] with a
//! reason, never an admission and never a guess. The reason is what the
//! refusal message names, because "forbidden" without the missing fact is the
//! refusal that made a lead ask a person.

use std::collections::BTreeSet;
use std::sync::Arc;

use beekeeper_core::coding_session_project_action_grant::ProjectActionCapability;
use beekeeper_db::coding_session_project_action_grant::{
    ProjectActionGrantLookup, ProjectActionGrantStanding,
};
use uuid::Uuid;

use crate::handlers::pack_source::project_write_admitted;
use crate::state::AppState;

/// The role a delegation's holder must still occupy for it to apply.
pub(crate) const DELEGATED_SEAT_ROLE: &str = "lead";

/// Whether a delegation admits this act, and if not, which fact was missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectActionDelegation {
    /// A live, owner-signed delegation held by an active lead seat admits it.
    Admitted {
        /// The accepted kind:44228 transition that granted it.
        grant_event_id: String,
        /// Who signed it, re-confirmed as a current project writer.
        granted_by: String,
    },
    /// No delegation admits it. `reason` names the missing fact.
    Refused(ProjectActionDelegationRefusal),
}

/// Why no delegation admitted the act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectActionDelegationRefusal {
    /// The capability is outside what any delegation may carry (kind:46030).
    CapabilityNotDelegable,
    /// The caller holds no live delegation of this project here.
    NoLiveGrant,
    /// A delegation exists, but its signer is no longer a project writer.
    GranterNoLongerWrites,
    /// A delegation exists, but its holder no longer sits in the lead seat.
    HolderIsNotTheLead,
    /// The chain could not be read whole, so nothing is decided from it.
    Undecidable,
}

impl ProjectActionDelegationRefusal {
    /// The sentence a refusal adds after the standing rule's own words.
    ///
    /// Each one names the fact that is missing and what would supply it: a
    /// refusal a lead can act on is the whole point of the mechanism.
    pub(crate) fn detail(self) -> &'static str {
        match self {
            Self::CapabilityNotDelegable => {
                "a project-action delegation never covers approving a host step — approval is \
                 the separate act of letting a command run on somebody's machine"
            }
            Self::NoLiveGrant => {
                "no live project-action delegation for this project names this key; a project \
                 owner grants one by signing a grant-project-actions link into this session's \
                 authority chain"
            }
            Self::GranterNoLongerWrites => {
                "the delegation's signer is no longer one of this project's writers, so the \
                 delegation no longer stands; a current project owner must sign a new one"
            }
            Self::HolderIsNotTheLead => {
                "the delegation is held by a key that does not hold this session's active lead \
                 seat"
            }
            Self::Undecidable => {
                "this channel's authority chain could not be read whole, so no delegation was \
                 decided either way"
            }
        }
    }
}

/// Decide from already-read facts. Pure, so every arm is unit-testable.
///
/// `standings` are the live delegations of the requested project held by the
/// requested key, as folded from whole chains; `writing_granters` is the
/// subset of their signers that are admitted to write the project **now**.
pub(crate) fn decide_project_action_delegation(
    capability: ProjectActionCapability,
    lookup: &ProjectActionGrantLookup,
    writing_granters: &BTreeSet<String>,
) -> ProjectActionDelegation {
    if !capability.is_delegable() {
        return ProjectActionDelegation::Refused(
            ProjectActionDelegationRefusal::CapabilityNotDelegable,
        );
    }
    let standings: &[ProjectActionGrantStanding] = match lookup {
        ProjectActionGrantLookup::Undecidable => {
            return ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::Undecidable)
        }
        ProjectActionGrantLookup::Decided(standings) => standings,
    };
    if standings.is_empty() {
        return ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::NoLiveGrant);
    }
    let mut seen_lead = false;
    for standing in standings {
        if standing.seat_role.as_deref() != Some(DELEGATED_SEAT_ROLE) {
            continue;
        }
        seen_lead = true;
        if writing_granters.contains(&standing.grant.granted_by) {
            return ProjectActionDelegation::Admitted {
                grant_event_id: standing.grant.grant_event_id.clone(),
                granted_by: standing.grant.granted_by.clone(),
            };
        }
    }
    ProjectActionDelegation::Refused(if seen_lead {
        ProjectActionDelegationRefusal::GranterNoLongerWrites
    } else {
        ProjectActionDelegationRefusal::HolderIsNotTheLead
    })
}

/// Read the chain and the granters' current standing, then decide.
///
/// `channel_id` is the channel the action is filed in — the project's
/// coding-session channel, which is also where its team sessions' authority
/// chains live, so one channel-scoped read covers both.
pub(crate) async fn project_action_delegation_admitted(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    channel_id: Uuid,
    project: &str,
    caller: &[u8],
    capability: ProjectActionCapability,
) -> ProjectActionDelegation {
    if !capability.is_delegable() {
        return ProjectActionDelegation::Refused(
            ProjectActionDelegationRefusal::CapabilityNotDelegable,
        );
    }
    let lookup = match state
        .db
        .live_project_action_grants(community, channel_id, project, caller)
        .await
    {
        Ok(lookup) => lookup,
        Err(error) => {
            tracing::error!(error = %error, "project action delegation lookup failed");
            return ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::Undecidable);
        }
    };
    let mut writing_granters = BTreeSet::new();
    if let ProjectActionGrantLookup::Decided(standings) = &lookup {
        for standing in standings {
            if standing.seat_role.as_deref() != Some(DELEGATED_SEAT_ROLE)
                || writing_granters.contains(&standing.grant.granted_by)
            {
                continue;
            }
            match project_write_admitted(state, community, project, &standing.grant.granted_by)
                .await
            {
                Ok(Ok(_)) => {
                    writing_granters.insert(standing.grant.granted_by.clone());
                }
                Ok(Err(_)) => {}
                Err(()) => {
                    // The granter's standing could not be read. Deciding
                    // "granter no longer writes" from a failed read would
                    // revoke a live delegation over a database hiccup.
                    return ProjectActionDelegation::Refused(
                        ProjectActionDelegationRefusal::Undecidable,
                    );
                }
            }
        }
    }
    decide_project_action_delegation(capability, &lookup, &writing_granters)
}

#[cfg(test)]
#[path = "project_action_grant_tests.rs"]
mod tests;
