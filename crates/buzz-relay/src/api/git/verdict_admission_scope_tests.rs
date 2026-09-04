//! The seat lookup, against Postgres, and the sentence it produces.

use super::*;

use nostr::Keys;

use crate::api::git::policy::tests::policy_test_state;

/// A key that was never seated anywhere resolves no missions, so the search
/// falls through to the next lookup rather than reading a channel it has no
/// business in.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_unseated_key_resolves_no_mission() {
    let state = policy_test_state().await;
    let community = state
        .db
        .ensure_configured_community(&format!("scope-{}.example", Uuid::new_v4().simple()))
        .await
        .expect("community")
        .id;
    let stranger = Keys::generate();
    let scope = resolve_candidate_scope(
        &state,
        community,
        None,
        None,
        &stranger.public_key().to_hex(),
    )
    .await
    .expect("scope");
    assert!(
        scope.is_none(),
        "no seat, no project and no binding is nowhere to look"
    );
}

/// The bound channel is still the last fallback, and it says so.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn the_bound_channel_is_the_last_fallback() {
    let state = policy_test_state().await;
    let community = state
        .db
        .ensure_configured_community(&format!("scope-{}.example", Uuid::new_v4().simple()))
        .await
        .expect("community")
        .id;
    let bound = Uuid::new_v4();
    let scope = resolve_candidate_scope(
        &state,
        community,
        Some(bound),
        None,
        &Keys::generate().public_key().to_hex(),
    )
    .await
    .expect("scope")
    .expect("a bound repository has somewhere to look");
    assert_eq!(scope.channels, vec![bound]);
    assert!(scope.genesis_ids.is_empty());
    assert_eq!(
        scope.source,
        VerdictAdmissionCandidateSource::BoundChannel,
        "with no seat and no project the search is where it always was"
    );
}

/// The refusal sentence names the seat lookup and the key it ran for.
#[test]
fn the_seat_lookup_names_itself_in_the_refusal() {
    let source = VerdictAdmissionCandidateSource::SeatOfMission {
        seat: "0123abcd".to_owned(),
        seats: 2,
    };
    let clause = source.searched_clause(2);
    assert!(clause.contains("that seat 0123abcd"), "{clause}");
    assert!(clause.starts_with("Searched 2 mission(s)"), "{clause}");
}
