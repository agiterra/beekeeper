//! The conversation path sends every channel mention as a `steer`; the sender
//! never chose that class. On a runtime that cannot take a native steer the
//! downgrade notice ("Request queued for the next turn boundary … cannot
//! apply this request as a native steer") is true only while a turn is in
//! flight. To an idle participant the message simply starts a turn.

use super::*;

const QUEUED_NOTICE: &str = "Request queued for the next turn boundary";

/// Admit one ordinary channel message through the real replay path and return the
/// channel replies and the receipt stages for it.
async fn admit(host: &mut Provider, channel: Uuid, event: &Event) -> (Vec<String>, Vec<String>) {
    host.open_replay_window(channel);
    host.handle_command_event(channel, event)
        .await
        .expect("hold");
    host.flush_replays_now()
        .await
        .expect("existing replay admission");
    let sink = CollectingSink::new();
    host.flush(&sink).await.expect("outbox");
    let replies = sink
        .all()
        .into_iter()
        .filter(|e| e.kind == nostr::Kind::Custom(9))
        .map(|e| e.content)
        .collect();
    (replies, receipt_stages(&sink, &event.id.to_hex()))
}

#[tokio::test]
async fn an_idle_participant_on_a_no_steer_runtime_is_not_told_its_message_was_queued() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, target) = routed_provider(dir.path(), channel, &actor).await;
    assert!(!host.native_steer_deliverable(&target.session_id));
    assert!(
        !host.turn_in_flight(&target.session_id),
        "idle before the mention"
    );

    let event = message(channel, &actor, None, test_operator_keys());
    let (replies, stages) = admit(&mut host, channel, &event).await;
    assert!(
        !stages.contains(&"turn_degraded".to_owned()),
        "nothing was running, so nothing was downgraded: {stages:?}"
    );
    assert!(stages.contains(&"turn_queued".to_owned()), "{stages:?}");
    assert!(
        replies
            .iter()
            .all(|reply| !reply.contains(QUEUED_NOTICE) && !reply.contains("native steer")),
        "an ordinary message to an idle participant is not narrated as a steer: {replies:?}"
    );
}

#[tokio::test]
async fn a_busy_participant_on_a_no_steer_runtime_is_told_the_message_waits_for_the_boundary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, target) = routed_provider(dir.path(), channel, &actor).await;
    assert!(!host.native_steer_deliverable(&target.session_id));

    // The first request is in the execution's custody and not yet settled
    // (nothing pumps the actor here), so a turn is genuinely in flight.
    let first = message(channel, &actor, None, test_operator_keys());
    let (_, first_stages) = admit(&mut host, channel, &first).await;
    assert!(
        !first_stages.contains(&"turn_degraded".to_owned()),
        "{first_stages:?}"
    );
    assert!(
        host.turn_in_flight(&target.session_id),
        "the first turn is in flight"
    );

    let second = message(
        channel,
        &actor,
        Some(&first.id.to_hex()),
        test_operator_keys(),
    );
    let (replies, stages) = admit(&mut host, channel, &second).await;
    assert!(
        stages.contains(&"turn_degraded".to_owned()),
        "a turn is running and the runtime cannot steer it: {stages:?} {replies:?}"
    );
    assert!(
        replies.iter().any(|reply| reply.starts_with(QUEUED_NOTICE)
            && reply.contains("cannot apply this request as a native steer")),
        "the sender learns the words wait for the running turn: {replies:?}"
    );
}
