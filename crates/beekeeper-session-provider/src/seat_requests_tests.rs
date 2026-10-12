//! What the seat-request ledger derives, and what it deliberately omits.

use super::*;

use beekeeper_core::coding_session_authority_claim::ClaimState;
use std::collections::BTreeSet;
use std::path::PathBuf;

use uuid::Uuid;

/// This provider's own authority pubkey, as the fence asks about it.
const TEST_PROVIDER: &str = "99";

/// A minimal open session record. Every test below varies only the fields it
/// is actually about.
fn record(session_id: &str, command_id: &str) -> SessionRecord {
    SessionRecord {
        execution_binding: None,
        execution_boundary: None,
        process_witness: None,
        authority_withdrawn: None,
        project_head_seen_at: None,
        session_id: session_id.to_owned(),
        generation: 1,
        channel_id: Uuid::nil(),
        command_id: command_id.to_owned(),
        generation_command_id: Some(command_id.to_owned()),
        provider_instance_ref: "claude-primary".into(),
        runtime: "claude".into(),
        driver: "claude".into(),
        cwd: PathBuf::from("/tmp/checkout"),
        project_ref: None,
        repo_ref: None,
        session_ref: None,
        genesis_ref: None,
        actor: None,
        role: None,
        pack_ref: None,
        compose_ref: None,
        founder_pubkey: None,
        granted_operators: BTreeSet::new(),
        granted_viewers: BTreeSet::new(),
        authority_seq: 0,
        model: None,
        model_requested: None,
        model_effective: None,
        routing: None,
        resume_cursor: Some("saved-acp-session".into()),
        title: None,
        created_at_ms: 0,
        next_seq: 1,
        next_lease_sequence: 1,
        bootstrap_transport: None,
        open_turn: None,
        closed: false,
        created_by: None,
        handover: ClaimState::NoClaim,
        retired: None,
    }
}

fn seated(session_id: &str, command_id: &str, actor: &str) -> SessionRecord {
    let mut record = record(session_id, command_id);
    record.actor = Some(actor.to_owned());
    record.role = Some("builder".into());
    record.project_ref = Some(format!("30621:{}:beekeeper", "11".repeat(32)));
    record
}

/// The key the desktop files custody under is the *generation's* command id,
/// not the create's: after a resume those differ, and staging under the create
/// would leave the restore path finding nothing.
#[test]
fn a_resumed_generation_asks_for_custody_under_its_own_command_id() {
    let mut seat = seated("session-1", "create-1", &"aa".repeat(32));
    seat.generation = 3;
    seat.generation_command_id = Some("resume-2".into());

    let rows = derive_seat_requests([&seat], TEST_PROVIDER);

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].command_id, "resume-2");
    assert_eq!(rows[0].generation, 3);
    assert_eq!(rows[0].session_id, "session-1");
}

/// A record written before generation commands were persisted names its
/// generation with the create's command id, and that is what custody keys on.
#[test]
fn a_record_without_a_generation_command_falls_back_to_the_create() {
    let mut seat = seated("session-1", "create-1", &"aa".repeat(32));
    seat.generation_command_id = None;

    let rows = derive_seat_requests([&seat], TEST_PROVIDER);

    assert_eq!(rows[0].command_id, "create-1");
}

/// Only open, seated generations are asked for. A stopped one must not have a
/// key re-staged for it, and an unseated one has no key at all.
#[test]
fn closed_and_unseated_generations_ask_for_nothing() {
    let unseated = record("session-human", "create-h");
    let mut stopped = seated("session-stopped", "create-s", &"bb".repeat(32));
    stopped.closed = true;
    let open = seated("session-open", "create-o", &"cc".repeat(32));

    let rows = derive_seat_requests([&unseated, &stopped, &open], TEST_PROVIDER);

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].session_id, "session-open");
}

/// The exact key set the desktop reader parses, and nothing that could carry a
/// secret.
#[test]
fn a_row_carries_the_public_facts_only() {
    let seat = seated("session-1", "create-1", &"aa".repeat(32));
    let dir = tempfile::tempdir().expect("tempdir");

    write_seat_requests(dir.path(), [&seat], TEST_PROVIDER).expect("write");

    let body = std::fs::read_to_string(seat_requests_path(dir.path())).expect("read");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(parsed["version"], 1);
    assert_eq!(
        parsed["requests"][0],
        serde_json::json!({
            "commandId": "create-1",
            "actor": "aa".repeat(32),
            "role": "builder",
            "projectRef": format!("30621:{}:beekeeper", "11".repeat(32)),
            "sessionId": "session-1",
            "generation": 1,
        }),
        "{body}"
    );
    // The custody file's own fields must never appear here.
    for forbidden in ["nsec", "authTag", "cwd", "packDir", "relayUrl"] {
        assert!(!body.contains(forbidden), "{forbidden} leaked into {body}");
    }
}

/// A rewrite replaces the whole set: a generation that stopped stops asking.
#[test]
fn a_rewrite_drops_the_rows_that_no_longer_apply() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = seated("session-1", "create-1", &"aa".repeat(32));
    let second = seated("session-2", "create-2", &"bb".repeat(32));

    write_seat_requests(dir.path(), [&first, &second], TEST_PROVIDER).expect("write both");
    write_seat_requests(dir.path(), [&second], TEST_PROVIDER).expect("rewrite");

    let file: SeatRequestsFile = serde_json::from_str(
        &std::fs::read_to_string(seat_requests_path(dir.path())).expect("read"),
    )
    .expect("parse");
    assert_eq!(file.requests.len(), 1);
    assert_eq!(file.requests[0].session_id, "session-2");
}

