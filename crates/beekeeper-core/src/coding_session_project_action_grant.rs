//! NIP-CSAT: what a `grant-project-actions` link delegates, and the fold that
//! decides whether one is live.
//!
//! # The defect this exists for (ledger 186, finding 178(f))
//!
//! A team session was given the goal "build kettle with tests and a verify
//! action, land it on main" and could not finish it. Publishing a project
//! action (kind 30620) and starting a manual run of one (kind 46020) are
//! admitted for the channel owner or admin plus the project's creator, a
//! roster owner or an endorsed repository's founder
//! (`crates/beekeeper-relay/src/handlers/command_executor.rs`). A seat holds none
//! of those, and nothing in setup could give it one, so the lead did the only
//! honest thing available and asked a person for a ruling. The system had
//! accepted a routine goal it lacked the standing authority to finish.
//!
//! # What the delegation is, and is not
//!
//! One narrow, owner-signed, revocable capability, carried as a link in the
//! session's own authority chain
//! ([`crate::coding_session_authority_transition`]) rather than as a new kind:
//!
//! - **It is** permission for one pubkey to publish *this project's* kind:30620
//!   action definitions and to start manual kind:46020 runs of them.
//! - **It is not** permission to approve a host step (kind:46030). Approval is
//!   the separate, deliberate act of letting a command run on somebody's
//!   machine, and a seat that could both write the command and approve it
//!   would be no boundary at all.
//! - **It is not** permission over any other project, and it grants no
//!   steering, hiring, reading or membership authority of any kind.
//!
//! The scope is a capability, so a consumer asks
//! [`ProjectActionCapability::is_delegable`] rather than re-deriving the
//! boundary from a kind number at each call site.

use crate::coding_session_authority_transition::CodingSessionAuthorityTransitionType;

/// One thing a caller might try to do with a project's actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectActionCapability {
    /// Save or update a kind:30620 action definition bound to the project,
    /// including one whose steps run on a host.
    PublishDefinition,
    /// Start a manual kind:46020 run of one of the project's actions.
    TriggerManualRun,
    /// Release a host step's approval gate (kind:46030) so a command runs on
    /// an operator's machine.
    ApproveHostStep,
}

impl ProjectActionCapability {
    /// Whether a `grant-project-actions` link can confer this capability.
    ///
    /// `false` for [`Self::ApproveHostStep`] on purpose, and it is the whole
    /// reason this returns a decision instead of the caller writing `true`:
    /// the grant must never grow into an approval by someone reading the
    /// admission code and assuming "project actions" covers every event that
    /// mentions one.
    pub const fn is_delegable(self) -> bool {
        match self {
            Self::PublishDefinition | Self::TriggerManualRun => true,
            Self::ApproveHostStep => false,
        }
    }

    /// The wire token this capability is named by in a refusal or a report.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PublishDefinition => "publish-definition",
            Self::TriggerManualRun => "trigger-manual-run",
            Self::ApproveHostStep => "approve-host-step",
        }
    }
}

/// One link of an accepted authority chain, as much of it as this fold reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectActionGrantLink {
    /// Sequence number of the link in its chain.
    pub seq: u32,
    /// Event id (lowercase hex) of the accepted transition.
    pub accepted_event_id: String,
    /// The transition's type.
    pub transition_type: CodingSessionAuthorityTransitionType,
    /// Pubkey (lowercase hex) that signed the transition.
    pub signer_pubkey: String,
    /// Pubkey (lowercase hex) the transition targets.
    pub grantee_pubkey: String,
    /// The project coordinate the transition names, when it names one.
    pub project_ref: Option<String>,
}

/// A delegation that is live at the end of the folded chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectActionGrant {
    /// Who may publish and trigger this project's actions.
    pub grantee_pubkey: String,
    /// Which project's actions.
    pub project_ref: String,
    /// Who signed the delegation. A consumer re-checks this pubkey against the
    /// project's *current* owners, so a granter who has since lost ownership
    /// cannot leave a capability behind.
    pub granted_by: String,
    /// The accepted transition that granted it — the evidence a refusal or a
    /// disclosure cites.
    pub grant_event_id: String,
}

