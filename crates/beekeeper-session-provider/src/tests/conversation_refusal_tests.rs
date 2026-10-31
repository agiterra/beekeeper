//! A source-specific refused reply cannot poison the existing conversation path.
use super::*;

struct RefuseThread {
    root: String,
    delivered: std::sync::Mutex<Vec<Event>>,
}
impl crate::publish::EventSink for RefuseThread {
    async fn publish(&self, event: Event) -> Result<(), String> {
        if event.kind == nostr::Kind::Custom(9)
            && event
                .tags
                .iter()
                .any(|t| t.as_slice().get(1) == Some(&self.root))
        {
            return Err("invalid: reply parent unavailable".into());
        }
        self.delivered.lock().unwrap().push(event);
        Ok(())
    }
}
#[tokio::test]
async fn refused_source_reply_is_parked_while_next_thread_runs_once() {
    let dir = tempfile::tempdir().unwrap();
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, _) = routed_provider(dir.path(), channel, &actor).await;
    let doomed = message(channel, &actor, None, test_operator_keys());
    host.handle_command_event(channel, &doomed).await.unwrap();
    pump_until_turn_finished(&mut host).await;
    let fresh = nostr::EventBuilder::new(nostr::Kind::Custom(9), "another ordinary question")
        .tags(doomed.tags.clone())
        .sign_with_keys(test_operator_keys())
        .unwrap();
    host.handle_command_event(channel, &fresh).await.unwrap();
    pump_until_turn_finished(&mut host).await;
    let sink = RefuseThread {
        root: doomed.id.to_hex(),
        delivered: Default::default(),
    };
    host.flush(&sink).await.unwrap();
    assert!(host.outbox.parked_len() > 0);
    assert!(sink
        .delivered
        .lock()
        .unwrap()
        .iter()
        .any(|e| e.kind == nostr::Kind::Custom(9)
            && e.tags
                .iter()
                .any(|t| t.as_slice().get(1) == Some(&fresh.id.to_hex()))));
    assert!(host.state.is_command_consumed(&doomed.id.to_hex()));
    assert!(host.state.is_command_consumed(&fresh.id.to_hex()));
    let count = sink.delivered.lock().unwrap().len();
    host.handle_command_event(channel, &doomed).await.unwrap();
    host.handle_command_event(channel, &fresh).await.unwrap();
    host.flush(&sink).await.unwrap();
    assert_eq!(sink.delivered.lock().unwrap().len(), count);
    assert!(!host.outbox.pending_len().gt(&0));
    let config = host.config.clone();
    let sessions: Vec<_> = host
        .state
        .sessions()
        .map(|r| r.session_id.clone())
        .collect();
    for id in sessions {
        host.sessions
            .shutdown_and_join(&id, crate::session::ACTOR_RETIRE_GRACE)
            .await;
    }
    drop(host);
    let mut recovered = Provider::new(config).unwrap();
    assert!(recovered.outbox.parked_len() > 0);
    recovered
        .handle_command_event(channel, &doomed)
        .await
        .unwrap();
    recovered
        .handle_command_event(channel, &fresh)
        .await
        .unwrap();
    assert!(recovered.in_flight.is_empty());
}
