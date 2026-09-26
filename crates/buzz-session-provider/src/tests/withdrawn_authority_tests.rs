//! An execution whose own authority is withdrawn stops, through the existing
//! stop path, and does not come back under it.
//!
//! Two withdrawals the provider already sees as authoritative events:
//! the execution's own seat revoked (an accepted `revoke-seat` naming its
//! actor), and its project deleted (the NIP-09 kind 5 naming the project
//! coordinate, applied by the relay). Each test runs a real process and
//! checks it is gone, then asks for it back and is refused. An unrelated
//! execution — another seat, another project, a project re-created after its
//! tombstone — keeps running.

use super::*;

use crate::session::testing::{group_gone_within, group_is_gone, legacy_request, lingering_agent};

/// The agent's pid (its process group's id): a live execution's adapter.
fn adapter_pid(provider: &Provider, session_id: &str) -> u32 {
    provider
        .sessions
        .handle(session_id)
        .expect("live handle")
        .child_pid_for_tests()
        .expect("a spawned adapter has a pid")
}

/// A stopped execution is its adapter's whole process group gone from the
/// OS — the grandchild it left running included — not a dropped handle.
async fn assert_stopped(pid: u32, what: &str) {
    assert!(
        group_gone_within(pid, Duration::from_secs(10)).await,
        "{what}: its adapter or a process it started is still running"
    );
}

fn seat_transition(
    channel_id: Uuid,
    genesis_ref: &str,
    prev: Option<String>,
    seq: u32,
    grantee_hex: &str,
    transition_type: CodingSessionAuthorityTransitionType,
) -> Event {
    let mut payload =
        buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload::new(
            transition_type,
            genesis_ref.to_owned(),
            prev,
            seq,
            grantee_hex.to_owned(),
        );
    payload.role = Some("builder".to_owned());
    buzz_sdk::builders::build_coding_session_authority_transition(channel_id, &payload)
        .expect("transition builder")
        .sign_with_keys(test_operator_keys())
        .expect("sign transition")
}