/// Fold one session's accepted chain into its live project-action grants.
///
/// `links` must arrive in the chain's accepted order; callers that read them
/// from storage sort by `seq` first. A grant is superseded by a later
/// `revoke-project-actions` naming the same `(granteePubkey, projectRef)`
/// pair, and by a later grant of the same pair (the newer link's signer is
/// then the granter, which matters because only the granter's *current*
/// ownership is checked).
///
/// Links of any other type are ignored: this fold is the project-action grant
/// set and nothing else, exactly as the seat projection is the seat set and
/// nothing else.
pub fn fold_project_action_grants<I>(links: I) -> Vec<ProjectActionGrant>
where
    I: IntoIterator<Item = ProjectActionGrantLink>,
{
    let mut live: Vec<ProjectActionGrant> = Vec::new();
    for link in links {
        let Some(project_ref) = link.project_ref.clone() else {
            continue;
        };
        let key = (link.grantee_pubkey.clone(), project_ref.clone());
        let existing = live
            .iter()
            .position(|grant| (grant.grantee_pubkey.clone(), grant.project_ref.clone()) == key);
        match link.transition_type {
            CodingSessionAuthorityTransitionType::GrantProjectActions => {
                let grant = ProjectActionGrant {
                    grantee_pubkey: link.grantee_pubkey,
                    project_ref,
                    granted_by: link.signer_pubkey,
                    grant_event_id: link.accepted_event_id,
                };
                match existing {
                    Some(index) => live[index] = grant,
                    None => live.push(grant),
                }
            }
            CodingSessionAuthorityTransitionType::RevokeProjectActions => {
                if let Some(index) = existing {
                    live.remove(index);
                }
            }
            CodingSessionAuthorityTransitionType::GrantOperator
            | CodingSessionAuthorityTransitionType::GrantViewer
            | CodingSessionAuthorityTransitionType::Revoke
            | CodingSessionAuthorityTransitionType::GrantSeat
            | CodingSessionAuthorityTransitionType::RevokeSeat
            | CodingSessionAuthorityTransitionType::Takeover
            | CodingSessionAuthorityTransitionType::Transfer => {}
        }
    }
    live
}

/// The live grant of `project_ref` held by `grantee_pubkey`, if any.
///
/// Comparison is on the exact strings the chain carries; callers lowercase
/// their pubkeys first, as every other consumer of this chain does.
pub fn find_project_action_grant<'a>(
    grants: &'a [ProjectActionGrant],
    grantee_pubkey: &str,
    project_ref: &str,
) -> Option<&'a ProjectActionGrant> {
    grants
        .iter()
        .find(|grant| grant.grantee_pubkey == grantee_pubkey && grant.project_ref == project_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(byte: &str) -> String {
        byte.repeat(32)
    }

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:kettle";

    fn link(
        seq: u32,
        transition_type: CodingSessionAuthorityTransitionType,
        signer: &str,
        grantee: &str,
        project_ref: Option<&str>,
    ) -> ProjectActionGrantLink {
        ProjectActionGrantLink {
            seq,
            accepted_event_id: hex("0a"),
            transition_type,
            signer_pubkey: signer.to_owned(),
            grantee_pubkey: grantee.to_owned(),
            project_ref: project_ref.map(str::to_owned),
        }
    }

    #[test]
    fn approval_is_never_delegable() {
        assert!(ProjectActionCapability::PublishDefinition.is_delegable());
        assert!(ProjectActionCapability::TriggerManualRun.is_delegable());
        assert!(!ProjectActionCapability::ApproveHostStep.is_delegable());
    }

    #[test]
    fn a_grant_is_live_until_its_exact_pair_is_revoked() {
        let owner = hex("11");
        let lead = hex("22");
        let other = hex("33");
        let grants = fold_project_action_grants(vec![
            link(
                1,
                CodingSessionAuthorityTransitionType::GrantProjectActions,
                &owner,
                &lead,
                Some(PROJECT),
            ),
            link(
                2,
                CodingSessionAuthorityTransitionType::GrantSeat,
                &owner,
                &lead,
                None,
            ),
            link(
                3,
                CodingSessionAuthorityTransitionType::RevokeProjectActions,
                &owner,
                &other,
                Some(PROJECT),
            ),
        ]);
        assert_eq!(grants.len(), 1);
        assert_eq!(
            find_project_action_grant(&grants, &lead, PROJECT)
                .map(|grant| grant.granted_by.clone()),
            Some(owner)
        );
        assert!(find_project_action_grant(&grants, &other, PROJECT).is_none());
    }

    #[test]
    fn a_revoke_ends_the_delegation() {
        let owner = hex("11");
        let lead = hex("22");
        let grants = fold_project_action_grants(vec![
            link(
                1,
                CodingSessionAuthorityTransitionType::GrantProjectActions,
                &owner,
                &lead,
                Some(PROJECT),
            ),
            link(
                2,
                CodingSessionAuthorityTransitionType::RevokeProjectActions,
                &owner,
                &lead,
                Some(PROJECT),
            ),
        ]);
        assert!(grants.is_empty());
    }

    #[test]
    fn a_grant_of_one_project_is_not_a_grant_of_another() {
        let owner = hex("11");
        let lead = hex("22");
        let elsewhere = PROJECT.replace("kettle", "elsewhere");
        let grants = fold_project_action_grants(vec![link(
            1,
            CodingSessionAuthorityTransitionType::GrantProjectActions,
            &owner,
            &lead,
            Some(PROJECT),
        )]);
        assert!(find_project_action_grant(&grants, &lead, &elsewhere).is_none());
    }

    #[test]
    fn a_later_grant_replaces_the_granter_of_record() {
        let first = hex("11");
        let second = hex("44");
        let lead = hex("22");
        let grants = fold_project_action_grants(vec![
            link(
                1,
                CodingSessionAuthorityTransitionType::GrantProjectActions,
                &first,
                &lead,
                Some(PROJECT),
            ),
            link(
                2,
                CodingSessionAuthorityTransitionType::GrantProjectActions,
                &second,
                &lead,
                Some(PROJECT),
            ),
        ]);
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].granted_by, second);
    }
}