/// Nothing seated is still an answer, not an absent file: the desktop has to
/// be able to tell "no custody is needed" from "the provider never wrote".
#[test]
fn nothing_seated_writes_an_empty_request_list() {
    let dir = tempfile::tempdir().expect("tempdir");

    write_seat_requests(
        dir.path(),
        [&record("session-human", "create-h")],
        TEST_PROVIDER,
    )
    .expect("write");

    let file: SeatRequestsFile = serde_json::from_str(
        &std::fs::read_to_string(seat_requests_path(dir.path())).expect("read"),
    )
    .expect("parse");
    assert_eq!(file.version, SEAT_REQUESTS_VERSION);
    assert!(file.requests.is_empty(), "{file:?}");
}

#[tokio::test]
async fn accepted_modern_actor_custody_fulfills_requests_but_missing_and_role_custody_remain_visible(
) {
    use nostr::{EventBuilder, Keys};
    use serde_json::json;
    let dir = tempfile::tempdir().expect("isolated provider");
    let channel = Uuid::new_v4();
    let cwd = dir.path().join("work");
    std::fs::create_dir(&cwd).expect("workspace");
    let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
    let mut host = crate::tests::provider(&dir.path().join("state"), Some(&projects));
    let actor = Keys::generate();
    let (relay, _, server) =
        crate::tests::spawn_test_relay_with_events(&host.config.keys, vec![]).await;
    host.set_rest_client(relay.rest_client());
    host.set_relay_self(host.config.keys.public_key().to_hex());
    let seats = dir.path().join("actor-seats.json");
    crate::state::atomic_write(&seats,&serde_json::to_vec(&json!({"version":1,"pending":{"modern-request":{"pubkey":actor.public_key().to_hex(),"nsec":actor.secret_key().to_secret_hex(),"authTag":null,"relayUrl":host.config.relay_url}}})).expect("scoped staging")).expect("one-shot seat");
    host.config.actor_seats_file = Some(seats);
    let event = crate::tests::create_event(&host, channel, "modern-request");
    let mut payload: serde_json::Value =
        serde_json::from_str(&event.content).expect("existing create");
    payload["action"]["actor"] = json!(actor.public_key().to_hex());
    let event = EventBuilder::new(event.kind, payload.to_string())
        .tags(event.tags)
        .sign_with_keys(crate::tests::test_operator_keys())
        .expect("signed create");
    host.handle_command_event(channel, &event)
        .await
        .expect("actual fake ACP startup");
    let accepted = host
        .state
        .sessions()
        .next()
        .expect("accepted generation")
        .clone();
    let actor_dir = host
        .config
        .state_dir
        .join("actors")
        .join(actor.public_key().to_hex());
    std::fs::create_dir_all(&actor_dir).expect("actor directory");
    crate::state::atomic_write(
        &actor_dir.join("key"),
        actor.secret_key().to_secret_hex().as_bytes(),
    )
    .expect("worker custody");
    crate::state::atomic_write(&host.config.state_dir.join("conversation-routes.json"),&serde_json::to_vec(&json!({"routes":[{"actorPubkey":actor.public_key().to_hex(),"channelId":channel,"target":host.target_for(&accepted)}]})).expect("exact accepted route")).expect("route");
    let read = || {
        serde_json::from_str::<SeatRequestsFile>(
            &std::fs::read_to_string(seat_requests_path(&host.config.state_dir))
                .expect("request observation"),
        )
        .expect("requests")
    };
    host.publish_seat_requests();
    assert!(
        read().requests.is_empty(),
        "valid modern worker-owned custody needs no restager"
    );
    // A project association does not turn an attributed Solo actor into a role-seat.
    host.state
        .update_session(&accepted.session_id, |record| {
            record.project_ref = Some(format!(
                "30621:{}:project",
                crate::tests::test_operator_keys().public_key().to_hex()
            ))
        })
        .expect("project association");
    host.publish_seat_requests();
    assert!(read().requests.is_empty());
    std::fs::remove_file(actor_dir.join("key")).expect("simulate unavailable custody");
    host.publish_seat_requests();
    assert_eq!(
        read().requests.len(),
        1,
        "missing custody must not be hidden"
    );
    crate::state::atomic_write(&actor_dir.join("key"), b"invalid custody").expect("invalid key");
    host.publish_seat_requests();
    assert_eq!(
        read().requests.len(),
        1,
        "invalid custody remains unresolved"
    );
    crate::state::atomic_write(
        &actor_dir.join("key"),
        actor.secret_key().to_secret_hex().as_bytes(),
    )
    .expect("restore valid fixture key");
    host.state
        .update_session(&accepted.session_id, |record| {
            record.role = Some("builder".into())
        })
        .expect("role recovery fact");
    host.publish_seat_requests();
    let unresolved = read();
    assert_eq!(unresolved.requests.len(), 1);
    assert_eq!(unresolved.requests[0].role, "builder");
    host.sessions
        .shutdown_and_join(&accepted.session_id, crate::session::ACTOR_RETIRE_GRACE)
        .await;
    server.abort();
}