fn failed_codes(sink: &CollectingSink) -> Vec<String> {
    sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .iter()
        .filter(|receipt| receipt["status"] == "failed")
        .filter_map(|receipt| receipt["error"]["code"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn a_revoked_seat_stops_its_own_running_execution_and_it_does_not_resume() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&dir.path().join("state"), None);
    let relay_keys = Keys::generate();
    provider.set_relay_self(relay_keys.public_key().to_hex());
    let genesis_ref = "ab".repeat(32);
    let seat = Keys::generate().public_key().to_hex();
    let other_seat = Keys::generate().public_key().to_hex();

    let mut record = governed_record(channel_id, &cwd, &genesis_ref);
    record.actor = Some(seat.clone());
    record.role = Some("builder".to_owned());
    let session_id = record.session_id.clone();
    let target = record.target(&provider.config.instance_id);
    provider.state.insert_session(record).expect("insert");
    let mut bystander = governed_record(channel_id, &cwd, &genesis_ref);
    bystander.actor = Some(other_seat.clone());
    bystander.role = Some("verifier".to_owned());
    let bystander_id = bystander.session_id.clone();
    provider
        .state
        .insert_session(bystander)
        .expect("insert bystander");

    // Both seats' executions are running, each with a grandchild in its
    // adapter's process group.
    let agent = fake_agent(dir.path(), "lingering-agent", &lingering_agent());
    for id in [&session_id, &bystander_id] {
        provider
            .sessions
            .create(legacy_request(&agent, &cwd, id))
            .await
            .expect("start");
    }
    assert_eq!(provider.sessions.live_count(), 2);
    let seat_pid = adapter_pid(&provider, &session_id);
    let bystander_pid = adapter_pid(&provider, &bystander_id);

    let grant = seat_transition(
        channel_id,
        &genesis_ref,
        None,
        1,
        &seat,
        CodingSessionAuthorityTransitionType::GrantSeat,
    );
    let revoke = seat_transition(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &seat,
        CodingSessionAuthorityTransitionType::RevokeSeat,
    );
    let (mut relay, _queries, server) =
        spawn_test_relay_with_events(&provider.config.keys, vec![grant.clone(), revoke.clone()])
            .await;
    for transition in [&grant, &revoke] {
        let receipt = acceptance_receipt_for_transition(&relay_keys, channel_id, transition);
        provider
            .handle_relay_event(&mut relay, channel_id, &receipt)
            .await
            .expect("apply");
    }

    let stopped = provider.state().session(&session_id).expect("record");
    assert!(stopped.closed, "the revoked seat's execution was stopped");
    assert!(stopped.authority_withdrawn.is_some(), "{stopped:?}");
    assert_stopped(seat_pid, "the revoked seat's execution").await;
    let bystander = provider.state().session(&bystander_id).expect("bystander");
    assert!(
        !bystander.closed && bystander.authority_withdrawn.is_none(),
        "another seat was stopped"
    );
    assert!(
        !group_is_gone(bystander_pid),
        "another seat's process was stopped"
    );

    // Asked back: refused, and nothing starts.
    let resume = lifecycle_target_event(
        &provider,
        channel_id,
        "resume-after-revoke-seat",
        "session.resume",
        &target,
    );
    provider
        .handle_command_event(channel_id, &resume)
        .await
        .expect("handle resume");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    // Stopped is terminal: the resume is refused before anything is prepared.
    assert!(
        failed_codes(&sink)
            .iter()
            .any(|code| code == payload::SESSION_CLOSED),
        "{:?}",
        failed_codes(&sink)
    );
    assert_eq!(
        provider.sessions.live_count(),
        1,
        "the resume started nothing"
    );
    assert!(
        !group_is_gone(bystander_pid),
        "another seat's process was stopped"
    );

    provider.sessions.shutdown(&bystander_id);
    assert_stopped(bystander_pid, "the bystander, at teardown").await;
    provider.settle_git_probes().await;
    relay.shutdown().await;
    server.abort();
}

fn project_create(
    provider: &Provider,
    channel_id: Uuid,
    command_id: &str,
    project_ref: &str,
) -> Event {
    let content = serde_json::json!({
        "schema": "buzz-coding-session-lifecycle-command/v1",
        "commandId": command_id,
        "action": {
            "type": "session.create",
            "projectRef": project_ref,
            "repoRef": null,
            "providerInstanceRef": "claude-primary",
            "providerAuthorityPubkey": provider.config.pubkey_hex(),
            "model": null,
            "title": "Ship it",
            "initialTurn": null,
        },
    })
    .to_string();
    signed_lifecycle_event(channel_id, content)
}

fn tombstone_at(signer: &Keys, project_ref: &str, created_at: u64) -> Event {
    nostr::EventBuilder::new(nostr::Kind::EventDeletion, "project deleted")
        .tags(vec![nostr::Tag::parse(["a", project_ref]).expect("tag")])
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(signer)
        .expect("sign tombstone")
}

fn head_at(owner: &Keys, dtag: &str, created_at: u64) -> Event {
    nostr::EventBuilder::new(nostr::Kind::Custom(30621), "{}")
        .tags(vec![nostr::Tag::parse(["d", dtag]).expect("tag")])
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(owner)
        .expect("sign head")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// A provider whose projects file records `checkout` as `project_ref`'s
/// checkout, the relay serving the project's head (seen by the create's own
/// revalidation), and one running execution created through the real path.
async fn project_provider(
    dir: &Path,
    owner: &Keys,
    project_ref: &str,
    head_created_at: u64,
) -> (
    Provider,
    HarnessRelay,
    RecordingTestRelay,
    tokio::task::JoinHandle<()>,
    Uuid,
    String,
) {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = dir.join("projects.json");
    std::fs::write(
        &projects,
        serde_json::json!({
            "version": 1,
            "projects": { project_ref: cwd },
            "channels": { channel_id.to_string(): cwd },
        })
        .to_string(),
    )
    .expect("projects");
    let agent = fake_agent(dir, "lingering-agent", &lingering_agent());
    let mut provider = Provider::new(config_of(
        Keys::generate(),
        &dir.join("state"),
        Some(&projects),
        agent,
    ))
    .expect("provider");
    let (mut relay, control, server) = spawn_recording_test_relay(
        &provider.config.keys,
        vec![head_at(owner, "demo", head_created_at)],
    )
    .await;
    provider.set_rest_client(relay.rest_client());
    let create = project_create(&provider, channel_id, "create-project", project_ref);
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");
    let session_id = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .session_id
        .clone();
    assert_eq!(
        provider.sessions.live_count(),
        1,
        "the create started a process"
    );
    assert_eq!(
        provider.seen_project_head(project_ref),
        Some(head_created_at),
        "the start's revalidation saw the project's head"
    );
    (provider, relay, control, server, channel_id, session_id)
}

/// The project's deletion — signed by a roster Owner rather than the
/// creator, which the relay admits the same way — reaches the provider
/// through the project-deletion subscription, is judged applied against the
/// head the provider saw, and stops the running execution; neither a resume
/// nor a fresh create of that project starts anything afterwards.
#[tokio::test]
async fn a_deleted_project_stops_its_running_execution_and_no_execution_of_it_restarts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let creator = Keys::generate();
    let roster_owner = Keys::generate();
    let project_ref = format!("30621:{}:demo", creator.public_key().to_hex());
    let head_created = now() - 600;
    let (mut provider, mut relay, control, server, channel_id, session_id) =
        project_provider(dir.path(), &creator, &project_ref, head_created).await;
    let target = provider
        .state()
        .session(&session_id)
        .expect("record")
        .target(&provider.config.instance_id);
    let execution_pid = adapter_pid(&provider, &session_id);
    // Another project's execution, running beside it.
    let mut bystander = governed_record(channel_id, &dir.path().join("checkout"), &"cd".repeat(32));
    bystander.project_ref = Some(format!("30621:{}:other", creator.public_key().to_hex()));
    let bystander_id = bystander.session_id.clone();
    provider
        .state
        .insert_session(bystander)
        .expect("insert bystander");
    let agent = fake_agent(dir.path(), "lingering-bystander", &lingering_agent());
    provider
        .sessions
        .create(legacy_request(
            &agent,
            &dir.path().join("checkout"),
            &bystander_id,
        ))
        .await
        .expect("start bystander");
    let bystander_pid = adapter_pid(&provider, &bystander_id);

    // Deleted and applied: the relay holds the tombstone and no longer the head.
    {
        let mut events = control.events.lock().expect("events");
        events.retain(|event| event.kind != nostr::Kind::Custom(30621));
        events.push(tombstone_at(&roster_owner, &project_ref, now()));
    }
    let (listener, mut deletions) = project_deletion::ProjectDeletionListener::spawn(
        project_deletion::DeletionListenerConfig {
            relay_url: control.url.clone(),
            keys: provider.config.keys.clone(),
            auth_tag: None,
        },
    );
    provider.project_deletion_listener = Some(listener);
    provider.sync_project_deletion_listener();
    for _ in 0..4 {
        let event = tokio::time::timeout(Duration::from_secs(20), deletions.recv())
            .await
            .expect("a deletion event within the deadline")
            .expect("listener open");
        provider.handle_project_deletion_event(event);
        if provider
            .state()
            .session(&session_id)
            .is_some_and(|record| record.closed)
        {
            break;
        }
    }
    let record = provider.state().session(&session_id).expect("record");
    assert!(record.closed, "the deleted project's execution was stopped");
    assert!(record.authority_withdrawn.is_some());
    assert_stopped(execution_pid, "the deleted project's execution").await;
    assert!(
        !group_is_gone(bystander_pid),
        "another project's execution was stopped"
    );

    let resume = lifecycle_target_event(
        &provider,
        channel_id,
        "resume-deleted",
        "session.resume",
        &target,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &resume)
        .await
        .expect("resume");
    let recreate = project_create(&provider, channel_id, "create-deleted", &project_ref);
    provider
        .handle_relay_event(&mut relay, channel_id, &recreate)
        .await
        .expect("create");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let codes = failed_codes(&sink);
    assert!(
        codes.iter().any(|code| code == payload::SESSION_CLOSED),
        "{codes:?}"
    );
    assert!(
        codes
            .iter()
            .any(|code| code == crate::execution_scope::EXECUTION_SCOPE_INVALID),
        "{codes:?}"
    );
    assert_eq!(
        provider.sessions.live_count(),
        1,
        "nothing of the deleted project restarted"
    );
    assert!(
        !group_is_gone(bystander_pid),
        "another project's execution was stopped"
    );

    if let Some(listener) = provider.project_deletion_listener.take() {
        listener.shutdown();
    }
    provider.sessions.shutdown(&bystander_id);
    assert_stopped(bystander_pid, "the bystander, at teardown").await;
    provider.settle_git_probes().await;
    relay.shutdown().await;
    server.abort();
}

/// What is not a proven deletion stops nothing: a project re-created after
/// its tombstone, a tombstone whose head the relay still serves, a head this
/// host never saw, and a relay that could not be read.
#[tokio::test]
async fn only_an_applied_deletion_of_the_head_this_host_saw_stops_anything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let creator = Keys::generate();
    let project_ref = format!("30621:{}:demo", creator.public_key().to_hex());
    let head_created = now() - 600;
    let (mut provider, relay, control, server, _channel_id, session_id) =
        project_provider(dir.path(), &creator, &project_ref, head_created).await;
    let rest = provider.rest_client.clone().expect("rest");
    let seen = provider.seen_project_head(&project_ref);

    // Re-created: the relay serves a head newer than the tombstone.
    {
        let mut events = control.events.lock().expect("events");
        events.clear();
        events.push(head_at(&creator, "demo", now()));
    }
    let fact = project_deletion::check(&rest, &project_ref, head_created + 1, seen).await;
    assert!(
        matches!(fact, project_deletion::ProjectFact::Present { .. }),
        "{fact:?}"
    );
    provider.handle_project_deletion_event(project_deletion::ProjectDeletionEvent::Checked {
        project: project_ref.clone(),
        tombstone_at: head_created + 1,
        fact,
    });

    // A judgement of that old tombstone arriving late, after the newer head
    // was witnessed: stale, so it stops nothing.
    provider.handle_project_deletion_event(project_deletion::ProjectDeletionEvent::Checked {
        project: project_ref.clone(),
        tombstone_at: head_created + 1,
        fact: project_deletion::ProjectFact::Deleted,
    });

    // Not applied: the head the tombstone is newer than is still served.
    {
        let mut events = control.events.lock().expect("events");
        events.clear();
        events.push(head_at(&creator, "demo", head_created));
    }
    let fact = project_deletion::check(&rest, &project_ref, now(), seen).await;
    assert_eq!(fact, project_deletion::ProjectFact::NotApplied);

    // A head this host never saw, now missing: unknown, not deleted.
    control.events.lock().expect("events").clear();
    let fact = project_deletion::check(&rest, &project_ref, now(), None).await;
    assert!(
        matches!(fact, project_deletion::ProjectFact::Unknown(_)),
        "{fact:?}"
    );

    // A relay that cannot answer: unknown, not deleted.
    server.abort();
    let fact = project_deletion::check(&rest, &project_ref, now(), seen).await;
    assert!(
        matches!(fact, project_deletion::ProjectFact::Unknown(_)),
        "{fact:?}"
    );

    let record = provider.state().session(&session_id).expect("record");
    assert!(!record.closed && record.authority_withdrawn.is_none());
    assert_eq!(
        provider.sessions.live_count(),
        1,
        "the execution keeps running"
    );
    let pid = adapter_pid(&provider, &session_id);
    assert!(!group_is_gone(pid), "the execution keeps running");
    provider.sessions.shutdown(&session_id);
    assert_stopped(pid, "the execution, at teardown").await;
    provider.settle_git_probes().await;
    relay.shutdown().await;
}

