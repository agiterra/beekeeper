//! Admission cases for the project-action delegation (ledger 186).
//!
//! These exercise the pure decision, which is where the rule lives; the async
//! wrapper adds only the two reads it names. Every case states the fact that
//! decides it, because a refusal that does not name its missing fact is the
//! refusal that made a lead ask a person (finding 178(f)).

use super::*;
use buzz_core::coding_session_project_action_grant::ProjectActionGrant;

const PROJECT: &str =
    "30621:1111111111111111111111111111111111111111111111111111111111111111:kettle";

fn hex(byte: &str) -> String {
    byte.repeat(32)
}

fn standing(seat_role: Option<&str>, granted_by: &str) -> ProjectActionGrantStanding {
    ProjectActionGrantStanding {
        grant: ProjectActionGrant {
            grantee_pubkey: hex("22"),
            project_ref: PROJECT.to_owned(),
            granted_by: granted_by.to_owned(),
            grant_event_id: hex("0a"),
        },
        genesis_ref: hex("0b"),
        seat_role: seat_role.map(str::to_owned),
    }
}

fn granters(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|key| (*key).to_owned()).collect()
}

#[test]
fn a_live_owner_signed_delegation_admits_the_lead() {
    let owner = hex("11");
    for capability in [
        ProjectActionCapability::PublishDefinition,
        ProjectActionCapability::TriggerManualRun,
    ] {
        let decision = decide_project_action_delegation(
            capability,
            &ProjectActionGrantLookup::Decided(vec![standing(Some("lead"), &owner)]),
            &granters(&[&owner]),
        );
        assert_eq!(
            decision,
            ProjectActionDelegation::Admitted {
                grant_event_id: hex("0a"),
                granted_by: owner.clone(),
            },
            "capability {}",
            capability.as_str()
        );
    }
}

#[test]
fn approving_a_host_step_is_never_delegated() {
    let owner = hex("11");
    let decision = decide_project_action_delegation(
        ProjectActionCapability::ApproveHostStep,
        &ProjectActionGrantLookup::Decided(vec![standing(Some("lead"), &owner)]),
        &granters(&[&owner]),
    );
    assert_eq!(
        decision,
        ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::CapabilityNotDelegable)
    );
}

#[test]
fn no_delegation_means_no_admission() {
    let decision = decide_project_action_delegation(
        ProjectActionCapability::PublishDefinition,
        &ProjectActionGrantLookup::Decided(Vec::new()),
        &granters(&[]),
    );
    assert_eq!(
        decision,
        ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::NoLiveGrant)
    );
}

#[test]
fn a_granter_who_no_longer_writes_the_project_leaves_no_capability() {
    let former_owner = hex("11");
    let decision = decide_project_action_delegation(
        ProjectActionCapability::PublishDefinition,
        &ProjectActionGrantLookup::Decided(vec![standing(Some("lead"), &former_owner)]),
        &granters(&[]),
    );
    assert_eq!(
        decision,
        ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::GranterNoLongerWrites)
    );
}

#[test]
fn a_delegation_held_by_a_non_lead_seat_does_not_apply() {
    let owner = hex("11");
    for seat in [None, Some("builder")] {
        let decision = decide_project_action_delegation(
            ProjectActionCapability::TriggerManualRun,
            &ProjectActionGrantLookup::Decided(vec![standing(seat, &owner)]),
            &granters(&[&owner]),
        );
        assert_eq!(
            decision,
            ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::HolderIsNotTheLead),
            "seat {seat:?}"
        );
    }
}

#[test]
fn a_chain_that_could_not_be_read_whole_decides_nothing() {
    let decision = decide_project_action_delegation(
        ProjectActionCapability::PublishDefinition,
        &ProjectActionGrantLookup::Undecidable,
        &granters(&[&hex("11")]),
    );
    assert_eq!(
        decision,
        ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::Undecidable)
    );
}

#[test]
fn every_refusal_names_the_missing_fact() {
    for refusal in [
        ProjectActionDelegationRefusal::CapabilityNotDelegable,
        ProjectActionDelegationRefusal::NoLiveGrant,
        ProjectActionDelegationRefusal::GranterNoLongerWrites,
        ProjectActionDelegationRefusal::HolderIsNotTheLead,
        ProjectActionDelegationRefusal::Undecidable,
    ] {
        let detail = refusal.detail();
        assert!(detail.len() > 30, "{refusal:?} says too little: {detail}");
        assert!(
            !detail.ends_with('.'),
            "{refusal:?} is a clause, not a sentence"
        );
    }
}
