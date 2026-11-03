//! A thread bound to a stopped execution, once Start has replaced it. The thread
//! follows the one current execution the Host journal records as its replacement,
//! and nothing else: every other case keeps the binding, so the reply is refused
//! truthfully. Fixtures walk the journal's real transitions. The new execution
//! starts Fresh, so none of this claims the thread's memory carried over.
use super::conversation_tests::{install_routes, message, routed_provider};
use super::*;
use crate::conversation::{ConversationRoute, ConversationRoutes};
use beekeeper_host_core::{layout::Instance, ownership};

/// `message` is deterministic: two built in one second share an id, and the second
/// would read as a replay of the first.
fn distinct(event: Event) -> Event {
    nostr::EventBuilder::new(nostr::Kind::Custom(9), Uuid::new_v4().to_string())
        .tags(event.tags.iter().cloned())
        .sign_with_keys(test_operator_keys())
        .expect("signed input")
}

struct Stopped {
    host: Provider,
    channel: Uuid,
    actor: Keys,
    old: CodingSessionTarget,
    root: Event,
    home: tempfile::TempDir,
    _dir: tempfile::TempDir,
}

const REVISION: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

/// One thread bound to an execution, and that execution stopped as the Host records
/// it. No successor yet.
async fn stopped() -> Stopped {
    let run = closed().await;
    run.enroll();
    ownership::host_stopped(
        run.home.path(),
        Instance::Dev,
        &run.actor.public_key().to_hex(),
        &run.host.config.relay_url,
        &run.old,
    )
    .expect("host stop");
    run
}

/// The same thread and closed execution, with an empty Host journal.
async fn closed() -> Stopped {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel = Uuid::new_v4();
    let actor = Keys::generate();
    let (mut host, old) = routed_provider(dir.path(), channel, &actor).await;
    let home = tempfile::tempdir().expect("host home");
    host.conversation_ownership_context = Some((home.path().to_path_buf(), Instance::Dev));
    let root = message(channel, &actor, None, test_operator_keys());
    host.conversation_command(channel, &root)
        .expect("bind")
        .expect("command");
    host.state
        .update_session(&old.session_id, |record| record.closed = true)
        .expect("stopped");
    Stopped {
        host,
        channel,
        actor,
        old,
        root,
        home,
        _dir: dir,
    }
}

impl Stopped {
    /// `ownership` as a fresh setup records it: this actor, adopted at `old`.
    fn enroll(&self) {
        ownership::record_host_enrollment(
            self.home.path(),
            Instance::Dev,
            &self.actor.public_key().to_hex(),
            &self.host.config.relay_url,
            &Uuid::new_v4().to_string(),
            REVISION,
            Some(self.old.clone()),
        )
        .expect("enrollment");
    }

    /// What a real Start records: the stopped transfer to history, this one adopted.
    fn adopt(&self, target: &CodingSessionTarget) {
        ownership::successor(
            self.home.path(),
            Instance::Dev,
            &self.actor.public_key().to_hex(),
            &self.host.config.relay_url,
            &Uuid::new_v4().to_string(),
            REVISION,
            target.clone(),
        )
        .expect("successor");
    }

    /// A second execution beside the bound one, as Start creates it.
    fn add(&mut self, edit: impl FnOnce(&mut state::SessionRecord)) -> CodingSessionTarget {
        let mut record = self
            .host
            .state
            .session(&self.old.session_id)
            .expect("old record")
            .clone();
        record.session_id = Uuid::new_v4().to_string();
        record.closed = false;
        edit(&mut record);
        let target = self.host.target_for(&record);
        self.host.state.insert_session(record).expect("successor");
        target
    }

    fn routes(&self, targets: Vec<CodingSessionTarget>) {
        install_routes(&self.host, self.channel, &self.actor, targets);
    }

    fn reply(&self) -> Event {
        distinct(message(
            self.channel,
            &self.actor,
            Some(&self.root.id.to_hex()),
            test_operator_keys(),
        ))
    }

    fn routed(&mut self, event: &Event) -> Option<CodingSessionTarget> {
        self.host
            .conversation_command(self.channel, event)
            .expect("route")
            .map(|command| {
                commands::decode_turn_command(&command)
                    .expect("command")
                    .target
            })
    }

    fn origins(&self) -> serde_json::Value {
        serde_json::from_slice(
            &std::fs::read(self.host.config.state_dir.join("conversation-origins.json"))
                .expect("origins"),
        )
        .expect("json")
    }

    fn bound_session(&self) -> String {
        let key = format!(
            "{}:{}:{}",
            self.channel,
            self.actor.public_key().to_hex(),
            self.root.id.to_hex()
        );
        self.origins()["bindings"][key]["sessionId"]
            .as_str()
            .expect("binding")
            .to_owned()
    }

    /// The reply stays on the stopped execution, and the thread stays bound to it.
    fn assert_kept(&mut self) {
        let reply = self.reply();
        assert_eq!(self.routed(&reply), Some(self.old.clone()));
        assert_eq!(self.bound_session(), self.old.session_id);
    }
}