/// A deletion made while the provider was down: the head it witnessed is in
/// its durable records, so after a restart the replayed tombstone and the
/// missing head are recognised as an applied deletion — the old execution
/// does not reopen, and is marked withdrawn.
#[tokio::test]
async fn an_offline_deletion_is_recognised_after_the_provider_restarts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let creator = Keys::generate();
    let project_ref = format!("30621:{}:demo", creator.public_key().to_hex());
    let head_created = now() - 600;
    let (mut first, relay, control, server, channel_id, session_id) =
        project_provider(dir.path(), &creator, &project_ref, head_created).await;
    assert_eq!(
        first
            .state()
            .session(&session_id)
            .expect("record")
            .project_head_seen_at,
        Some(head_created),
        "the witnessed head is part of the durable record"
    );
    let target = first
        .state()
        .session(&session_id)
        .expect("record")
        .target(&first.config.instance_id);
    let pid = adapter_pid(&first, &session_id);
    first.sessions.shutdown(&session_id);
    assert_stopped(pid, "the first provider's execution").await;
    first.settle_git_probes().await;
    drop(first);

    // Deleted while no provider ran.
    {
        let mut events = control.events.lock().expect("events");
        events.retain(|event| event.kind != nostr::Kind::Custom(30621));
        events.push(tombstone_at(&creator, &project_ref, now()));
    }
    let mut restarted = provider(
        &dir.path().join("state"),
        Some(&dir.path().join("projects.json")),
    );
    restarted.set_rest_client(relay.rest_client());
    assert_eq!(
        restarted.seen_project_head(&project_ref),
        Some(head_created),
        "the fact survived the restart"
    );
    let mut relay = relay;
    let resume = lifecycle_target_event(
        &restarted,
        channel_id,
        "resume-after-offline-deletion",
        "session.resume",
        &target,
    );
    restarted
        .handle_relay_event(&mut relay, channel_id, &resume)
        .await
        .expect("resume");
    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let codes = failed_codes(&sink);
    assert!(
        codes
            .iter()
            .any(|code| code == crate::execution_scope::EXECUTION_SCOPE_INVALID),
        "{codes:?}"
    );
    assert_eq!(
        restarted.sessions.live_count(),
        0,
        "the old execution did not reopen"
    );
    let record = restarted.state().session(&session_id).expect("record");
    assert!(
        record.closed && record.authority_withdrawn.is_some(),
        "{record:?}"
    );

    restarted.settle_git_probes().await;
    relay.shutdown().await;
    server.abort();
}
