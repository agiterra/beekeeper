//! Signed read prerequisites for the live dequeue custody regression.
use super::*;
use beekeeper_core::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamTransactionBody,
};
use beekeeper_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
};

pub(super) async fn prepare_assignment(
    provider: &mut Provider,
    record: &mut SessionRecord,
    base_sha: &str,
) -> (String, tokio::task::JoinHandle<()>) {
    // An assignment wake needs a real brief before it can reach the actor.
    // Reuse the signed genesis/seat authority fixture, while retaining this
    // test's live actor and checkout (the fixture's execution is not used).
    let relay_keys = Keys::generate();
    let fixture = super::super::team_wake_driver_tests::driver_channel_fixture(
        provider,
        &relay_keys,
        &record.cwd,
        record.channel_id,
        false,
        0,
        |_| 0,
    );
    record.role = fixture.builder_record.role.clone();
    record.actor = fixture.builder_record.actor.clone();
    record.session_ref = Some(fixture.scope.session_ref.clone());
    record.genesis_ref = Some(fixture.scope.genesis_ref.clone());
    provider
        .state
        .insert_session(record.clone())
        .expect("seat facts");
    let assignment_event = build_coding_session_team_transaction(
        &record.channel_id.to_string(),
        coding_session_team_transaction_payload(
            fixture.scope.session_ref.clone(),
            fixture.scope.genesis_ref.clone(),
            None,
            Some("wake-1".into()),
            CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
                assignee_actor: fixture.builder.public_key().to_hex(),
                assignee_role: "builder".into(),
                objective: "Exercise custody at dequeue".into(),
                brief: "The prepared input must still be held when the actor starts.".into(),
                branch: Some("seat/verifier".into()),
                base_sha: Some(base_sha.to_owned()),
                file_ownership: vec!["README.md".into()],
                acceptance_steps: vec!["Check the input at dequeue".into()],
            }),
        ),
    )
    .expect("assignment")
    .sign_with_keys(&fixture.lead)
    .expect("sign assignment");
    let assignment = assignment_event.id.to_hex();
    let mut events = fixture.events;
    events.push(assignment_event);
    let (relay, _queries, server) =
        spawn_test_relay_with_events(&provider.config.keys, events).await;
    provider.set_rest_client(relay.rest_client());
    provider.set_relay_self(relay_keys.public_key().to_hex());
    (assignment, server)
}