#[tokio::test]
async fn old_thread_reply_reaches_the_current_execution_and_rebinds() {
    let mut run = stopped().await;
    let current = run.add(|_| {});
    run.adopt(&current);
    run.routes(vec![run.old.clone(), current.clone()]);

    let reply = run.reply();
    assert_eq!(run.routed(&reply), Some(current.clone()));
    assert_eq!(run.bound_session(), current.session_id);

    let origins = run.origins();
    let id = reply.id.to_hex();
    let root = run.root.id.to_hex();
    let origin = &origins["requests"][&id];
    assert_eq!(origin["target"]["sessionId"], current.session_id.as_str());
    assert_eq!(origin["author"], test_operator_keys().public_key().to_hex());
    assert_eq!(origin["root"], root.as_str());
    assert_eq!(origin["event"], id.as_str());
    // The earlier request keeps its own target, which its terminal reply uses.
    assert_eq!(
        origins["requests"][&root]["target"]["sessionId"],
        run.old.session_id.as_str()
    );

    // The next reply needs no second move, and a new top-level mention agrees.
    let next = run.reply();
    assert_eq!(run.routed(&next), Some(current.clone()));
    let top = distinct(message(run.channel, &run.actor, None, test_operator_keys()));
    assert_eq!(run.routed(&top), Some(current));
}

#[tokio::test]
async fn a_thread_bound_to_an_open_execution_holds_it() {
    let mut run = stopped().await;
    run.host
        .state
        .update_session(&run.old.session_id, |record| record.closed = false)
        .expect("open again");
    let other = run.add(|_| {});
    run.adopt(&other);
    run.routes(vec![run.old.clone(), other]);
    run.assert_kept();
}

#[tokio::test]
async fn several_current_executions_keep_the_binding() {
    let mut run = stopped().await;
    let (one, two) = (run.add(|_| {}), run.add(|_| {}));
    run.adopt(&one);
    run.routes(vec![run.old.clone(), one, two]);
    run.assert_kept();
}

#[tokio::test]
async fn no_successor_keeps_the_binding_and_is_refused() {
    let mut run = stopped().await;
    run.assert_kept();
    let reply = run.reply();
    run.host
        .handle_command_event(run.channel, &reply)
        .await
        .expect("refusal");
    let sink = CollectingSink::new();
    run.host.flush(&sink).await.expect("receipts");
    assert!(sink
        .all()
        .iter()
        .any(|event| event.kind == nostr::Kind::Custom(9)
            && event.content.contains("SESSION_CLOSED")));
    assert!(run.host.in_flight.is_empty());
}

#[tokio::test]
async fn a_stale_generation_is_not_a_successor() {
    let mut run = stopped().await;
    let mut stale = run.add(|_| {});
    stale.generation += 1;
    run.adopt(&stale);
    run.routes(vec![run.old.clone(), stale]);
    run.assert_kept();
}

#[tokio::test]
async fn another_project_runtime_or_provider_is_not_a_successor() {
    let edits: [fn(&mut state::SessionRecord); 4] = [
        |record| record.project_ref = Some("30621:other:project".into()),
        |record| record.repo_ref = Some("30617:other:repo".into()),
        |record| record.provider_instance_ref = "another-runtime".into(),
        |record| record.driver = "another-driver".into(),
    ];
    for edit in edits {
        let mut run = stopped().await;
        let other = run.add(edit);
        run.adopt(&other);
        run.routes(vec![run.old.clone(), other]);
        run.assert_kept();
    }
}

#[tokio::test]
async fn another_actors_open_route_is_not_a_successor() {
    let mut run = stopped().await;
    let other_actor = Keys::generate();
    let other = run.add(|record| record.actor = Some(other_actor.public_key().to_hex()));
    let mut routes: ConversationRoutes = serde_json::from_slice(
        &std::fs::read(run.host.config.state_dir.join("conversation-routes.json")).expect("routes"),
    )
    .expect("json");
    routes.routes.push(ConversationRoute {
        actor_pubkey: other_actor.public_key().to_hex(),
        channel_id: run.channel,
        target: other,
        replay_floor_unix: None,
    });
    state::atomic_write(
        &run.host.config.state_dir.join("conversation-routes.json"),
        &serde_json::to_vec(&routes).expect("routes"),
    )
    .expect("routes");
    run.assert_kept();
}

#[tokio::test]
async fn a_retired_execution_is_not_replaced() {
    let mut run = stopped().await;
    let current = run.add(|_| {});
    run.adopt(&current);
    run.routes(vec![run.old.clone(), current]);
    run.host
        .state
        .update_session(&run.old.session_id, |record| {
            record.retired = Some(state::Retirement {
                deletion_event_id: "ee".repeat(32),
                receipt_event_id: None,
                at: 1,
            });
        })
        .expect("retired");
    run.assert_kept();
}

