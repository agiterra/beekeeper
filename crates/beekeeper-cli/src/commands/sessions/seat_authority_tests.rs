use beekeeper_core::coding_session_team_transaction::{
    CodingSessionTeamActiveGrant, CodingSessionTeamActiveSeat,
};

use super::operations::ProjectedAuthority;
use super::seat_authority::{decide_for_test, should_retry_for_test};
use crate::error::CliError;

const FOUNDER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OPERATOR: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ACTOR: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn authority() -> ProjectedAuthority {
    ProjectedAuthority {
        grants: vec![CodingSessionTeamActiveGrant {
            actor_pubkey: OPERATOR.into(),
            grant_event_ref: "11".repeat(32),
            may_steer: true,
        }],
        seats: vec![CodingSessionTeamActiveSeat {
            actor_pubkey: LEAD.into(),
            role: "lead".into(),
        }],
        seat_grant_refs: [(LEAD.into(), "22".repeat(32))].into_iter().collect(),
        policy_grants: Vec::new(),
        head_event_id: Some("22".repeat(32)),
        head_seq: 2,
        claim: beekeeper_core::coding_session_authority_claim::ClaimState::NoClaim,
        claim_since: None,
        grant_accepted_at: std::collections::BTreeMap::new(),
        seat_accepted_at: std::collections::BTreeMap::new(),
    }
}

#[test]
fn founder_operator_and_lead_can_append_per_authority_matrix() {
    for (signer, role) in [(FOUNDER, "lead"), (OPERATOR, "lead"), (LEAD, "builder")] {
        assert_eq!(
            decide_for_test(&authority(), FOUNDER, signer, ACTOR, role).expect("authorized signer"),
            (None, true)
        );
    }
}

#[test]
fn exact_active_seat_is_idempotent_and_role_conflict_is_refused() {
    let exact =
        decide_for_test(&authority(), FOUNDER, FOUNDER, LEAD, "lead").expect("existing exact seat");
    assert_eq!(exact, (Some("22".repeat(32)), false));

    let error = decide_for_test(&authority(), FOUNDER, FOUNDER, LEAD, "verifier")
        .expect_err("role change requires revoke");
    assert!(error.to_string().contains("already holds role lead"));
}

#[test]
fn unauthorized_signer_and_self_nomination_are_refused() {
    let stranger = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    assert!(decide_for_test(&authority(), FOUNDER, stranger, ACTOR, "builder").is_err());
    assert!(decide_for_test(&authority(), FOUNDER, ACTOR, ACTOR, "builder").is_err());
}

#[test]
fn lead_cannot_escalate_an_actor_to_lead() {
    let error = decide_for_test(&authority(), FOUNDER, LEAD, ACTOR, "lead")
        .expect_err("lead escalation must fail");
    assert!(error.to_string().contains("cannot grant lead authority"));
}

#[test]
fn head_race_refetches_once_and_never_loops() {
    let stale = CliError::Other(
        "relay rejected event: invalid: prevAccepted does not match the chain's current head"
            .into(),
    );
    assert!(should_retry_for_test(0, &stale));
    assert!(!should_retry_for_test(1, &stale));
    assert!(!should_retry_for_test(
        0,
        &CliError::Other("relay rejected event: invalid signer".into())
    ));
}
