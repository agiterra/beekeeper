//! Admission cases for the project-action delegation (ledger 186).
//!
//! These exercise the pure decision, which is where the rule lives; the async
//! wrapper adds only the two reads it names. Every case states the fact that
//! decides it, because a refusal that does not name its missing fact is the
//! refusal that made a lead ask a person (finding 178(f)).

use super::*;
use beekeeper_core::coding_session_project_action_grant::ProjectActionGrant;

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
            RoleVocabulary::foundation(),
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
        RoleVocabulary::foundation(),
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
        RoleVocabulary::foundation(),
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
        RoleVocabulary::foundation(),
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
            RoleVocabulary::foundation(),
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
        RoleVocabulary::foundation(),
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

/// The seat a delegation's holder must occupy is the project's own lead
/// role: under `lead: captain` a `captain` seat is admitted and the
/// foundation's `lead` is not. The default vocabulary is unchanged.
#[test]
fn the_delegated_seat_is_the_projects_lead_role() {
    let owner = hex("11");
    let captain =
        RoleVocabulary::from_team(Some("captain"), Some(&["qa".to_owned()])).expect("a vocabulary");
    assert_eq!(
        decide_project_action_delegation(
            ProjectActionCapability::PublishDefinition,
            &ProjectActionGrantLookup::Decided(vec![standing(Some("captain"), &owner)]),
            &granters(&[&owner]),
            &captain,
        ),
        ProjectActionDelegation::Admitted {
            grant_event_id: hex("0a"),
            granted_by: owner.clone(),
        }
    );
    assert_eq!(
        decide_project_action_delegation(
            ProjectActionCapability::PublishDefinition,
            &ProjectActionGrantLookup::Decided(vec![standing(Some("lead"), &owner)]),
            &granters(&[&owner]),
            &captain,
        ),
        ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::HolderIsNotTheLead),
        "the foundation's name is not this project's lead"
    );
    assert_eq!(
        decide_project_action_delegation(
            ProjectActionCapability::PublishDefinition,
            &ProjectActionGrantLookup::Decided(vec![standing(Some("captain"), &owner)]),
            &granters(&[&owner]),
            RoleVocabulary::foundation(),
        ),
        ProjectActionDelegation::Refused(ProjectActionDelegationRefusal::HolderIsNotTheLead),
        "under the default vocabulary a captain seat holds nothing"
    );
}

/// Bee issue 1f0d7b3e, through the real reads: run admission asks the save
/// rule again, so a roster owner removed after saving no longer writes the
/// project, while the creator always does and a stranger never did.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn owner_writes_project_follows_the_current_roster() {
    use beekeeper_core::channel::ProjectRole;
    use beekeeper_core::kind::KIND_PROJECT;
    use beekeeper_core::tenant::TenantContext;
    use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

    use crate::handlers::ingest::{ingest_event, HttpAuthMethod, IngestAuth};

    let state = crate::api::git::policy::tests::policy_test_state().await;
    let host = format!("owner-writes-{}.example", Uuid::new_v4().simple());
    let community = state
        .db
        .ensure_configured_community(&host)
        .await
        .expect("community")
        .id;
    let tenant = TenantContext::resolved(community, host);
    let creator = Keys::generate();
    let owner = Keys::generate();
    let stranger = Keys::generate();
    let project = format!("30621:{}:admission", creator.public_key().to_hex());
    let head = EventBuilder::new(Kind::Custom(KIND_PROJECT as u16), r#"{"name":"admission"}"#)
        .tags([Tag::parse(["d", "admission"]).expect("d")])
        .sign_with_keys(&creator)
        .expect("project signature");
    let creator_auth = IngestAuth::Http {
        pubkey: creator.public_key(),
        scopes: vec![beekeeper_auth::Scope::ReposWrite],
        auth_method: HttpAuthMethod::Nip98,
    };
    ingest_event(&state, &tenant, head, creator_auth)
        .await
        .expect("project ingest");
    state
        .db
        .upsert_project_acl(
            community,
            creator.public_key().as_bytes(),
            "admission",
            "public",
            &[(owner.public_key().to_bytes().to_vec(), ProjectRole::Owner)],
            Timestamp::now().as_secs() as i64,
        )
        .await
        .expect("roster");
    let channel = Uuid::new_v4();
    let writes = |keys: &Keys| {
        let state = state.clone();
        let project = project.clone();
        let key = keys.public_key().to_bytes().to_vec();
        async move { owner_writes_project(&state, community, channel, &project, &key).await }
    };

    assert_eq!(writes(&creator).await, Ok(true), "the creator writes");
    assert_eq!(writes(&owner).await, Ok(true), "a roster owner writes");
    assert_eq!(writes(&stranger).await, Ok(false), "a stranger never did");

    state
        .db
        .remove_project_members(
            community,
            creator.public_key().as_bytes(),
            "admission",
            creator.public_key().as_bytes(),
            &[owner.public_key().to_bytes().to_vec()],
        )
        .await
        .expect("remove owner");
    assert_eq!(
        writes(&owner).await,
        Ok(false),
        "removed after saving: refused at admission"
    );
    assert_eq!(writes(&creator).await, Ok(true), "the creator still writes");
}