#[tokio::test]
async fn an_author_permitted_only_on_the_old_execution_does_not_follow() {
    let mut run = stopped().await;
    let owner = Keys::generate().public_key().to_hex();
    let current = run.add(|record| {
        record.founder_pubkey = Some(owner.clone());
        record.created_by = Some(owner.clone());
    });
    run.adopt(&current);
    run.routes(vec![run.old.clone(), current]);
    run.assert_kept();
}

#[tokio::test]
async fn a_stranger_reaches_nothing_and_moves_nothing() {
    let mut run = stopped().await;
    let current = run.add(|_| {});
    run.adopt(&current);
    run.routes(vec![run.old.clone(), current]);
    let stranger = message(
        run.channel,
        &run.actor,
        Some(&run.root.id.to_hex()),
        &Keys::generate(),
    );
    assert_eq!(run.routed(&stranger), None);
    assert_eq!(run.bound_session(), run.old.session_id);
}

#[tokio::test]
async fn a_refused_reply_stays_refused_once_a_successor_exists() {
    let mut run = stopped().await;
    let refused = run.reply();
    run.host
        .handle_command_event(run.channel, &refused)
        .await
        .expect("refusal");
    let sink = CollectingSink::new();
    run.host.flush(&sink).await.expect("receipts");
    let id = refused.id.to_hex();
    // Refused, and not consumed: consuming would claim it ran.
    assert!(run.host.state.is_command_refused(&id));
    assert!(!run.host.state.is_command_consumed(&id));

    let current = run.add(|_| {});
    run.adopt(&current);
    run.routes(vec![run.old.clone(), current.clone()]);

    // Delivered again: no turn, and nothing sent to the current execution.
    run.host
        .handle_command_event(run.channel, &refused)
        .await
        .expect("redelivery");
    let sink = CollectingSink::new();
    run.host.flush(&sink).await.expect("receipts");
    assert!(run.host.in_flight.is_empty());
    assert!(!sink.all().iter().any(|event| {
        event.content.contains(&id) && event.content.contains(&current.session_id)
    }));
    assert_eq!(
        run.origins()["requests"][&id]["target"]["sessionId"],
        run.old.session_id.as_str()
    );

    // A new reply moves the thread, and replaying the old one does not move it back.
    let next = run.reply();
    assert_eq!(run.routed(&next), Some(current.clone()));
    assert_eq!(run.routed(&refused), Some(run.old.clone()));
    assert_eq!(run.bound_session(), current.session_id);
}

/// The review's finding. Two responsibilities of one actor match on every field,
/// and only one stopped. The journal never recorded B as A's replacement.
#[tokio::test]
async fn a_sibling_with_matching_fields_does_not_take_over_the_thread() {
    let mut run = closed().await;
    run.enroll();
    let sibling = run.add(|_| {});
    run.routes(vec![run.old.clone(), sibling.clone()]);

    run.assert_kept();
    let new_thread = distinct(message(run.channel, &run.actor, None, test_operator_keys()));
    assert_eq!(run.routed(&new_thread), Some(sibling));
    assert_eq!(run.bound_session(), run.old.session_id);
}

#[tokio::test]
async fn a_stop_with_no_recorded_start_is_not_a_replacement() {
    let mut run = stopped().await;
    let current = run.add(|_| {});
    run.routes(vec![run.old.clone(), current]);
    run.assert_kept();
}

#[tokio::test]
async fn a_replacement_recorded_for_another_execution_does_not_count() {
    let mut run = stopped().await;
    let recorded = run.add(|_| {});
    run.adopt(&recorded);
    run.host
        .state
        .update_session(&recorded.session_id, |record| record.closed = true)
        .expect("that one stopped too");
    let unrecorded = run.add(|_| {});
    run.routes(vec![run.old.clone(), recorded, unrecorded]);
    run.assert_kept();
}

#[tokio::test]
async fn an_empty_journal_keeps_the_binding() {
    let mut run = closed().await;
    let current = run.add(|_| {});
    run.routes(vec![run.old.clone(), current]);
    run.assert_kept();
}

#[tokio::test]
async fn no_host_context_keeps_the_binding() {
    let mut run = stopped().await;
    let current = run.add(|_| {});
    run.adopt(&current);
    run.routes(vec![run.old.clone(), current]);
    run.host.conversation_ownership_context = None;
    run.assert_kept();
}

#[tokio::test]
async fn an_unreadable_journal_moves_nothing() {
    let mut run = stopped().await;
    let current = run.add(|_| {});
    run.adopt(&current);
    run.routes(vec![run.old.clone(), current.clone()]);
    let path = beekeeper_host_core::layout::host_dir(run.home.path(), Instance::Dev)
        .join("managed-ownership.json");
    std::fs::write(path, b"{not a journal").expect("corrupt");
    let reply = run.reply();
    let routed = run.host.conversation_command(run.channel, &reply);
    let moved = routed.ok().flatten().map(|command| {
        commands::decode_turn_command(&command)
            .expect("command")
            .target
    });
    assert_ne!(moved, Some(current));
    assert_eq!(run.bound_session(), run.old.session_id);
}
