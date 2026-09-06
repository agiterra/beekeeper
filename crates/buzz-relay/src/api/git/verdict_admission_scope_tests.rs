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

/// The refusal sentence names the seat lookup, the key it ran for, and — since
/// finding 91 — what the seats were narrowed to.
#[test]
fn the_seat_lookup_names_itself_and_its_narrowing_in_the_refusal() {
    let source = VerdictAdmissionCandidateSource::SeatOfMissionInScope {
        seat: "0123abcd".to_owned(),
        seats: 2,
        held: 3,
        within: "the 4 session channel(s) of 30621:aa:beekeeper".to_owned(),
    };
    let clause = source.searched_clause(2);
    assert!(
        clause.contains("the 2 of the 3 seat(s) held by 0123abcd"),
        "{clause}"
    );
    assert!(
        clause.contains("the 4 session channel(s) of 30621:aa:beekeeper"),
        "a narrowed count must say what it was narrowed to: {clause}"
    );
    assert!(clause.starts_with("Searched 2 mission(s)"), "{clause}");
}

/// A key seated only on other repositories' missions is not told it holds no
/// seat — it is told which of its seats this repository grants, which is none.
#[test]
fn a_key_seated_elsewhere_is_not_told_it_holds_no_seat() {
    let source = VerdictAdmissionCandidateSource::SeatOfMissionInScope {
        seat: "0123abcd".to_owned(),
        seats: 0,
        held: 2,
        within: "the channel this repository binds (c-1)".to_owned(),
    };
    let clause = source.searched_clause(1);
    assert!(clause.contains("holds 2 seat(s)"), "{clause}");
    assert!(
        clause.contains("none of them is on a mission of this repository's own channels"),
        "{clause}"
    );
    assert!(
        !clause.contains("holds no seat in the newest"),
        "the fall-back sentence would be false for a key that is seated elsewhere: {clause}"
    );
}

/// A repository that grants no channel at all has nowhere to look, whatever
/// seats the pusher holds elsewhere (finding 91).
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_repository_that_grants_no_channel_has_nowhere_to_look() {
    let state = policy_test_state().await;
    let community = state
        .db
        .ensure_configured_community(&format!("scope-{}.example", Uuid::new_v4().simple()))
        .await
        .expect("community")
        .id;
    let scope = resolve_candidate_scope(
        &state,
        community,
        None,
        None,
        &Keys::generate().public_key().to_hex(),
    )
    .await
    .expect("scope");
    assert!(
        scope.is_none(),
        "no project and no binding is nowhere to look, and never the community"
    );
}
